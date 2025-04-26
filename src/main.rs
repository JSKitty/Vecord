mod config;
mod discord;
mod message;
mod metadata;
mod nostr;

use message::BridgeMessage;

use anyhow::Result;
use config::Config;
use discord::DiscordBot;
use nostr::NostrClient;
use tokio::sync::mpsc;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    info!("Starting Vecord - Vector <-> Discord bridge");

    // Load configuration
    let config = Config::new()?;
    info!("Configuration loaded");

    // Create bi-directional channels for message passing
    let (discord_to_nostr_tx, mut discord_to_nostr_rx) = mpsc::channel::<BridgeMessage>(100);
    let (nostr_to_discord_tx, mut nostr_to_discord_rx) = mpsc::channel::<BridgeMessage>(100);

    // Initialize Discord bot
    let discord_bot = DiscordBot::new(&config);
    
    // Clone discord_bot for the receiver task
    let discord_bot_clone = discord_bot.clone();

    // Initialize Nostr client
    let mut nostr_client = NostrClient::new(&config)?;
    
    // Start Nostr client and get sender channel
    let nostr_sender = nostr_client.start(nostr_to_discord_tx).await?;
    info!("Nostr client initialized");

    // Spawn a task to forward messages from Discord to Nostr
    tokio::spawn(async move {
        while let Some(message) = discord_to_nostr_rx.recv().await {
            if let Err(e) = nostr_sender.send(message).await {
                error!("Error forwarding message to Nostr: {}", e);
            }
        }
    });

    // Spawn a task to forward messages from Nostr to Discord
    tokio::spawn(async move {
        while let Some(message) = nostr_to_discord_rx.recv().await {
            if let Err(e) = discord_bot_clone.send_message(&message).await {
                error!("Error forwarding message to Discord: {}", e);
            }
        }
    });

    // Start Discord bot (this is a blocking call)
    info!("Starting Discord bot");
    discord_bot.start(discord_to_nostr_tx).await?;

    Ok(())
}
