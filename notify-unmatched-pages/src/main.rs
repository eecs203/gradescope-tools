use anyhow::{Context, Result};
use app_utils::{init_from_env, init_tracing, InitFromEnv};
use gradescope_api::export_submissions::{read_zip, SubmissionPdfReader};
use lib203::homework::{find_homeworks, HwNumber};
use tokio::fs::{self, File};
use tokio_util::compat::TokioAsyncReadCompatExt;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    // full_path().await
    // load_zip().await
    load_pdf().await
}

async fn full_path() -> Result<()> {
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

    gradescope
        .export_submissions(&course, hw_1)
        .await
        .context("could not export submissions")?;

    Ok(())
}

async fn load_zip() -> Result<()> {
    let zip_data = File::open("out/submissions.zip").await?;
    read_zip(zip_data.compat()).await?;

    Ok(())
}

async fn load_pdf() -> Result<()> {
    let matched_data = fs::read("out/example_matched.pdf").await?;
    let unmatched_data = fs::read("out/example_unmatched.pdf").await?;
    let mixed_data = fs::read("out/example_mixed.pdf").await?;

    println!(
        "matched: {:#?}",
        SubmissionPdfReader::new(matched_data)?.question_matching()?
    );

    println!(
        "unmatched: {:#?}",
        SubmissionPdfReader::new(unmatched_data)?.question_matching()?
    );

    println!(
        "mixed: {:#?}",
        SubmissionPdfReader::new(mixed_data)?.question_matching()?
    );

    Ok(())
}
