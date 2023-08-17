use anyhow::{Context, Result};
use app_utils::{init_from_env, init_tracing, InitFromEnv};
use futures::{future, Stream, StreamExt, TryStreamExt};
use gradescope_api::export_submissions::{
    files_as_submissions, read_zip, MatchingState, SubmissionPdf,
};
use gradescope_api::submission::SubmissionsManagerProps;
use lib203::homework::{find_homeworks, HwNumber};
use tokio::fs::{self, File};
use tokio_util::compat::TokioAsyncReadCompatExt;
use tracing::{info, trace, warn};

const MIN_UNMATCHED_TO_NOTIFY: usize = 5;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    // let props_json = fs::read_to_string("out/submissions_manager_props.json")
    //     .await
    //     .unwrap();
    // let props: SubmissionsManagerProps = serde_json::from_str(&props_json).unwrap();

    // println!("{props:?}");

    download_submission_metadata().await?;

    // let submissions = load_zip().await?;
    // // let submissions = download_submissions().await?;

    // let num_unmatched = count_unmatched(submissions);

    // let errors = num_unmatched
    //     .inspect_ok(|(filename, num)| trace!(filename, num, "counted unmatched for submission"))
    //     .try_filter(|(_, unmatched)| future::ready(*unmatched >= 1))
    //     .inspect_ok(|(filename, num)| info!(filename, num, "found not totally matched submission"))
    //     .try_filter(|(_, unmatched)| future::ready(*unmatched >= MIN_UNMATCHED_TO_NOTIFY))
    //     .inspect_ok(|(filename, num)| warn!(filename, num, "Unmatched!"))
    //     .filter_map(|result| async move { result.err() })
    //     .collect::<Vec<_>>()
    //     .await;

    // println!("{errors:?}");

    Ok(())
}

fn count_unmatched(
    submissions: impl Stream<Item = Result<(String, SubmissionPdf)>>,
) -> impl Stream<Item = Result<(String, usize)>> {
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

async fn download_submission_metadata() -> Result<()> {
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
        .get_submissions_metadata(&course, hw_1)
        .await
        .context("could not get submissions")?;

    let submissions_to_students = submissions.submission_to_student_map()?;

    println!("mapping: {submissions_to_students:#?}");

    Ok(())
}

async fn download_submissions() -> Result<impl Stream<Item = Result<(String, SubmissionPdf)>>> {
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

async fn load_zip() -> Result<impl Stream<Item = Result<(String, SubmissionPdf)>>> {
    let zip = File::open("out/submissions.zip").await?.compat();
    let files = read_zip(zip);
    let submissions = files_as_submissions(files);
    Ok(submissions)
}
