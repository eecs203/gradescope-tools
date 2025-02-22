use anyhow::Result;
use app_utils::{InitFromEnv, init_from_env, init_tracing};
use clap::{Arg, ArgAction, command};
use futures::{StreamExt, pin_mut};
use gradescope_api::assignment_selector::AssignmentSelector;
use gradescope_api::course::CourseClient;
use itertools::Itertools;
use notify_unmatched_pages::identify::identify_unmatched;
use notify_unmatched_pages::report::UnmatchedReportRecord;
use tracing::debug;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let InitFromEnv {
        course, gradescope, ..
    } = init_from_env().await?;
    debug!("initialized");

    let matches = command!()
        .arg(
            Arg::new("out")
                .long("out")
                .required(true)
                .value_name("FILE"),
        )
        .arg(Arg::new("assignment").action(ArgAction::Append))
        .get_matches();

    let out_path = matches.get_one::<String>("out").unwrap();

    let selectors = matches
        .get_many::<String>("assignment")
        .unwrap_or_default()
        .cloned()
        .map(AssignmentSelector::new)
        .collect_vec();

    let course_client = CourseClient::new(&gradescope, &course);

    let assignments = course_client.get_assignments().await?;

    let reports = identify_unmatched(&selectors, &assignments, &course_client).await;
    pin_mut!(reports);

    let mut writer = csv::Writer::from_path(out_path)?;
    println!("Reports:");
    while let Some(report) = reports.next().await {
        match report {
            Ok(report) => {
                let record = UnmatchedReportRecord::new(report);
                writer.serialize(record)?;
                // use std::io::Write;
                // println!("{report}");
                // println!("{}", report.page_matching_link());
                // println!("{}", report.csv_string());
                // writeln!(&mut file, "{}", report.csv_string())?;
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
