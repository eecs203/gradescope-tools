use std::sync::Arc;

use anyhow::Result;
use gradescope_api::assignment::Assignment;
use slack_morphism::prelude::*;

#[tracing::instrument(skip(client, token), ret, err)]
async fn notify_unmatched_pages(
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
    Ok(())
}

pub struct ChooseAssignmentsTemplate<'a> {
    assignments: &'a [Assignment],
}

impl<'a> ChooseAssignmentsTemplate<'a> {
    pub fn new(assignments: &'a [Assignment]) -> Self {
        Self { assignments }
    }
}

impl<'a> SlackMessageTemplate for ChooseAssignmentsTemplate<'a> {
    fn render_template(&self) -> SlackMessageContent {
        SlackMessageContent::new().with_blocks(slack_blocks![some_into(
            SlackSectionBlock::new()
                .with_text(pt!("Check which assignment(s)?"))
                .with_accessory(
                    SlackBlockMultiStaticSelectElement::new("action_id".into())
                        .with_options(
                            self.assignments
                                .iter()
                                .map(|assignment| {
                                    SlackBlockChoiceItem::new(
                                        pt!(assignment.name().as_str()),
                                        serde_json::to_string(assignment).unwrap(),
                                    )
                                })
                                .collect()
                        )
                        .into()
                )
        )])
    }
}
