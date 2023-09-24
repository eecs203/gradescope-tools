use anyhow::{Context, Result};
use app_utils::{init_from_env, init_tracing, InitFromEnv};
use futures::future::{join_all, try_join_all};
use futures::{future, join, stream, Stream, StreamExt, TryStreamExt};
use gradescope_api::assignment::{Assignment, AssignmentId, AssignmentName};
use gradescope_api::client::{Auth, Client};
use gradescope_api::course::Course;
use gradescope_api::export_submissions::{
    files_as_submissions, read_zip, MatchingState, SubmissionPdf,
};
use gradescope_api::submission::{SubmissionId, SubmissionsManagerProps};
use gradescope_api::types::{Points, QuestionNumber};
use itertools::Itertools;
use lib203::homework::{find_homeworks, HwNumber};
use notify_unmatched_pages::report::UnmatchedReport;
use notify_unmatched_pages::unmatched_page_reports;
use tokio::fs::{self, File};
use tokio_util::compat::TokioAsyncReadCompatExt;
use tracing::{debug, error, info, trace, warn};

const MIN_UNMATCHED_TO_NOTIFY: usize = 1;

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
            AssignmentId::new("3338814".to_owned()),
            AssignmentName::new("Homework 3".to_owned()),
            Points::new(100.0).unwrap(),
        ),
        Assignment::new(
            AssignmentId::new("3338817".to_owned()),
            AssignmentName::new("Groupwork 3".to_owned()),
            Points::new(30.0).unwrap(),
        ),
    ];

    let results = try_join_all(
        targets
            .iter()
            .map(|assignment| unmatched_page_reports(&gradescope, &course, assignment)),
    )
    .await?;

    let results = stream::iter(results)
        .flatten_unordered(4)
        .collect::<Vec<_>>()
        .await;

    // let assignments = gradescope
    //     .get_assignments(&course)
    //     .await
    //     .context("could not get assignments")?;
    // trace!(?assignments, "got assignments");
    // let assignment = assignments
    //     .iter()
    //     .find(|assignment| assignment.name().as_str() == "Homework 1")
    //     .context("could not find assignment")?;
    // // let homeworks = find_homeworks(&assignments);
    // // trace!(?homeworks, "got homeworks");

    // // let assignment = homeworks
    // //     .get(&HwNumber::new("3"))
    // //     .context("could not find HW 3")?
    // //     .individual()
    // //     .context("could not find Individual HW 3")?;
    // debug!(?assignment, "got target assignment");

    // // let metadata = load_submission_metadata().await?;
    // let metadata = download_submission_metadata(&course, assignment, &gradescope).await?;

    // let submission_to_students = metadata.submission_to_student_map()?;

    // // let submissions = load_zip().await?;
    // let submissions = download_submissions(&course, assignment, &gradescope).await?;

    // let num_unmatched = unmatched_questions(submissions);

    // let unmatched_submissions = num_unmatched
    //     .inspect_ok(|(id, unmatched)| debug!(%id, ?unmatched, "unmatched questions for submission"))
    //     .try_filter(|(_, unmatched)| future::ready(!unmatched.is_empty()))
    //     .inspect_ok(|(id, unmatched)| info!(%id, num_unmatched = unmatched.len(), ?unmatched, "found not totally matched submission"))
    //     .try_filter(|(_, unmatched)| future::ready(unmatched.len() >= MIN_UNMATCHED_TO_NOTIFY))
    //     .inspect_ok(|(id, unmatched)| warn!(%id, num_unmatched = unmatched.len(), ?unmatched, "Unmatched submission!"));

    // let unmatched_students = unmatched_submissions
    //     .map_ok(|(id, unmatched)| {
    //         let students = submission_to_students.get(&id).with_context(|| {
    //             format!(
    //                 "could not find students for submission {id}, with {} mismatched problems: {unmatched:#?}",
    //                 unmatched.len(),
    //             )
    //         })?;

    //         Ok((id, students, unmatched))
    //     })
    //     .and_then(future::ready);

    // let results = unmatched_students
    //     .map_ok(UnmatchedReport::new)
    //     .inspect_err(|err| error!(%err, "error somewhere"))
    //     .collect::<Vec<_>>()
    //     .await;

    println!("Reports:");
    for report in results.iter().flatten() {
        println!("{report}");
        println!("\n----------\n");
    }
    println!();

    println!("Errors:");
    for result in results.iter() {
        if let Err(err) = result {
            println!("{err:#}");
        }
    }
    println!();

    println!("Meta summary:");

    let num_mismatched_assignments = results.iter().flatten().count();
    println!("Got {} mismatched assignments", num_mismatched_assignments);

    let student_names = results
        .iter()
        .flatten()
        .flat_map(|report| report.students())
        .map(|student| student.name())
        .sorted()
        .format(", ");
    println!("Mismatching students: {student_names}");

    Ok(())
}

fn unmatched_questions(
    submissions: impl Stream<Item = Result<(SubmissionId, SubmissionPdf)>>,
) -> impl Stream<Item = Result<(SubmissionId, Vec<QuestionNumber>)>> {
    submissions
        .map(|result| {
            tokio_rayon::spawn(move || {
                let (filename, pdf) = result?;
                let matching = pdf
                    .question_matching()
                    .context("cannot get question matching status")?;
                let unmatched = matching
                    .filter(|(matching, _)| matches!(matching, MatchingState::Unmatched))
                    .map(|(_, number)| number)
                    .collect();
                Ok((filename, unmatched))
            })
        })
        .buffer_unordered(16)
}

async fn download_submission_metadata(
    course: &Course,
    assignment: &Assignment,
    gradescope: &Client<Auth>,
) -> Result<SubmissionsManagerProps> {
    let metadata = gradescope
        .get_submissions_metadata(course, assignment)
        .await
        .context("could not get submissions")?;

    Ok(metadata)
}

async fn load_submission_metadata() -> Result<SubmissionsManagerProps> {
    let props_json = fs::read_to_string("out/submissions_manager_props.json").await?;
    let metadata = serde_json::from_str(&props_json)?;
    Ok(metadata)
}

async fn download_submissions(
    course: &Course,
    assignment: &Assignment,
    gradescope: &Client<Auth>,
) -> Result<impl Stream<Item = Result<(SubmissionId, SubmissionPdf)>>> {
    let submissions = gradescope
        .export_submissions(course, assignment)
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
