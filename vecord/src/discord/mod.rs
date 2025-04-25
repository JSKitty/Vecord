mod handler;

use crate::config::Config;
use anyhow::Result;
use serenity::all::{
    ChannelId, Client, CreateMessage, GatewayIntents, Http,
};
use std::sync::Arc;
use tokio::sync::mpsc;

pub use handler::Handler;

#[derive(Clone)]
pub struct DiscordBot {
    token: String,
    channel_id: ChannelId,
    http: Arc<Http>,
}

impl DiscordBot {
    pub fn new(config: &Config) -> Self {
        Self {
            token: config.discord_token.clone(),
            channel_id: ChannelId::new(config.discord_channel_id),
            http: Arc::new(Http::new(&config.discord_token)),
        }
    }

    pub async fn start(
        &self,
        message_sender: mpsc::Sender<String>,
    ) -> Result<()> {
        // Configure intents to receive message events
        let intents = GatewayIntents::GUILD_MESSAGES 
            | GatewayIntents::MESSAGE_CONTENT;

        // Create a new Client
        let mut client = Client::builder(&self.token, intents)
            .event_handler(Handler::new(
                self.channel_id,
                message_sender,
            ))
            .await?;

        // Start client, this is a blocking operation
        client.start().await?;

        Ok(())
    }

    pub async fn send_message(&self, content: &str) -> Result<()> {
        self.channel_id
            .send_message(&self.http, CreateMessage::new().content(content))
            .await?;
        Ok(())
    }
}
