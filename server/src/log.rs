use std::fmt::Write;
use std::sync::Arc;

use slack_morphism::prelude::{SlackApiChatPostMessageRequest, SlackHyperClient};
use slack_morphism::{SlackApiToken, SlackChannelId, SlackMessageContent};
use tokio::runtime::Handle;
use tracing::field::{Field, Visit};
use tracing::Subscriber;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::format;
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{fmt, registry, reload, EnvFilter, Layer, Registry};

pub fn init_tracing() -> reload::Handle<Option<SlackLayer>, Registry> {
    let (slack_layer, slack_layer_handle) = reload::Layer::new(None);

    registry()
        .with(slack_layer)
        .with(fmt::layer().event_format(format().pretty()))
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env()
                .unwrap(),
        )
        .init();

    slack_layer_handle
}

pub struct SlackLayer {
    client: Arc<SlackHyperClient>,
    token: SlackApiToken,
    channel: SlackChannelId,
}

impl SlackLayer {
    pub fn new(
        client: Arc<SlackHyperClient>,
        token: SlackApiToken,
        channel: SlackChannelId,
    ) -> Self {
        Self {
            client,
            token,
            channel,
        }
    }
}

impl<S: Subscriber + for<'b> LookupSpan<'b>> Layer<S> for SlackLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let metadata = event.metadata();
        let line_info = (metadata.file(), metadata.line());

        let mut message = match line_info {
            (Some(file), Some(line)) => format!("{} {file}:{line} {{", metadata.level()),
            (Some(file), None) => format!("{} {file} {{", metadata.level()),
            (None, _) => format!("{} {{", metadata.level()),
        };
        let mut visitor = SlackVisit::new();
        event.record(&mut visitor);
        message.push_str(" }");

        let (client, token) = (self.client.clone(), self.token.clone());
        let channel = self.channel.clone();
        let fut = async move {
            let session = client.open_session(&token);

            // Not much to do if error reporting fails
            let _ = session
                .chat_post_message(&SlackApiChatPostMessageRequest::new(
                    channel,
                    SlackMessageContent::new().with_text(message),
                ))
                .await;
        };

        if let Ok(handle) = Handle::try_current() {
            handle.spawn(fut);
        }
    }
}

struct SlackVisit {
    message: Option<String>,
    fields: String,
}

impl SlackVisit {
    pub fn new() -> Self {
        Self {
            message: None,
            fields: String::new(),
        }
    }
}

impl Visit for SlackVisit {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}"));
        } else {
            writeln!(&mut self.fields, "\t{} = {:?}", field.name(), value).unwrap();
        }
    }
}
