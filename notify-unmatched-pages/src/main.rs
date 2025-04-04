use anyhow::Result;
use app_utils::{InitFromEnv, init_from_env, init_tracing};
use clap::{Arg, ArgAction, command};
use futures::{StreamExt, pin_mut};
use gradescope_api::assignment_selector::AssignmentSelector;
use gradescope_api::course::CourseClient;
use itertools::Itertools;
use notify_unmatched_pages::identify::report_unmatched_many_assignments;
use notify_unmatched_pages::report::UnmatchedReportRecord;
use tracing::{debug, error, info};

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

    let all_assignments = course_client.get_assignments().await?;

    let assignments: Vec<_> = selectors
        .iter()
        .map(|selector| selector.select_from(&all_assignments))
        .try_collect()?;

    let reports = report_unmatched_many_assignments(&assignments, &course_client).await;
    pin_mut!(reports);

    let mut writer = csv::Writer::from_path(out_path)?;
    info!("Generating reports");
    while let Some(report) = reports.next().await {
        match report {
            Ok(report) => {
                let record = UnmatchedReportRecord::new(report);
                writer.serialize(record)?;
            }
            Err(err) => {
                error!(%err, "Error while identifying unmatched submissions");
            }
        }
    }

    Ok(())
}
