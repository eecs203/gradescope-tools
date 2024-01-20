use std::path::Path;

use anyhow::{bail, Context as AnyhowContext, Result};
use futures::{Stream, StreamExt, TryStreamExt};
use itertools::Itertools;
use pdf::content::{Op, TextDrawAdjusted};
use pdf::file::{CachedFile, FileOptions};
use pdf::object::{Page, PageRc};
use pdf::PdfError;

use crate::submission::SubmissionId;
use crate::types::QuestionNumber;
use crate::unmatched::{UnmatchedQuestion, UnmatchedSubmission, UnmatchedSubmissionStream};

pub struct SubmissionPdf {
    submission_id: SubmissionId,
    file: CachedFile<Vec<u8>>,
    pages: Vec<PageRc>,
}

impl SubmissionPdf {
    #[tracing::instrument(level = "debug", skip(pdf_data))]
    pub fn new(filename: String, pdf_data: Vec<u8>) -> Result<Self> {
        let filename_stem = Path::new(&filename)
            .file_stem()
            .context("cannot get PDF filename stem")?
            .to_str()
            .expect("the stem should be UTF-8 since all of `filename` is");
        let submission_id = SubmissionId::new(filename_stem.to_owned());

        let file = FileOptions::cached().load(pdf_data)?;
        let pages = Self::pages(&file)?;

        Ok(Self {
            submission_id,
            file,
            pages,
        })
    }

    pub fn as_unmatched(&self) -> Result<Option<UnmatchedSubmission>> {
        let unmatched_questions: Vec<_> = self.unmatched_questions().try_collect()?;
        if !unmatched_questions.is_empty() {
            let id = self.id().clone();
            Ok(Some(UnmatchedSubmission::new(id, unmatched_questions)))
        } else {
            Ok(None)
        }
    }

    pub fn id(&self) -> &SubmissionId {
        &self.submission_id
    }

    pub fn unmatched_questions(&self) -> impl Iterator<Item = Result<UnmatchedQuestion>> + '_ {
        self.matched_question_numbers()
            .map_ok(UnmatchedQuestion::new)
    }

    #[tracing::instrument(level = "trace", skip(self))]
    pub fn matched_question_numbers(&self) -> impl Iterator<Item = Result<QuestionNumber>> + '_ {
        self.pages
            .iter()
            .filter_map(|page| self.summary_page_questions(page).transpose())
            .flatten_ok()
    }

    fn pages(file: &CachedFile<Vec<u8>>) -> Result<Vec<PageRc>> {
        file.pages()
            .try_collect()
            .context("cannot get pages of PDF")
    }

    #[tracing::instrument(level = "debug", skip(self), fields(num_pages = self.pages.len()), err, ret)]
    fn count_assignment_summary_pages(&self) -> Result<usize> {
        self.pages
            .iter()
            .map(|page| self.is_just_after_assignment_summary_pages(page))
            .take_while(|result| !result.as_ref().copied().unwrap_or(false))
            .fold_ok(0, |acc, _| acc + 1)
    }

    /// Determines if the given page is just after the assignment summary pages. In particular:
    /// - if the page is an assignment summary page, returns false
    /// - if the page is just after the last assignment summary page, returns true
    /// - otherwise, there is no guarantee as to the return value
    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    fn is_just_after_assignment_summary_pages(&self, page: &Page) -> Result<bool> {
        // Suppose `page` is just after the assignment summary. Then the student must have submitted
        // a nonzero number of pages, and their first page must be preceded by a summary page, so
        // `page` is a summary page.
        //
        // Conversely, all assignment summary pages are not summary pages, so we will not return
        // `true` too early. We only have to guarantee behavior until one past the end of the
        // assignment summary, so it doesn't matter whether or not there are further summary pages.

        Ok(self.summary_page_questions(page)?.is_some())
    }

    /// Returns `Some` with a list of question numbers matched to the page, or `None` if it is not a
    /// summary page. See also [`PageKind::Summary`].
    #[tracing::instrument(level = "debug", skip(self), err, ret)]
    fn summary_page_questions(&self, page: &Page) -> Result<Option<Vec<QuestionNumber>>> {
        println!(
            "{}",
            self.page_ops_text(page).unwrap().collect_vec().join("")
        );
        Ok(None)
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

pub trait SubmissionPdfStream: Stream<Item = Result<SubmissionPdf>> + Sized {
    fn unmatched(
        self,
    ) -> UnmatchedSubmissionStream<impl Stream<Item = Result<UnmatchedSubmission>>> {
        UnmatchedSubmissionStream::new(
            self.map(|result| tokio_rayon::spawn(move || result?.as_unmatched()))
                .buffer_unordered(16)
                .try_filter_map(|option_unmatched| async move { Ok(option_unmatched) }),
        )
    }
}

impl<S: Stream<Item = Result<SubmissionPdf>>> SubmissionPdfStream for S {}
