use std::iter;

use anyhow::{bail, Context, Result};
use async_zip::base::read::stream::ZipFileReader;
use futures::AsyncRead;
use itertools::Itertools;
use pdf::content::{Op, TextDrawAdjusted};
use pdf::file::{File, FileOptions, NoCache};
use pdf::object::{Page, PageRc};
use pdf::PdfError;
use tracing::info;

use crate::types::QuestionNumber;

pub async fn read_zip(zip_data: impl AsyncRead + Unpin) -> Result<()> {
    let mut zip = ZipFileReader::new(zip_data);
    while let Some(zip_reading) = zip
        .next_with_entry()
        .await
        .context("could not read next zip entry")?
    {
        let entry = zip_reading.reader().entry();
        let filename = entry.filename().as_str()?;
        info!(filename, "read file entry from zip");

        zip = zip_reading.skip().await?;
    }

    Ok(())
}

pub struct SubmissionPdfReader {
    file: File<Vec<u8>, NoCache, NoCache>,
}

impl SubmissionPdfReader {
    pub fn new(pdf_data: Vec<u8>) -> Result<Self> {
        let file = FileOptions::uncached().load(pdf_data)?;

        Ok(Self { file })
    }

    pub fn unmatched_pages(&self) -> Result<()> {
        let page_kinds: Vec<_> = self.page_kinds()?;
        println!("kinds: {page_kinds:#?}");
        Ok(())
    }

    fn page_kinds(&self) -> Result<Vec<PageKind>> {
        let pages: Vec<_> = self
            .file
            .pages()
            .try_collect()
            .context("cannot get page of PDF")?;

        let (assignment_summary_pages, post_assignment_summary_pages) = self.split_pages(&pages)?;

        let assignment_summary_page_kinds =
            self.assignment_summary_page_kinds(assignment_summary_pages);
        let post_assignment_summary_page_kinds =
            self.post_assignment_summary_page_kinds(post_assignment_summary_pages);
        let page_kinds = assignment_summary_page_kinds
            .chain(post_assignment_summary_page_kinds)
            .try_collect()?;

        Ok(page_kinds)
    }

    fn split_pages<'a>(&self, pages: &'a [PageRc]) -> Result<(&'a [PageRc], &'a [PageRc])> {
        let num_assignment_summary_pages = self.count_assignment_summary_pages(pages)?;
        let split = pages.split_at(num_assignment_summary_pages);
        Ok(split)
    }

    fn assignment_summary_page_kinds(
        &self,
        assignment_summary_pages: &[PageRc],
    ) -> impl Iterator<Item = Result<PageKind>> {
        iter::repeat(PageKind::AssignmentSummary)
            .take(assignment_summary_pages.len())
            .map(Ok)
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

#[derive(Debug, Clone)]
enum PageKind {
    /// The first few pages, listing the assignment's name, student's name, score (if graded), every
    /// question with its rubric, and how each question scored on its rubric (if graded).
    AssignmentSummary,
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
