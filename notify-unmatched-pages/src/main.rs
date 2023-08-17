use anyhow::{Context, Result};
use app_utils::{init_from_env, init_tracing, InitFromEnv};
use futures::{future, stream, Stream, StreamExt, TryStream, TryStreamExt};
use gradescope_api::export_submissions::{
    files_as_submissions, read_zip, MatchingState, SubmissionPdf,
};
use gradescope_api::submission::{StudentSubmitter, SubmissionId, SubmissionsManagerProps};
use lib203::homework::{find_homeworks, HwNumber};
use tokio::fs::{self, File};
use tokio_util::compat::TokioAsyncReadCompatExt;
use tracing::{error, info, trace, warn};

use crate::report::UnmatchedReport;

pub mod report;

const MIN_UNMATCHED_TO_NOTIFY: usize = 5;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let metadata = load_submission_metadata().await?;
    // let metadata = download_submission_metadata().await?;

    let submission_to_students = metadata.submission_to_student_map()?;

    let submissions = load_zip().await?;
    // let submissions = download_submissions().await?;

    let num_unmatched = count_unmatched(submissions);

    let unmatched_submissions = num_unmatched
        .inspect_ok(|(id, num)| trace!(%id, num, "counted unmatched for submission"))
        .try_filter(|(_, num)| future::ready(*num >= 1))
        .inspect_ok(|(id, num)| info!(%id, num, "found not totally matched submission"))
        .try_filter(|(_, num)| future::ready(*num >= MIN_UNMATCHED_TO_NOTIFY))
        .inspect_ok(|(id, num)| warn!(%id, num, "Unmatched submission!"));

    let unmatched_students = unmatched_submissions
        .map_ok(|(id, num)| {
            let students = submission_to_students.get(&id).with_context(|| {
                format!(
                    "could not find students for submission {id}, with {num} mismatched problems"
                )
            })?;

            Ok((id, students, num))
        })
        .and_then(future::ready);

    let results = unmatched_students
        .map_ok(UnmatchedReport::new)
        .inspect_err(|err| error!(%err, "error somewhere"))
        .collect::<Vec<_>>()
        .await;

    println!("Reports:");
    for report in results.iter().flatten() {
        println!("{report}");
        println!("\n----------\n");
    }
    println!();

    println!("Errors:");
    for result in results.iter() {
        if let Err(err) = result {
            println!("{err}");
        }
    }

    Ok(())
}

fn count_unmatched(
    submissions: impl Stream<Item = Result<(SubmissionId, SubmissionPdf)>>,
) -> impl Stream<Item = Result<(SubmissionId, usize)>> {
    submissions
        .map(|result| {
            tokio_rayon::spawn(move || {
                let (filename, pdf) = result?;
                let matching = pdf
                    .question_matching()
                    .context("cannot get question matching status")?;
                let num_unmatched = matching
                    .filter(|(matching, _)| matches!(matching, MatchingState::Unmatched))
                    .count();
                Ok((filename, num_unmatched))
            })
        })
        .buffer_unordered(512)
}

async fn download_submission_metadata() -> Result<SubmissionsManagerProps> {
    let InitFromEnv {
        course,
        gradescope,
        course_name: _,
    } = init_from_env().await?;

    let assignments = gradescope
        .get_assignments(&course)
        .await
        .context("could not get assignments")?;
    let homeworks = find_homeworks(&assignments);

    let hw_1 = homeworks
        .get(&HwNumber::new("1"))
        .context("could not find HW 1")?
        .individual()
        .context("could not find Individual HW 1")?;

    let metadata = gradescope
        .get_submissions_metadata(&course, hw_1)
        .await
        .context("could not get submissions")?;

    Ok(metadata)
}

async fn load_submission_metadata() -> Result<SubmissionsManagerProps> {
    let props_json = fs::read_to_string("out/submissions_manager_props.json").await?;
    let metadata = serde_json::from_str(&props_json)?;
    Ok(metadata)
}

async fn download_submissions() -> Result<impl Stream<Item = Result<(SubmissionId, SubmissionPdf)>>>
{
    let InitFromEnv {
        course,
        gradescope,
        course_name: _,
    } = init_from_env().await?;

    let assignments = gradescope
        .get_assignments(&course)
        .await
        .context("could not get assignments")?;
    let homeworks = find_homeworks(&assignments);

    let hw_1 = homeworks
        .get(&HwNumber::new("1"))
        .context("could not find HW 1")?
        .individual()
        .context("could not find Individual HW 1")?;

    let submissions = gradescope
        .export_submissions(&course, hw_1)
        .await
        .context("could not export submissions")?;

    Ok(submissions)
}

async fn load_zip() -> Result<impl Stream<Item = Result<(SubmissionId, SubmissionPdf)>>> {
    let zip = File::open("out/submissions.zip").await?.compat();
    let files = read_zip(zip);
    let submissions = files_as_submissions(files);
    Ok(submissions)
}
