use std::time::Duration;
use std::{io, thread};

use anyhow::{bail, Context as AnyhowContext, Result};
use async_zip::base::read::stream::ZipFileReader;
use futures::channel::mpsc;
use futures::{AsyncBufRead, AsyncRead, SinkExt, Stream, StreamExt, TryStreamExt};
use itertools::Itertools;
use pdf::content::{Op, TextDrawAdjusted};
use pdf::file::{File, FileOptions, NoCache};
use pdf::object::{Page, PageRc};
use pdf::PdfError;
use reqwest::RequestBuilder;
use tokio::runtime::Handle;
use tracing::{info, trace};

use crate::types::QuestionNumber;

pub async fn download_submission_export(
    request: RequestBuilder,
) -> Result<impl AsyncBufRead + Unpin> {
    Ok(request
        .timeout(Duration::from_secs(30 * 60))
        .send()
        .await
        .context("export download failed")?
        .bytes_stream()
        .inspect_ok(|bytes| trace!(num_bytes = bytes.len(), "got byte chunk"))
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
        .into_async_read())
}

pub fn read_zip(
    zip_data: impl AsyncRead + Unpin + Send + 'static,
) -> impl Stream<Item = Result<(String, Vec<u8>)>> {
    let (sender, receiver) = mpsc::unbounded();
    let handle = Handle::current();

    thread::spawn(move || {
        handle.block_on(async move {
            let send = |result| async { sender.clone().feed(result).await.unwrap() };

            let mut zip = ZipFileReader::new(zip_data);
            loop {
                match zip.next_with_entry().await {
                    Ok(Some(mut zip_reading)) => {
                        let reader = zip_reading.reader_mut();
                        let entry = reader.entry();

                        match entry.filename().as_str() {
                            Err(err) => {
                                send(Err(err).context("cannot decode zip entry filename")).await
                            }
                            Ok(filename) => {
                                if filename.ends_with(".yml") {
                                    info!(filename, "skipping metadata file");
                                } else if !filename.ends_with(".pdf") {
                                    info!(filename, "skipping non-PDF zip entry");
                                } else {
                                    let filename = filename.to_owned();
                                    let mut buf = Vec::new();
                                    let result = reader.read_to_end_checked(&mut buf).await;
                                    match result {
                                        Err(err) => {
                                            send(
                                                Err(err).context("cannot read zip entry file data"),
                                            )
                                            .await
                                        }
                                        Ok(_) => send(Ok((filename, buf))).await,
                                    }
                                }
                            }
                        };

                        let result = zip_reading.skip().await;
                        zip = match result {
                            Ok(zip) => zip,
                            Err(err) => {
                                send(Err(err).context("cannot skip to next zip entry")).await;
                                break;
                            }
                        };
                    }
                    Err(err) => {
                        send(Err(err).context("cannot read next zip entry")).await;
                        break;
                    }
                    Ok(None) => break,
                }
            }
        });
    });

    receiver
}

pub fn files_as_submissions(
    files: impl Stream<Item = Result<(String, Vec<u8>)>>,
) -> impl Stream<Item = Result<(String, SubmissionPdf)>> {
    files
        .map(|result| {
            tokio_rayon::spawn(move || {
                let (filename, data) = result?;
                let submission = SubmissionPdf::new(data).context("cannot read file as PDF")?;
                anyhow::Ok((filename, submission))
            })
        })
        .buffer_unordered(512)
}

pub struct SubmissionPdf {
    file: File<Vec<u8>, NoCache, NoCache>,
}

impl SubmissionPdf {
    pub fn new(pdf_data: Vec<u8>) -> Result<Self> {
        let file = FileOptions::uncached().load(pdf_data)?;

        Ok(Self { file })
    }

    pub fn question_matching(
        &self,
    ) -> Result<impl Iterator<Item = (MatchingState, QuestionNumber)>> {
        let page_kinds = self.page_kinds()?;

        let questions_by_matching = page_kinds
            .into_iter()
            .dedup()
            .batching(|it| match it.next() {
                Some(PageKind::StudentSubmission) => match it.next() {
                    Some(PageKind::QuestionSummary(number)) => {
                        // Submission(s), Summary => submission pages were matched to question
                        Some((MatchingState::Matched, number))
                    }
                    None => None, // These pages aren't matched to a question; not a problem
                    Some(PageKind::StudentSubmission) => {
                        panic!("dedup should prevent submissions here")
                    }
                },
                Some(PageKind::QuestionSummary(number)) => {
                    // no submissions, Summary => no submission pages were matched to question
                    Some((MatchingState::Unmatched, number))
                }
                None => None,
            });

        Ok(questions_by_matching)
    }

    fn page_kinds(&self) -> Result<Vec<PageKind>> {
        let pages: Vec<_> = self
            .file
            .pages()
            .try_collect()
            .context("cannot get page of PDF")?;

        let (_, post_assignment_summary_pages) = self.split_pages(&pages)?;

        let page_kinds = self
            .post_assignment_summary_page_kinds(post_assignment_summary_pages)
            .try_collect()?;

        Ok(page_kinds)
    }

