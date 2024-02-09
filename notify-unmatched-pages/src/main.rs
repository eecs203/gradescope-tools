use std::fs::File;

use anyhow::Result;
use app_utils::{init_from_env, init_tracing, InitFromEnv};
use futures::{pin_mut, StreamExt};
use gradescope_api::assignment_selector::AssignmentSelector;
use gradescope_api::course::CourseClient;
use notify_unmatched_pages::identify::identify_unmatched;
use tracing::debug;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let InitFromEnv {
        course, gradescope, ..
    } = init_from_env().await?;
    debug!("initialized");

    let course_client = CourseClient::new(&gradescope, &course);

    let assignments = course_client.get_assignments().await?;

    let selectors = [
        AssignmentSelector::new("Homework 3".to_owned()),
        AssignmentSelector::new("Groupwork 3".to_owned()),
        AssignmentSelector::new("Grading of Groupwork 2".to_owned()),
    ];
    let reports = identify_unmatched(&selectors, &assignments, &course_client).await;
    pin_mut!(reports);

    let mut file = File::create("out/hw-3.csv")?;
    println!("Reports:");
    while let Some(report) = reports.next().await {
        match report {
            Ok(report) => {
                use std::io::Write;
                // println!("{report}");
                // println!("{}", report.page_matching_link());
                println!("{}", report.csv_string());
                writeln!(&mut file, "{}", report.csv_string())?;
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
