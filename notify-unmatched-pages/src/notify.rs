use anyhow::Result;
use futures::{future, Stream, StreamExt, TryStreamExt};
use gradescope_api::assignment::AssignmentClient;
use gradescope_api::question::Outline;
use gradescope_api::submission::SubmissionToStudentMap;
use gradescope_api::submission_export::pdf::SubmissionPdfStream;
use gradescope_api::submission_export::SubmissionExport;
use gradescope_api::unmatched::UnmatchedSubmissionStream;
use itertools::Itertools;
use tracing::error;

use crate::report::UnmatchedReport;

pub fn find_unsubmitted<'a>(
    client: &'a AssignmentClient<'a>,
    submission_export: impl SubmissionExport,
    submission_to_student_map: SubmissionToStudentMap,
    outline: Outline,
) -> impl Stream<Item = Result<UnmatchedReport<'a>>> {
    submission_export
        .submissions()
        .unmatched(outline.into_questions().collect_vec())
        .submitters(submission_to_student_map)
        .map_ok(|submitter| UnmatchedReport::new(client, submitter))
        .inspect_err(|err| error!(%err, "error getting nonmatching submitter"))
        .map(future::ready)
        .buffer_unordered(16)
}
