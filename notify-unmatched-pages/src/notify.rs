use anyhow::Result;
use futures::{future, AsyncRead, Stream, StreamExt, TryStreamExt};
use gradescope_api::assignment::AssignmentClient;
use gradescope_api::submission::SubmissionToStudentMap;
use gradescope_api::submission_export::pdf::SubmissionPdfStream;
use gradescope_api::submission_export::SubmissionExport;
use tracing::error;

use crate::report::UnmatchedReport;

pub fn find_unsubmitted<'a>(
    assignment_client: &'a AssignmentClient<'a>,
    submission_export: SubmissionExport<impl AsyncRead + Unpin + Send + 'static>,
    submission_to_student_map: SubmissionToStudentMap,
) -> impl Stream<Item = Result<UnmatchedReport<'a>>> {
    submission_export
        .submissions()
        .unmatched()
        .submitters(submission_to_student_map)
        .map_ok(|submitter| {
            UnmatchedReport::new(
                assignment_client.course(),
                assignment_client.assignment(),
                submitter,
            )
        })
        .inspect_err(|err| error!(%err, "error getting nonmatching submitter"))
        .map(future::ready)
        .buffer_unordered(16)
}
