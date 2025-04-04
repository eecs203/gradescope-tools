use anyhow::Result;
use futures::future::{Either, try_join3};
use futures::{FutureExt, StreamExt, TryStreamExt, future, stream};
use gradescope_api::assignment::Assignment;
use gradescope_api::course::CourseClient;
use gradescope_api::services::gs_service::GsService;
use gradescope_api::submission_export::SubmissionExport;
use gradescope_api::submission_export::pdf::SubmissionPdfStream;
use gradescope_api::unmatched::UnmatchedSubmissionStream;
use itertools::Itertools;
use tracing::error;

use crate::report::{UnmatchedReport, UnmatchedReportStream};

pub async fn report_unmatched_many_assignments<'a>(
    assignments: &'a [&'a Assignment],
    course_client: &'a CourseClient<'a, impl GsService>,
) -> impl UnmatchedReportStream + 'a {
    stream::iter(assignments).flat_map_unordered(None, |assignment| {
        Box::pin(report_unmatched(assignment, course_client).flatten_stream())
    })
}

async fn report_unmatched<'a>(
    assignment: &'a Assignment,
    course_client: &CourseClient<'a, impl GsService>,
) -> impl UnmatchedReportStream + 'a {
    match report_unmatched_helper(assignment, course_client).await {
        Ok(stream) => Either::Left(stream),
        Err(err) => Either::Right(stream::iter([Err(err)])),
    }
}

async fn report_unmatched_helper<'a>(
    assignment: &'a Assignment,
    course_client: &CourseClient<'a, impl GsService>,
) -> Result<impl UnmatchedReportStream + 'a> {
    let assignment_client = course_client.with_assignment(assignment);

    assignment_client.ensure_submissions_export_on_fs().await?;

    let (submission_export, submission_to_student_map, outline) = try_join3(
        assignment_client.load_submission_export_from_fs(),
        assignment_client.submission_to_student_map(),
        assignment_client.outline(),
    )
    .await?;

    let reports = submission_export
        .submissions()
        .unmatched(outline.into_questions().collect_vec())
        .submitters(submission_to_student_map)
        .map_ok(move |submitter| UnmatchedReport::new(&assignment_client, submitter))
        .inspect_err(|err| error!(%err, "error getting nonmatching submitter"))
        .map(future::ready)
        .buffer_unordered(16);

    Ok(reports)
}
