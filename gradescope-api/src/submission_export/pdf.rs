use std::ops::RangeFrom;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{Context as AnyhowContext, Result, anyhow};
use futures::{Stream, StreamExt, TryStreamExt};
use itertools::Itertools;
use nom::branch::alt;
use nom::bytes::complete::tag;
use nom::character::complete::{anychar, char, digit1, multispace0};
use nom::combinator::{eof, map_res};
use nom::error::ParseError;
use nom::multi::{many_till, many0, separated_list0, separated_list1};
use nom::sequence::{delimited, preceded, tuple};
use nom::{AsChar, IResult, InputIter, InputLength, Parser, Slice};

use crate::question::{Question, QuestionNumber};
use crate::submission::SubmissionId;
use crate::unmatched::{UnmatchedQuestion, UnmatchedSubmission, UnmatchedSubmissionStream};

pub struct SubmissionPdf {
    submission_id: SubmissionId,
    text: String,
}

impl SubmissionPdf {
    #[tracing::instrument(level = "debug", skip(pdf_data))]
    pub fn new(filename: String, pdf_data: &[u8]) -> Result<Self> {
        let filename_stem = Path::new(&filename)
            .file_stem()
            .context("cannot get PDF filename stem")?
            .to_str()
            .expect("the stem should be UTF-8 since all of `filename` is");
        let submission_id = SubmissionId::new(filename_stem.to_owned());

        let mut text =
            pdf_extract::extract_text_from_mem(pdf_data).context("could not parse data as PDF")?;
        // Remove whitespace for reliability. See also `Self::matched_question_numbers`.
        text.retain(|c| !c.is_whitespace());

        Ok(Self {
            submission_id,
            text,
        })
    }

    pub fn as_unmatched(&self, all_questions: &[Question]) -> Result<Option<UnmatchedSubmission>> {
        let unmatched_questions = self.unmatched_questions(all_questions)?.collect_vec();
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

    pub fn unmatched_questions<'a>(
        &self,
        all_questions: &'a [Question],
    ) -> Result<impl Iterator<Item = UnmatchedQuestion> + 'a> {
        let mut matched = self.matched_question_numbers()?;
        // Allow for binary search later
        matched.sort_unstable();
        matched.dedup();

        Ok(all_questions
            .iter()
            .filter(move |question| matched.binary_search(question.number()).is_err())
            .cloned()
            .map(UnmatchedQuestion::new))
    }

    /// Ideally, the PDFs would provide structured data on which questions are matched to which
    /// pages. Unfortunately, they do not do this, and are actually difficult to get anything out
    /// of. Because of Gradescope's PDF creation process (apparently ultimately via Skia), and the
    /// way PDFs handle text, each character is rendered separately, and encoded not via UTF-8 or
    /// ASCII, but by a font-based mapping from bytes to symbols.
    ///
    /// Ideally we would process PDFs page-by-page for extra certainty that we're processing things
    /// correctly. However, the only library I could find which correctly implemented a mapping from
    /// the character encoding to UTF-8 doesn't seem to provide this functionality, but only gives
    /// a single string containing all text from the PDF. Moreover, because each character is
    /// rendered independently, whitespace is simply encoded as rendering the next character farther
    /// away, so while the library makes a good attempt, there isn't a truly reliable way to detect
    /// whitespace.
    ///
    /// So, the text we're parsing is a string of all text in the PDF, with all whitespace removed.
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn matched_question_numbers(&self) -> Result<Vec<QuestionNumber>> {
        pdf_text(&self.text)
            .map(|(_, question_nums)| question_nums)
            .map_err(|err| anyhow!("could not parse question numbers from PDF text: {err}"))
    }
}

fn pdf_text(text: &str) -> IResult<&str, Vec<QuestionNumber>> {
    delimited(
        // "Total Points" appears near the top of each PDF
        skip_thru(tag_ws("Total Points")),
        questions,
        eof,
    )(text)
}

fn questions(text: &str) -> IResult<&str, Vec<QuestionNumber>> {
    many0(page)
        .map(|question_nums| question_nums.into_iter().flatten().collect_vec())
        .parse(text)
}

fn page(text: &str) -> IResult<&str, Vec<QuestionNumber>> {
    preceded(skip_thru_page_label, question_num_list)(text)
}

fn skip_thru_page_label(text: &str) -> IResult<&str, ()> {
    skip_thru(alt((
        tag_ws("Questions assigned to the following page:"),
        tag_ws("Question assigned to the following page:"),
        tag_ws("No questions assigned to the following page."),
    )))
    .map(|_| ())
    .parse(text)
}

fn question_num_list(text: &str) -> IResult<&str, Vec<QuestionNumber>> {
    let comma = tuple((multispace0, char(','), multispace0)).map(|_| ());
    let comma_and = tuple((
        multispace0,
        char(','),
        multispace0,
        tag_ws("and"),
        multispace0,
    ))
    .map(|_| ());
    let and = tuple((multispace0, tag_ws("and"), multispace0)).map(|_| ());
    let list_sep = alt((comma_and, comma, and));
    separated_list0(list_sep, question_num)(text)
}

fn question_num(text: &str) -> IResult<&str, QuestionNumber> {
    map_res(separated_list1(char('.'), digit1), |parts| {
        QuestionNumber::from_str(&parts.join("."))
    })(text)
}

fn skip_thru<I, O, E>(mut comb: impl Parser<I, O, E>) -> impl FnMut(I) -> IResult<I, (), E>
where
    I: Clone + InputIter + InputLength + Slice<RangeFrom<usize>>,
    <I as InputIter>::Item: AsChar,
    E: ParseError<I>,
{
    let mut comb_fn = move |input| comb.parse(input);
    move |input| many_till(anychar, &mut comb_fn).map(|_| ()).parse(input)
}

/// Like [`tag`], but strips out whitespace in the given string.
fn tag_ws(target: &str) -> impl FnMut(&str) -> IResult<&str, ()> {
    let target = target.split_whitespace().join("");
    move |text| tag(target.as_str()).map(|_| ()).parse(text)
}

pub trait SubmissionPdfStream: Stream<Item = Result<SubmissionPdf>> + Send + Sized {
    fn unmatched(self, all_questions: Vec<Question>) -> impl UnmatchedSubmissionStream {
        let all_questions = Arc::new(all_questions);
        self.map(move |result| {
            let all_questions = Arc::clone(&all_questions);
            tokio_rayon::spawn(move || result?.as_unmatched(&Arc::clone(&all_questions)))
        })
        .buffer_unordered(16)
        .try_filter_map(|option_unmatched| async move { Ok(option_unmatched) })
    }
}

impl<S: Stream<Item = Result<SubmissionPdf>> + Send> SubmissionPdfStream for S {}
