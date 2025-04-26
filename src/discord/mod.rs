mod handler;

use crate::config::Config;
use crate::message::{BridgeMessage, NostrMessageMetadata};
use anyhow::Result;
use serenity::all::{
    ChannelId, Client, Colour, CreateEmbed, CreateEmbedAuthor, CreateMessage, GatewayIntents, Http
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
        message_sender: mpsc::Sender<BridgeMessage>,
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

    pub async fn send_message(&self, message: &BridgeMessage) -> Result<()> {
        match message {
            BridgeMessage::Nostr { content, metadata } => {
                // Create a message builder
                let msg = CreateMessage::new();
                
                // Create a rich embed
                let mut embed = CreateEmbed::new();
                embed = embed.description(content);
                // Create a footer text without using the closure
                embed = embed.footer(serenity::all::CreateEmbedFooter::new(metadata.pubkey.clone()));
                embed = embed.color(Colour::from_rgb(89, 252, 179));
                
                // Add thumbnail if avatar is available
                if let Some(avatar_url) = &metadata.avatar_url {
                    embed = embed.author(CreateEmbedAuthor::new(metadata.username.clone()).icon_url(avatar_url));
                }
                
                // Send with rich embed
                self.channel_id
                    .send_message(&self.http, msg.embed(embed))
                    .await?;
            },
            
            BridgeMessage::Discord { author, content } => {
                // This shouldn't happen, but handle it gracefully
                self.channel_id
                    .send_message(&self.http, CreateMessage::new()
                        .content(format!("[Discord] {}: {}", author, content)))
                    .await?;
            }
        }
        
        Ok(())
    }
}
