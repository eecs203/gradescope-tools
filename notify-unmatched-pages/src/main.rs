use anyhow::Result;
use app_utils::{init_from_env, init_tracing, InitFromEnv};
use futures::future::try_join;
use futures::{future, stream, Stream, StreamExt, TryStreamExt};
use gradescope_api::assignment::{Assignment, AssignmentId, AssignmentName};
use gradescope_api::course::CourseClient;
use gradescope_api::export_submissions::{files_as_submissions, read_zip};
use gradescope_api::submission::{SubmissionId, SubmissionsManagerProps};
use gradescope_api::submission_export::pdf::SubmissionPdf;
use gradescope_api::types::Points;
use notify_unmatched_pages::report::UnmatchedReport;
use tokio::fs::{self, File};
use tokio_util::compat::TokioAsyncReadCompatExt;
use tracing::{debug, error};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let InitFromEnv {
        course,
        gradescope,
        course_name: _,
    } = init_from_env().await?;
    debug!("initialized");

    // let assignments = gradescope
    //     .get_assignments(&course)
    //     .await
    //     .context("could not get assignments")?;
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
            AssignmentId::new("3520797".to_owned()),
            AssignmentName::new("Homework 6".to_owned()),
            Points::new(100.0).unwrap(),
        ),
        Assignment::new(
            AssignmentId::new("3520799".to_owned()),
            AssignmentName::new("Groupwork 6".to_owned()),
            Points::new(30.0).unwrap(),
        ),
    ];

    let course_client = CourseClient::new(&gradescope, &course);

    let assignment_clients = targets
        .iter()
        .map(|assignment| course_client.with_assignment(assignment));

    let reports = stream::iter(assignment_clients)
        .then(|client| async move {
            let (submission_export, submission_to_student_map) = try_join(
                client.download_submission_export(),
                client.submission_to_student_map(),
            )
            .await?;

            let assignment = client.assignment();

            let reports = submission_export
                .submissions()
                .unmatched()
                .submitters(submission_to_student_map)
                .map_ok(move |submitter| (submitter, assignment));

            anyhow::Ok(reports)
        })
        .try_flatten()
        .map_ok(|(submitter, assignment)| UnmatchedReport::new(&course, assignment, submitter))
        .inspect_err(|err| error!(%err, "error getting nonmatching submitters"))
        .map(future::ready)
        .buffer_unordered(16)
        .collect::<Vec<_>>()
        .await;

    println!("Reports:");
    for report in reports.iter().flatten() {
        println!("{report}");
        println!("{}", report.page_matching_link());
        println!("\n----------\n");
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

async fn load_zip() -> Result<impl Stream<Item = Result<(SubmissionId, SubmissionPdf)>>> {
    let zip = File::open("out/submissions.zip").await?.compat();
    let files = read_zip(zip);
    let submissions = files_as_submissions(files);
    Ok(submissions)
}
