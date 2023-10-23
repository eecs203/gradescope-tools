use std::env;
use std::sync::Arc;

use anyhow::{Context, Result};
use app_utils::{init_from_env, InitFromEnv};
use dotenvy::dotenv;
use futures::future::{join, try_join};
use futures::{future, StreamExt, TryStreamExt};
use gradescope_api::assignment::AssignmentClient;
use gradescope_api::assignment_selector::AssignmentSelector;
use gradescope_api::client::Client;
use gradescope_api::course::CourseClient;
use log::{init_tracing, SlackLayer};
use notify_unmatched_pages::report::UnmatchedReport;
use slack_morphism::prelude::*;
use tokio::time::{interval_at, Instant};
use tracing::{debug, error, info, trace};

mod log;
mod page_match_task;
mod task;

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

    // Need to specify App token for Socket Mode:
    let app_token_value: SlackApiTokenValue = env::var("SLACK_APP_TOKEN").unwrap().into();
    let app_token: SlackApiToken = SlackApiToken::new(app_token_value);

    // Register an app token to listen for events,
    socket_mode_listener.listen_for(&app_token).await?;

    // Start WS connections calling Slack API to get WS url for the token,
    // and wait for Ctrl-C to shutdown
    // There are also `.start()`/`.shutdown()` available to manage manually
    socket_mode_listener.serve().await;

    // // Create our Slack API token
    // let token_value: SlackApiTokenValue = env::var("SLACK_TOKEN").unwrap().into();
    // let token: SlackApiToken = SlackApiToken::new(token_value);

    // // Create a Slack session with this token
    // // A session is just a lightweight wrapper around your token
    // // not to specify it all the time for series of calls.
    // let session = client.open_session(&token);

    // // Make your first API call (which is `api.test` here)
    // let test: SlackApiTestResponse = session
    //     .api_test(&SlackApiTestRequest::new().with_foo("Test".into()))
    //     .await?;

    // // Send a simple text message
    // let post_chat_req = SlackApiChatPostMessageRequest::new(
    //     "#gradescope-scraper-messages".into(),
    //     SlackMessageContent::new().with_text("Hey there!".into()),
    // );

    // let post_chat_resp = session.chat_post_message(&post_chat_req).await?;

    Ok(())
}

async fn on_command_event(
    event: SlackCommandEvent,
    client: Arc<SlackHyperClient>,
    _states: SlackClientEventsUserState,
) -> Result<SlackCommandEventResponse, Box<dyn std::error::Error + Send + Sync>> {
    println!("{:#?}", event);

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

    let assignments = gradescope
        .get_assignments(&course)
        .await
        .context("could not get assignments")?;
    trace!(?assignments, "got assignments");

    let assignment = assignment_selector
        .select_from(&assignments)
        .context("could not get assignment")?;
    debug!(?assignment, "got target assignment");

    let course_client = CourseClient::new(&gradescope, &course);

    let assignment_client = course_client.with_assignment(assignment);

    let (submission_export, submission_to_student_map) = try_join(
        assignment_client.download_submission_export(),
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

    let session = client.open_session(&token);

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
