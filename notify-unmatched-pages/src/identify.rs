use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use futures::future::{try_join3, Either};
use futures::{future, stream, FutureExt, StreamExt, TryStreamExt};
use gradescope_api::assignment::{self, Assignment, AssignmentClient};
use gradescope_api::assignment_selector::AssignmentSelector;
use gradescope_api::course::CourseClient;
use gradescope_api::submission_export::pdf::SubmissionPdfStream;
use gradescope_api::submission_export::{submissions_export_load, SubmissionExport};
use gradescope_api::unmatched::UnmatchedSubmissionStream;
use itertools::Itertools;
use tracing::error;

use crate::report::{UnmatchedReport, UnmatchedReportStream};

pub async fn assignments(
    selectors: &[AssignmentSelector],
    assignments: &[Assignment],
    course_client: &CourseClient<'_>,
) -> Result<impl UnmatchedReportStream + '_> {
    Ok(stream::iter(selectors)
        .flat_map_unordered(None, |selector| {
            let fut_res_stream = single_assignment(selector, assignments, course_client);

            stream::once(single_assignment(selector, assignments, course_client)).map(|result| {
                match result {
                    Ok(stream) => Either::Left(stream),
                    Err(err) => Either::Right(stream::iter([Err(err)])),
                }
            })
        })
        .await)
}

pub async fn single_assignment<'a>(
    selector: &AssignmentSelector,
    assignments: &'a [Assignment],
    course_client: &CourseClient<'a>,
) -> Result<impl UnmatchedReportStream + 'a> {
    let assignment = selector.select_from(assignments)?;
    let assignment_client = course_client.with_assignment(assignment);

    let path = save_submissions_to_fs(&assignment_client).await?;
    let reports = find_unsubmitted(assignment_client, path).await?;

    Ok(reports)
}

pub async fn save_submissions_to_fs(client: &AssignmentClient<'_>) -> Result<PathBuf> {
    let path = export_path(client);
    if path.exists() {
        // The file was successfully downloaded on a previous run
        return Ok(path);
    }

    let mut submissions_response = client.export_submissions().await?;

    let tmp_path = path.with_extension("tmp");
    let mut file = fs::File::create(&tmp_path)?;
    while let Some(data) = submissions_response.chunk().await? {
        file.write_all(&data)?;
    }
    std::fs::rename(tmp_path, &path)?;

    Ok(path)
}

pub async fn find_unsubmitted(
    client: AssignmentClient<'_>,
    path: PathBuf,
) -> Result<impl UnmatchedReportStream + '_> {
    let (submission_export, submission_to_student_map, outline) = try_join3(
        submissions_export_load(path),
        client.submission_to_student_map(),
        client.outline(),
    )
    .await?;

    Ok(submission_export
        .submissions()
        .unmatched(outline.into_questions().collect_vec())
        .submitters(submission_to_student_map)
        .map_ok(move |submitter| UnmatchedReport::new(&client, submitter))
        .inspect_err(|err| error!(%err, "error getting nonmatching submitter"))
        .map(future::ready)
        .buffer_unordered(16))
}

fn export_path(client: &AssignmentClient<'_>) -> PathBuf {
    let course = client.course().name();
    let name = client.assignment().name().as_str();
    PathBuf::from(format!("out/{course}-{name}-export.zip"))
}
