use anyhow::{Context, Result};
use app_utils::{init_from_env, init_tracing, InitFromEnv};
use futures::future::try_join;
use futures::{future, stream, StreamExt, TryStreamExt};
use gradescope_api::assignment::{Assignment, AssignmentId, AssignmentName};
use gradescope_api::course::CourseClient;
use gradescope_api::submission::SubmissionsManagerProps;
use gradescope_api::submission_export::submissions_export_load;
use gradescope_api::types::Points;
use notify_unmatched_pages::notify::find_unsubmitted;
use notify_unmatched_pages::report::UnmatchedReport;
use tokio::fs;
use tracing::{debug, error};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let InitFromEnv {
        course, gradescope, ..
    } = init_from_env().await?;
    debug!("initialized");

    let assignments = gradescope
        .get_assignments(&course)
        .await
        .context("could not get assignments")?;
    dbg!(&assignments);
    // trace!(?assignments, "got assignments");
    // let (hw, gw) = assignments
    //     .iter()
    //     .inspect(|x| println!("before: {x:?}"))
    //     .filter(|assignment| {
    //         assignment.name().as_str() == "Homework 1"
    //             || assignment.name().as_str() == "Groupwork 1"
    //     })
    //     .inspect(|x| println!("{x:?}"))
    //     .collect_tuple()
    //     .context("could not find assignment")?;
    let targets = [
        Assignment::new(
            AssignmentId::new("3739823".to_owned()),
            AssignmentName::new("Homework 11".to_owned()),
            Points::new(100.0).unwrap(),
        ),
        Assignment::new(
            AssignmentId::new("3739838".to_owned()),
            AssignmentName::new("Groupwork 11".to_owned()),
            Points::new(30.0).unwrap(),
        ),
    ];

    let course_client = CourseClient::new(&gradescope, &course);

    let assignment_clients = targets
        .iter()
        .map(|assignment| course_client.with_assignment(assignment));

    // let exports: Vec<_> = stream::iter(assignment_clients)
    //     .map(|client| async move {
    //         anyhow::Ok((
    //             client.assignment(),
    //             try_join(
    //                 client.download_submission_export(),
    //                 client.submission_to_student_map(),
    //             )
    //             .await?,
    //         ))
    //     })
    //     .buffer_unordered(8);

    let exports = stream::iter(assignment_clients.zip(vec!["out/hw11.zip", "out/gw11.zip"]))
        .map(|(client, path)| async move {
            anyhow::Ok((
                client.assignment(),
                try_join(
                    submissions_export_load(path),
                    client.submission_to_student_map(),
                )
                .await?,
            ))
        })
        .buffer_unordered(8);

    let reports = exports
        .map_ok(
            |(assignment, (submission_export, submission_to_student_map))| {
                find_unsubmitted(
                    &course,
                    assignment,
                    submission_export,
                    submission_to_student_map,
                )
            },
        )
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
