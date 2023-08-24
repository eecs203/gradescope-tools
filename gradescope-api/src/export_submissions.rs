use std::path::Path;
use std::{io, thread};

use anyhow::{anyhow, bail, Context as AnyhowContext, Result};
use async_zip::base::read::stream::ZipFileReader;
use futures::channel::mpsc;
use futures::{AsyncBufRead, AsyncRead, SinkExt, Stream, StreamExt, TryStreamExt};
use itertools::Itertools;
use pdf::content::{Op, TextDrawAdjusted};
use pdf::file::{CachedFile, FileOptions};
use pdf::object::{Page, PageRc, Ref, Resolve, XObject};
use pdf::PdfError;
use reqwest::Response;
use tokio::runtime::Handle;
use tracing::{debug, info, trace};

use crate::submission::SubmissionId;
use crate::types::QuestionNumber;

pub async fn download_submission_export(response: Response) -> Result<impl AsyncBufRead + Unpin> {
    Ok(response
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
) -> impl Stream<Item = Result<(SubmissionId, SubmissionPdf)>> {
    files
        .map(|result| {
            tokio_rayon::spawn(move || {
                let (filename, data) = result?;

                let filename_stem = Path::new(&filename)
                    .file_stem()
                    .context("cannot get PDF filename stem")?
                    .to_str()
                    .expect("the stem should be UTF-8 since all of `filename` is");
                let id = SubmissionId::new(filename_stem.to_owned());

                let submission = SubmissionPdf::new(data).context("cannot read file as PDF")?;

                anyhow::Ok((id, submission))
            })
        })
        .buffer_unordered(16)
}

pub struct SubmissionPdf {
    file: CachedFile<Vec<u8>>,
    pages: Vec<PageRc>,
    gradescope_logo_ref: Ref<XObject>,
}

impl SubmissionPdf {
    #[tracing::instrument(level = "debug", skip(pdf_data))]
    pub fn new(pdf_data: Vec<u8>) -> Result<Self> {
        let file = FileOptions::cached().load(pdf_data)?;
        let pages = Self::pages(&file)?;
        let gradescope_logo_ref = Self::gradescope_logo_ref(&file, &pages)?;
        debug!(?gradescope_logo_ref);

        Ok(Self {
            file,
            pages,
            gradescope_logo_ref,
        })
    }

    fn pages(file: &CachedFile<Vec<u8>>) -> Result<Vec<PageRc>> {
        file.pages()
            .try_collect()
            .context("cannot get pages of PDF")
    }

    fn gradescope_logo_ref(file: &CachedFile<Vec<u8>>, pages: &[PageRc]) -> Result<Ref<XObject>> {
        // The assignment summary pages are collectively somewhat like a single page; the footer of
        // only the last one contains a page number and the Gradescope logo. This should be the
        // first appearance of the Gradescope logo in each PDF.
        //
        // It is not necessarily the first appearance of any image, however. In graded work, a
        // grader's comments are indicated with a speech bubble image, which may thus appear in the
        // assignment summary pages. However, the image is a small icon.
        //
        // I conjecture that any image appearing before the Gradescope logo will be a small icon, or
        // at least not the same size as the logo itself. Thus, we can identify the logo by its
        // size.
        //
        // After the assignment summary pages, it is possible (though unlikely) that a student's
        // submission is the exact same size as the logo, so we do want the first image of the right
        // size, not just any.

        const LOGO_WIDTH: u32 = 1280;
        const LOGO_HEIGHT: u32 = 193;

        let logo_ref = pages
            .iter()
            .map(|page| page.resources())
            .map_ok(|resource| resource.xobjects.iter())
            .flatten_ok()
            .map_ok(|(_, xobject_ref)| file.get(*xobject_ref))
            .flatten_ok()
            .filter_map_ok(|xobject| {
                if let XObject::Image(image) = xobject.data().as_ref() {
                    Some((xobject.get_ref(), image.width, image.height))
                } else {
                    None
                }
            })
            .filter_ok(|(_, width, height)| *width == LOGO_WIDTH && *height == LOGO_HEIGHT)
            .map_ok(|(xobject_ref, _, _)| xobject_ref)
            .next()
            .context("could not find Gradescope logo")?
            .context("could not get Gradescope logo")?;

        Ok(logo_ref)
    }

    #[tracing::instrument(level = "trace", skip(self))]
    pub fn question_matching(
        &self,
    ) -> Result<impl Iterator<Item = (MatchingState, QuestionNumber)>> {
        let page_kinds = self.page_kinds()?;
        trace!(?page_kinds);

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
                        unreachable!("dedup should prevent submissions here")
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
        let post_assignment_summary_pages = self.post_assignment_summary_pages()?;

        let page_kinds = self
            .post_assignment_summary_page_kinds(post_assignment_summary_pages)
            .try_collect()?;

        Ok(page_kinds)
    }

    fn post_assignment_summary_pages(&self) -> Result<&[PageRc]> {
        let num_assignment_summary_pages = self.count_assignment_summary_pages()?;
        let (_, post) = self.pages.split_at(num_assignment_summary_pages);
        Ok(post)
    }

    fn post_assignment_summary_page_kinds<'a>(
        &'a self,
        pages: &'a [PageRc],
    ) -> impl Iterator<Item = Result<PageKind>> + 'a {
        pages
            .iter()
            .map(|page| self.post_assignment_summary_page_kind(page))
    }

    #[tracing::instrument(level = "debug", skip(self), fields(num_pages = self.pages.len()), err, ret)]
    fn count_assignment_summary_pages(&self) -> Result<usize> {
        let mut pages = self
            .pages
            .iter()
            .zip(1..)
            .inspect(|(_, page_number)| trace!(page_number))
            .map(|(x, _)| x);

        let num_assignment_summary_pages = pages
            .by_ref()
            .map(move |page| self.is_last_assignment_summary_page(page))
            .take_while_inclusive(|result| !result.as_ref().copied().unwrap_or(false))
            .fold_ok(0, |acc, _| acc + 1)?;

        debug!(num_assignment_summary_pages);

        let just_after = pages.next().ok_or_else(|| {
            anyhow!("no pages after the {num_assignment_summary_pages} assignment summary pages")
        })?;
        if !self.is_just_after_assignment_summary_pages(just_after)? {
            bail!("found unexpected page just after the {num_assignment_summary_pages} assignment summary pages");
        }

        Ok(num_assignment_summary_pages)
    }

    /// When called on a sequence of pages starting with the first page, returns false until the
    /// last of the assignment summary pages, when it returns true.
    fn is_last_assignment_summary_page(&self, page: &Page) -> Result<bool> {
        // The first place the Gradescope logo appears should appear is at the end of the last of
        // the assignment summary pages. See also the comments in the implementation of
        // `Self::gradescope_logo_ref`
        self.has_gradescope_logo(page)
    }

    /// Determines if the given page is likely just after the assignment summary pages. In
    /// particular, if the page does immediately follow the last assignment summary page, returns
    /// true, and if not, is likely, but not guaranteed, to return false.
    fn is_just_after_assignment_summary_pages(&self, page: &Page) -> Result<bool> {
        // The first page after the assignment summary is either a page of student submission (if
        // they matched pages to the first question) or the question summary for the first question
        // (if they did not).
        //
        // If the page is a student submission page, we can't tell from just it alone whether or not
        // it immediately follows an assignment summary page, so we return true, since given only
        // that information, it may.

        Ok(self.is_first_question_summary_page(page) || self.is_student_submission_page(page)?)
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

    #[tracing::instrument(level = "trace", skip(self, page), err, ret)]
    fn is_student_submission_page(&self, page: &Page) -> Result<bool> {
        // If a page doesn't use fonts, it must be a student submission page. Student submissions
        // are always included as images of their rendered PDF submissions, so usually don't have
        // fonts. By contrast, other pages use fonts, since they have text for question numbers,
        // grading, and so on. However, student submission pages sometimes do contain text; when
        // graders leave comments, the location of their comments is marked by a numbered icon.
        //
        // To account for the rare case we do have a student submission page with fonts, we must
        // check the images. Question summary pages always have the Gradescope logo, while student
        // submission pages never do, but always contain at least the image of their work.

        Ok(self.has_no_fonts(page)? || !self.has_gradescope_logo(page)?)
    }

    #[tracing::instrument(level = "trace", skip(self, page), err, ret)]
    fn has_no_fonts(&self, page: &Page) -> Result<bool> {
        Ok(page.resources()?.fonts.is_empty())
    }

    #[tracing::instrument(level = "trace", skip(self, page), err, ret)]
    fn has_gradescope_logo(&self, page: &Page) -> Result<bool> {
        Ok(page
            .resources()?
            .xobjects
            .values()
            .contains(&self.gradescope_logo_ref))
    }

    /// Is this a question summary page for the first question, i.e. the first question summary page
    /// to appear in the PDF?
    #[tracing::instrument(level = "trace", skip(self, page), ret)]
    fn is_first_question_summary_page(&self, page: &Page) -> bool {
        self.get_number_of_question_summary(page)
            .map(|question_number| {
                trace!(%question_number);
                question_number
            })
            .map_err(|err| {
                trace!(question_number_err = %err);
                err
            })
            .as_ref()
            .map_or(false, QuestionNumber::is_first)
    }

    /// Given that the page is a question summary page, get the question number of the question
    /// being summarized.
    fn get_number_of_question_summary(&self, page: &Page) -> Result<QuestionNumber> {
        let Some(first_text) = self.page_ops_text(page)?.next() else {
            bail!("No text on question summary page")
        };
        let question_number = first_text
            .trim()
            .to_owned()
            .parse()
            .context("could not parse question number")?;
        Ok(question_number)
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