    fn split_pages<'a>(&self, pages: &'a [PageRc]) -> Result<(&'a [PageRc], &'a [PageRc])> {
        let num_assignment_summary_pages = self.count_assignment_summary_pages(pages)?;
        let split = pages.split_at(num_assignment_summary_pages);
        Ok(split)
    }

    fn post_assignment_summary_page_kinds<'a>(
        &'a self,
        pages: &'a [PageRc],
    ) -> impl Iterator<Item = Result<PageKind>> + 'a {
        pages
            .iter()
            .map(|page| self.post_assignment_summary_page_kind(page))
    }

    fn count_assignment_summary_pages(&self, pages: &[PageRc]) -> Result<usize> {
        let first_page = pages.first().context("PDF has no pages")?;
        let first_question_number = self
            .get_first_question_number_from_first_page(first_page)
            .context("cannot get first question number")?;

        let num_assignment_summary_pages = pages
            .iter()
            .map(move |page| self.is_assignment_summary_page(page, &first_question_number))
            .take_while(|result| result.as_ref().copied().unwrap_or(true))
            .fold_ok(0, |acc, _| acc + 1)?;

        Ok(num_assignment_summary_pages)
    }

    /// When applied to a sequence of pages starting just after the first one, determines if it is
    /// still an assignment summary page, or if those have just ended.
    fn is_assignment_summary_page(
        &self,
        page: &Page,
        first_question: &QuestionNumber,
    ) -> Result<bool> {
        // The first page after the assignment summary is either a page of student submission (if
        // they matched pages to the first question) or the question summary for the first question
        // (if they did not).
        Ok(!self.is_first_question_summary_page(page, first_question)?
            && !self.is_student_submission_page(page)?)
    }

    /// For pages after the assignment summary, determine their kind
    fn post_assignment_summary_page_kind(&self, page: &Page) -> Result<PageKind> {
        if self.is_student_submission_page(page)? {
            Ok(PageKind::StudentSubmission)
        } else {
            // Must be question summary page, since it's not student submission and we're after the
            // assignment summary.
            self.get_number_of_question_summary(page)
                .map(PageKind::QuestionSummary)
        }
    }

    fn is_student_submission_page(&self, page: &Page) -> Result<bool> {
        // The submission pages don't use fonts, since student submissions are always included as
        // images of their rendered PDF submissions. By contrast, other pages use fonts, since they
        // have text. So, we detect submission pages by their absence of fonts.
        Ok(page.resources()?.fonts.is_empty())
    }

    /// Is this a question summary page for the first question, i.e. the first question summary page
    /// to appear in the PDF?
    fn is_first_question_summary_page(
        &self,
        page: &Page,
        first_question: &QuestionNumber,
    ) -> Result<bool> {
        let Some(first_text) = self.page_ops_text(page)?.next() else { return Ok(false) };
        Ok(first_text.trim() == first_question.as_str())
    }

    /// Given that the page is a question summary page, get the question number of the question
    /// being summarized.
    fn get_number_of_question_summary(&self, page: &Page) -> Result<QuestionNumber> {
        let Some(first_text) = self.page_ops_text(page)?.next() else {
            bail!("No text on question summary page")
        };
        let question_number_text = first_text.trim().to_owned();
        Ok(QuestionNumber::new(question_number_text))
    }

    /// Gets the number for the first question (or its first part) given the first page of the PDF
    fn get_first_question_number_from_first_page(&self, page: &Page) -> Result<QuestionNumber> {
        self.page_ops_text(page)?
            .skip_while(|text| text != "TOTAL POINTS")
            .skip_while(|text| text != "QUESTION 1")
            .nth(3) // after QUESTION 1, question name, number of points
            .context("could not find first question number")
            .map(|num| num.trim().to_owned())
            .map(QuestionNumber::new)
    }

    fn page_ops_text(&self, page: &Page) -> Result<impl Iterator<Item = String>, PdfError> {
        Ok(self
            .page_ops(page)?
            .into_iter()
            .filter_map(Self::op_to_text))
    }

    fn page_ops(&self, page: &Page) -> Result<Vec<Op>, PdfError> {
        match &page.contents {
            Some(contents) => contents.operations(&self.file),
            None => Ok(vec![]),
        }
    }

    fn op_to_text(op: Op) -> Option<String> {
        // TODO: per the docs of `PdfString::to_string_lossy`, there is a more correct way to
        // convert them to strings by finding their actual encoding
        match op {
            Op::TextDraw { text } => Some(text.to_string_lossy()),
            Op::TextDrawAdjusted { array } => {
                let text = array
                    .iter()
                    .filter_map(|adjusted| match adjusted {
                        TextDrawAdjusted::Text(text) => Some(text.to_string_lossy()),
                        _ => None,
                    })
                    .join(" ");
                Some(text)
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum MatchingState {
    Matched,
    Unmatched,
}

/// Kinds of pages after the assignment summary pages, which are the first few, listing the
/// assignment's name, student's name, score (if graded), every question with its rubric, and how
/// each question scored on its rubric (if graded).
#[derive(Debug, Clone, PartialEq, Eq)]
enum PageKind {
    /// One page summarizing a single question (or part). Contains the same information about the
    /// question as the assignment summary, but just for the one question.
    ///
    /// Comes immediately after all student submission pages matched to that question; these will be
    /// copied and repeated if the same page was matched to several questions. When no pages were
    /// matched to a question, no student submission page will appear immediately before it, so we
    /// will see two question summary pages back-to-back.
    QuestionSummary(QuestionNumber),
    /// One of the pages of the student's submission. Notably, Gradescope renders submitted PDFs as
    /// images, then uses those images for these pages. For that reason, there will never be text on
    /// a student submission page, which we can use to detect them.
    StudentSubmission,
}
