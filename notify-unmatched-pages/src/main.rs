use std::pin::pin;

use anyhow::{Context, Result};
use app_utils::{init_from_env, init_tracing, InitFromEnv};
use futures::future::{try_join, try_join3};
use futures::{future, pin_mut, stream, StreamExt, TryStreamExt};
use gradescope_api::assignment::{Assignment, AssignmentId, AssignmentName};
use gradescope_api::assignment_selector::AssignmentSelector;
use gradescope_api::course::CourseClient;
use gradescope_api::submission::SubmissionsManagerProps;
use gradescope_api::submission_export::submissions_export_load;
use gradescope_api::types::Points;
use itertools::Itertools;
use notify_unmatched_pages::notify::find_unsubmitted;
use notify_unmatched_pages::report::{self, UnmatchedReport};
use tokio::fs;
use tracing::{debug, error, trace};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let InitFromEnv {
        course, gradescope, ..
    } = init_from_env().await?;
    debug!("initialized");

    let course_client = CourseClient::new(&gradescope, &course);

    let assignments = gradescope
        .get_assignments(&course)
        .await
        .context("could not get assignments from Gradescope")?;
    trace!(?assignments, "got assignments");

    let assignment_selector = AssignmentSelector::new("Assignment 0".to_owned());
    let path = "out/assignment0.zip";
    let assignment = assignment_selector.select_from(&assignments)?;

    let assignment_client = course_client.with_assignment(assignment);

    let (submissions_export, submission_to_student_map, outline) = try_join3(
        submissions_export_load(path),
        assignment_client.submission_to_student_map(),
        assignment_client.outline(),
    )
    .await?;
    let reports = find_unsubmitted(
        &assignment_client,
        submissions_export,
        submission_to_student_map,
        outline,
    );
    pin_mut!(reports);

    println!("Reports:");
    while let Some(report) = reports.next().await {
        match report {
            Ok(report) => {
                println!("{report}");
                println!("{}", report.page_matching_link());
                // println!("{}", report.csv_string());
            }
            Err(err) => {
                eprintln!("error!");
                eprintln!("{err:?}");
            }
        }
        println!("\n----------\n");
    }
    println!();

    Ok(())
}

async fn load_submission_metadata() -> Result<SubmissionsManagerProps> {
    let props_json = fs::read_to_string("out/submissions_manager_props.json").await?;
    let metadata = serde_json::from_str(&props_json)?;
    Ok(metadata)
}
