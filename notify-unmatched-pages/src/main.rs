use anyhow::{Context, Result};
use app_utils::{init_from_env, init_tracing, InitFromEnv};
use futures::{future, Stream, StreamExt, TryStreamExt};
use gradescope_api::export_submissions::{
    files_as_submissions, read_zip, MatchingState, SubmissionPdf,
};
use lib203::homework::{find_homeworks, HwNumber};
use tokio::fs::File;
use tokio_util::compat::TokioAsyncReadCompatExt;
use tracing::{info, trace, warn};

const MIN_UNMATCHED_TO_NOTIFY: usize = 5;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let submissions = load_zip().await?;
    // let submissions = download_submissions().await?;

    let num_unmatched = count_unmatched(submissions);

    let errors = num_unmatched
        .inspect_ok(|(filename, num)| trace!(filename, num, "counted unmatched for submission"))
        .try_filter(|(_, unmatched)| future::ready(*unmatched >= 1))
        .inspect_ok(|(filename, num)| info!(filename, num, "found not totally matched submission"))
        .try_filter(|(_, unmatched)| future::ready(*unmatched >= MIN_UNMATCHED_TO_NOTIFY))
        .inspect_ok(|(filename, num)| warn!(filename, num, "Unmatched!"))
        .filter_map(|result| async move { result.err() })
        .take(100)
        .collect::<Vec<_>>()
        .await;

    println!("{errors:?}");

    Ok(())
}

fn count_unmatched(
    submissions: impl Stream<Item = Result<(String, SubmissionPdf)>>,
) -> impl Stream<Item = Result<(String, usize)>> {
    submissions.and_then(|(filename, pdf)| async move {
        let matching = pdf
            .question_matching()
            .context("cannot get question matching status")?;
        let num_unmatched = matching
            .filter(|(matching, _)| matches!(matching, MatchingState::Unmatched))
            .count();
        Ok((filename, num_unmatched))
    })
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
