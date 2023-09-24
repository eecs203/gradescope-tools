use anyhow::{Context, Result};
use futures::{future, stream, Stream, StreamExt, TryStreamExt};
use gradescope_api::assignment::Assignment;
use gradescope_api::client::{Auth, Client};
use gradescope_api::course::Course;
use gradescope_api::export_submissions::{MatchingState, SubmissionPdf};
use gradescope_api::submission::{SubmissionId, SubmissionsManagerProps};
use gradescope_api::types::QuestionNumber;
use itertools::Itertools;
use tracing::{debug, error, info, trace, warn};

use crate::report::UnmatchedReport;

pub mod report;

pub async fn unmatched_page_reports<'a>(
    gradescope: &'a Client<Auth>,
    course: &'a Course,
    assignment: &'a Assignment,
) -> Result<impl Stream<Item = Result<UnmatchedReport>> + 'a> {
    let metadata = download_submission_metadata(course, assignment, gradescope).await?;
    let submission_to_students = metadata.submission_to_student_map()?;

    let submissions = download_submissions(course, assignment, gradescope).await?;

    let num_unmatched = unmatched_questions(submissions);

    let unmatched_submissions = num_unmatched
        .inspect_ok(|(id, unmatched)| debug!(%id, ?unmatched, "unmatched questions for submission"))
        .try_filter(|(_, unmatched)| future::ready(!unmatched.is_empty()))
        .inspect_ok(|(id, unmatched)| info!(%id, num_unmatched = unmatched.len(), ?unmatched, "found not totally matched submission"));

    let results = unmatched_submissions
        .map_ok(move |(id, unmatched)| {
            let students = submission_to_students.get(&id).with_context(|| {
                format!(
                    "could not find students for submission {id}, with {} mismatched problems: {unmatched:#?}",
                    unmatched.len(),
                )
            })?;

            Ok(UnmatchedReport::new(id, assignment.name().clone(), students, unmatched))
        })
        .and_then(future::ready)
        .inspect_err(|err| error!(%err, "error somewhere"));

    Ok(results)
}

fn unmatched_questions(
    submissions: impl Stream<Item = Result<(SubmissionId, SubmissionPdf)>>,
) -> impl Stream<Item = Result<(SubmissionId, Vec<QuestionNumber>)>> {
    submissions
        .map(|result| {
            tokio_rayon::spawn(move || {
                let (filename, pdf) = result?;
                let matching = pdf
                    .question_matching()
                    .context("cannot get question matching status")?;
                let unmatched = matching
                    .filter(|(matching, _)| matches!(matching, MatchingState::Unmatched))
                    .map(|(_, number)| number)
                    .collect();
                Ok((filename, unmatched))
            })
        })
        .buffer_unordered(16)
}

pub async fn download_submission_metadata(
    course: &Course,
    assignment: &Assignment,
    gradescope: &Client<Auth>,
) -> Result<SubmissionsManagerProps> {
    let metadata = gradescope
        .get_submissions_metadata(course, assignment)
        .await
        .context("could not get submissions")?;

    Ok(metadata)
}

pub async fn download_submissions(
    course: &Course,
    assignment: &Assignment,
    gradescope: &Client<Auth>,
) -> Result<impl Stream<Item = Result<(SubmissionId, SubmissionPdf)>>> {
    let submissions = gradescope
        .export_submissions(course, assignment)
        .await
        .context("could not export submissions")?;

    Ok(submissions)
}
