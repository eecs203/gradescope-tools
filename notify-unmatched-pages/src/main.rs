use anyhow::{Context, Result};
use app_utils::{init_from_env, init_tracing, InitFromEnv};
use gradescope_api::export_submissions::read_zip;
use lib203::homework::{find_homeworks, HwNumber};
use tokio::fs::File;
use tokio_util::compat::TokioAsyncReadCompatExt;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    full_path().await
    // load_zip().await
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
