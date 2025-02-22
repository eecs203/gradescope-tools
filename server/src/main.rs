use std::env;
use std::sync::Arc;

use anyhow::{Context, Result};
use app_utils::{init_from_env, InitFromEnv};
use dotenvy::dotenv;
use futures::future::try_join;
use futures::{future, StreamExt, TryStreamExt};
use gradescope_api::assignment_selector::AssignmentSelector;
use gradescope_api::course::CourseClient;
use gradescope_api::submission_export::pdf::SubmissionPdfStream;
use gradescope_api::submission_export::SubmissionExport;
use log::{init_tracing, SlackLayer};
use notify_unmatched_pages::identify::identify_unmatched;
use notify_unmatched_pages::report::UnmatchedReport;
use slack_morphism::prelude::*;
use tracing::{error, info};

use crate::notify_unmatched::ChooseAssignmentsTemplate;

mod log;
mod notify_unmatched;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().unwrap();
    let slack_layer_handle = init_tracing();

    let client = Arc::new(SlackClient::new(SlackClientHyperConnector::new()));

    let token_value: SlackApiTokenValue = env::var("SLACK_TOKEN").unwrap().into();
    let token: SlackApiToken = SlackApiToken::new(token_value);

    let log_channel = env::var("SLACK_LOG_CHANNEL").unwrap().into();

    let slack_layer = SlackLayer::new(client.clone(), token, log_channel);
    slack_layer_handle.reload(slack_layer).unwrap();

    let socket_mode_callbacks =
        SlackSocketModeListenerCallbacks::new().with_command_events(on_command_event);

    let listener_environment = Arc::new(SlackClientEventsListenerEnvironment::new(client.clone()));

    let socket_mode_listener = SlackClientSocketModeListener::new(
        &SlackClientSocketModeConfig::new(),
        listener_environment.clone(),
        socket_mode_callbacks,
    );

    let app_token_value: SlackApiTokenValue = env::var("SLACK_APP_TOKEN").unwrap().into();
    let app_token: SlackApiToken = SlackApiToken::new(app_token_value);
    socket_mode_listener.listen_for(&app_token).await?;
    socket_mode_listener.serve().await;

    Ok(())
}

#[tracing::instrument(skip(client, _states), ret, err)]
async fn on_command_event(
    event: SlackCommandEvent,
    client: Arc<SlackHyperClient>,
    _states: SlackClientEventsUserState,
) -> Result<SlackCommandEventResponse, Box<dyn std::error::Error + Send + Sync>> {
    let token_value: SlackApiTokenValue = env::var("SLACK_TOKEN").unwrap().into();
    let token: SlackApiToken = SlackApiToken::new(token_value);
    tokio::spawn(notify_unmatched_pages(
        AssignmentSelector::new(event.text.unwrap_or_default()),
        client,
        token,
        event.channel_id,
    ));

    Ok(SlackCommandEventResponse::new(
        SlackMessageContent::new().with_text("it worked".into()),
    ))
}

#[tracing::instrument(skip(client, token), ret, err)]
async fn notify_unmatched_pages(
    assignment_selector: AssignmentSelector,
    client: Arc<SlackHyperClient>,
    token: SlackApiToken,
    channel: SlackChannelId,
) -> Result<()> {
    let InitFromEnv {
        gradescope, course, ..
    } = init_from_env().await?;

    let assignments = gradescope.get_assignments(&course).await?;
    info!(?assignments, "got assignments");

    let session = client.open_session(&token);
    let message = ChooseAssignmentsTemplate::new(&assignments);
    let request = SlackApiChatPostMessageRequest::new(channel.clone(), message.render_template());
    session.chat_post_message(&request).await?;

    let assignment = assignment_selector
        .select_from(&assignments)
        .context("could not get assignment")?;
    info!(?assignment, "got target assignment");

    let course_client = CourseClient::new(&gradescope, &course);

    let selectors: Vec<AssignmentSelector> = todo!();
    let assignments = course_client.get_assignments().await?;

    let reports = identify_unmatched(&selectors, &assignments, &course_client).await;

    let assignment_client = course_client.with_assignment(assignment);

    let (submission_export, submission_to_student_map) = try_join(
        assignment_client.export_submissions(),
        assignment_client.submission_to_student_map(),
    )
    .await?;

    let nonmatching_submitters = submission_export
        .submissions()
        .unmatched()
        .submitters(submission_to_student_map);

    let reports = nonmatching_submitters.map_ok(|nonmatching_submitter| {
        UnmatchedReport::new(&course, assignment, nonmatching_submitter)
    });

    let slack_errors = reports.then(|result| async {
        match result {
            Ok(report) => {
                session
                    .chat_post_message(&SlackApiChatPostMessageRequest::new(
                        channel.clone(),
                        SlackMessageContent::new().with_text(report.to_string()),
                    ))
                    .await
            }
            Err(err) => {
                session
                    .chat_post_message(&SlackApiChatPostMessageRequest::new(
                        channel.clone(),
                        SlackMessageContent::new().with_text(err.to_string()),
                    ))
                    .await
            }
        }
    });

    slack_errors
        .filter_map(|result| future::ready(result.err()))
        .for_each(|err| {
            error!(?err);
            future::ready(())
        })
        .await;

    Ok(())
}
