use anyhow::{Context, Result};
use app_utils::{init_from_env, init_tracing, InitFromEnv};
use futures::future::try_join;
use futures::{future, stream, StreamExt, TryStreamExt};
use gradescope_api::assignment::{Assignment, AssignmentId, AssignmentName};
use gradescope_api::assignment_selector::AssignmentSelector;
use gradescope_api::course::CourseClient;
use gradescope_api::submission::SubmissionsManagerProps;
use gradescope_api::submission_export::submissions_export_load;
use gradescope_api::types::Points;
use itertools::Itertools;
use notify_unmatched_pages::notify::find_unsubmitted;
use notify_unmatched_pages::report::UnmatchedReport;
use tokio::fs;
use tracing::{debug, error, trace};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let InitFromEnv {
        course, gradescope, ..
    } = init_from_env().await?;
    debug!("initialized");

    let assignment_selectors = ["Assignment 0"];
    let assignment_selectors =
        assignment_selectors.map(|selector| AssignmentSelector::new(selector.to_owned()));

    let assignments = gradescope
        .get_assignments(&course)
        .await
        .context("could not get assignments from Gradescope")?;
    trace!(?assignments, "got assignments");

    let targets = assignment_selectors
        .iter()
        .map(|selector| selector.select_from(&assignments));

    let course_client = CourseClient::new(&gradescope, &course);

    let assignment_clients = targets
        .iter()
        .map(|assignment| course_client.with_assignment(assignment))
        .collect_vec();

    let reports = stream::iter(&assignment_clients)
        .zip(stream::iter(["out/hw11.zip", "out/gw11.zip"]))
        .map(|(client, path)| async move {
            let (submissions_export, submission_to_student_map) = try_join(
                submissions_export_load(path),
                client.submission_to_student_map(),
            )
            .await?;
            anyhow::Ok(find_unsubmitted(
                client,
                submissions_export,
                submission_to_student_map,
            ))
        })
        .buffer_unordered(assignment_clients.len().min(2))
        .try_flatten()
        .collect::<Vec<_>>()
        .await;

    println!("Reports:");
    for report in reports.iter().flatten() {
        // println!("{report}");
        // println!("{}", report.page_matching_link());
        println!("{}", report.csv_string());
        // println!("\n----------\n");
    }
    println!();

    println!("Errors:");
    for result in reports.iter() {
        if let Err(err) = result {
            println!("{err:#}");
        }
    }
    println!();

    println!("Meta summary:");

    let num_mismatched_assignments = reports.iter().flatten().count();
    println!("Got {} mismatched assignments", num_mismatched_assignments);

    Ok(())
}

async fn load_submission_metadata() -> Result<SubmissionsManagerProps> {
    let props_json = fs::read_to_string("out/submissions_manager_props.json").await?;
    let metadata = serde_json::from_str(&props_json)?;
    Ok(metadata)
}
