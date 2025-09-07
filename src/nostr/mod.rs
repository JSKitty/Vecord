use crate::config::Config;
use crate::message::{BridgeMessage, NostrMessageMetadata};
use crate::metadata::{MetadataCache, UserMetadata};
use anyhow::{Result, anyhow};
use nostr_sdk::{
    FromBech32, Keys, Kind, PublicKey, SecretKey, ToBech32,
    nips::nip59::UnwrappedGift, RelayPoolNotification,
};
use std::time::Duration;
use std::str::FromStr;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::fs;
use std::io::{Read, Write};
use tokio::sync::mpsc;
use tracing::{error, info};

// Vector SDK
use vector_sdk::VectorBot;

/// Helper function to parse a pubkey from either bech32 or hex format
fn parse_pubkey(key_str: &str) -> Result<PublicKey> {
    if key_str.starts_with("npub") {
        PublicKey::from_bech32(key_str).map_err(|e| anyhow!("Invalid bech32 pubkey: {}", e))
    } else {
        PublicKey::from_hex(key_str).map_err(|e| anyhow!("Invalid hex pubkey: {}", e))
    }
}

/// Manages the list of subscribers
#[derive(Clone)]
struct SubscriberList {
    subscribers: Arc<Mutex<HashSet<PublicKey>>>,
    file_path: Option<String>,
}

impl SubscriberList {
    fn new(file_path: Option<String>) -> Result<Self> {
        let mut subscribers = HashSet::new();

        // Try to load subscribers from the file if it exists
        if let Some(path) = &file_path {
            if let Ok(mut file) = fs::File::open(path) {
                let mut contents = String::new();
                if file.read_to_string(&mut contents).is_ok() {
                    for line in contents.lines() {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            if let Ok(pubkey) = parse_pubkey(trimmed) {
                                subscribers.insert(pubkey);
                                info!("Loaded subscriber: {}", trimmed);
                            } else {
                                error!("Failed to parse pubkey: {}", trimmed);
                            }
                        }
                    }
                }
            }
        }

        Ok(Self {
            subscribers: Arc::new(Mutex::new(subscribers)),
            file_path,
        })
    }

    fn add(&self, pubkey: PublicKey) -> bool {
        let added;
        {
            let mut lock = self.subscribers.lock().unwrap();
            added = lock.insert(pubkey);
        }

        // Save to file if a path is specified
        if added {
            self.save_to_file();
        }

        added
    }

    fn remove(&self, pubkey: &PublicKey) -> bool {
        let removed;
        {
            let mut lock = self.subscribers.lock().unwrap();
            removed = lock.remove(pubkey);
        }

        // Save to file if a path is specified
        if removed {
            self.save_to_file();
        }

        removed
    }

    fn contains(&self, pubkey: &PublicKey) -> bool {
        let lock = self.subscribers.lock().unwrap();
        lock.contains(pubkey)
    }

    fn get_all(&self) -> Vec<PublicKey> {
        let lock = self.subscribers.lock().unwrap();
        lock.iter().cloned().collect()
    }

    fn save_to_file(&self) {
        if let Some(path) = &self.file_path {
            let lock = self.subscribers.lock().unwrap();
            if let Ok(mut file) = fs::File::create(path) {
                for pubkey in lock.iter() {
                    let bech32 = pubkey.to_bech32().unwrap_or_else(|_| pubkey.to_string());
                    if let Err(e) = writeln!(file, "{}", bech32) {
                        error!("Failed to write subscriber to file: {}", e);
                    }
                }
            } else {
                error!("Failed to open subscribers file for writing: {}", path);
            }
        }
    }
}

pub struct NostrClient {
    keys: Keys,
    relays: Vec<String>,
    subscribers: SubscriberList,
    metadata_cache: MetadataCache,
    bot: Option<VectorBot>,
}

impl NostrClient {
    pub fn new(config: &Config) -> Result<Self> {
        // Create keys from secret key
        let secret_key = SecretKey::from_str(&config.nostr_private_key)?;
        let keys = Keys::new(secret_key);

        // Initialize subscriber list with optional file path
        let subscribers = SubscriberList::new(config.subscribers_file.clone())?;

        // Initialize metadata cache
        let metadata_cache = MetadataCache::new(config.metadata_cache_file.clone())?;

        Ok(Self {
            keys,
            relays: config.nostr_relays.clone(),
            subscribers,
            metadata_cache,
            bot: None,
        })
    }

    pub async fn start(
        &mut self,
        discord_sender: mpsc::Sender<BridgeMessage>,
    ) -> Result<mpsc::Sender<BridgeMessage>> {
        // Build VectorBot with default metadata (SDK sets up client, metadata and giftwrap subscription)
        let bot = VectorBot::new(
            self.keys.clone(),
            "Vecord".to_string(),
            "Vecord".to_string(),
            "The Vecord Bridge - Bringing the anonymity of Vector to the Discord realm.".to_string(),
            "https://jskitty.cat/vector/img/vecord.png",
            "https://jskitty.cat/vector/img/vecord.png",
            "".to_string(),
            "".to_string(),
        ).await;

        // Optionally add user-configured relays on top of SDK defaults
        for relay in &self.relays {
            if let Err(e) = bot.client.add_relay(relay).await {
                error!("Failed to add relay {}: {:?}", relay, e);
            }
        }

        // Ensure connections are established (SDK already connects, but reconnect to include any added relays)
        bot.client.connect().await;

        // Wait briefly for connections to establish
        tokio::time::sleep(Duration::from_secs(1)).await;

        // Create a channel for sending messages to Nostr
        let (nostr_sender, mut nostr_receiver) = mpsc::channel::<BridgeMessage>(100);

        // Clone bot for the sender task
        let bot_clone = bot.clone();
        let subscribers_clone = self.subscribers.clone();

        // Spawn a task to handle sending messages from Discord to Nostr
        tokio::spawn(async move {
            while let Some(message) = nostr_receiver.recv().await {
                if let BridgeMessage::Discord { author, content } = message {
                    // Format the message for Nostr
                    let nostr_message = format!("[Discord] {}: {}", author, content);

                    // Get current list of subscribers
                    let subscribers = subscribers_clone.get_all();

                    for pubkey in subscribers {
                        // Use Vector SDK Channel API
                        let chat = bot_clone.get_chat(pubkey).await;
                        let ok = chat.send_private_message(&nostr_message).await;
                        if !ok {
                            error!("Error sending private message to Nostr user {}", pubkey);
                        } else {
                            info!("Sent Discord message to Nostr user: {}", pubkey);
                        }
                    }
                }
            }
        });

        // Get our pubkey for filtering own messages
        let my_pubkey = self.keys.public_key();

        // Store the bot
        self.bot = Some(bot.clone());

        // Clone for the notification handler
        let subscribers_clone = self.subscribers.clone();
        let metadata_cache_clone = self.metadata_cache.clone();
        let bot_clone = bot.clone();

        // Spawn a task to handle incoming Nostr private messages
        tokio::spawn(async move {
            let mut notifications = bot.client.notifications();

            while let Ok(notification) = notifications.recv().await {
                match notification {
                    RelayPoolNotification::Event { event, relay_url: _, subscription_id: _ } => {
                        // Skip our own events to prevent loops
                        if event.pubkey == my_pubkey {
                            continue;
                        }

                        // Try to decrypt the message via SDK-configured client
                        if let Ok(UnwrappedGift { rumor, sender }) = bot.client.unwrap_gift_wrap(&event).await {
                            // Only process encrypted direct messages
                            if rumor.kind != Kind::PrivateDirectMessage {
                                continue;
                            };

                            // Create some simplified utility variables
                            let sender_pubkey = sender;
                            let message_content = rumor.content.trim();

                            // Handle subscription commands
                            if message_content == "!subscribe" {
                                if subscribers_clone.add(sender_pubkey) {
                                    info!("New subscriber: {}", sender_pubkey);
                                    // Send confirmation
                                    let chat = bot_clone.get_chat(sender_pubkey).await;
                                    let _ = chat.send_private_message(
                                        "You are now subscribed to the Discord channel. You will receive all messages from the Discord channel. Send !unsubscribe to stop receiving messages."
                                    ).await;
                                } else {
                                    // Already subscribed
                                    let chat = bot_clone.get_chat(sender_pubkey).await;
                                    let _ = chat.send_private_message(
                                        "You are already subscribed to the Discord channel."
                                    ).await;
                                }
                                continue;
                            } else if message_content == "!unsubscribe" {
                                if subscribers_clone.remove(&sender_pubkey) {
                                    info!("Unsubscribed: {}", sender_pubkey);
                                    // Send confirmation
                                    let chat = bot_clone.get_chat(sender_pubkey).await;
                                    let _ = chat.send_private_message(
                                        "You have been unsubscribed from the Discord channel. You will no longer receive messages."
                                    ).await;
                                } else {
                                    // Not subscribed
                                    let chat = bot_clone.get_chat(sender_pubkey).await;
                                    let _ = chat.send_private_message(
                                        "You are not currently subscribed to the Discord channel."
                                    ).await;
                                }
                                continue;
                            } else if message_content == "!help" {
                                // Send help information
                                let chat = bot_clone.get_chat(sender_pubkey).await;
                                let _ = chat.send_private_message(
                                    "Available commands:\n!subscribe - Start receiving Discord messages\n!unsubscribe - Stop receiving Discord messages\n!help - Show this help message"
                                ).await;
                                continue;
                            }

                            // Only relay messages from subscribed users
                            if subscribers_clone.contains(&sender_pubkey) {
                                // Try to fetch user metadata (via SDK client)
                                let metadata = match metadata_cache_clone.fetch_metadata(&bot.client, &sender_pubkey).await {
                                    Ok(metadata) => metadata,
                                    Err(e) => {
                                        error!("Failed to fetch metadata for {}: {}", sender_pubkey, e);
                                        // Create a default metadata entry if fetch fails
                                        UserMetadata::new(&sender_pubkey)
                                    }
                                };

                                // Get the best username for display
                                let username = metadata.get_best_name();

                                // Create metadata for the message
                                let pubkey_str = sender_pubkey.to_bech32().unwrap_or_else(|_| sender_pubkey.to_string());
                                let message_metadata = NostrMessageMetadata {
                                    username: username.clone(),
                                    pubkey: pubkey_str,
                                    avatar_url: metadata.picture,
                                };

                                // Create the bridge message
                                let bridge_message = BridgeMessage::Nostr {
                                    content: message_content.to_string(),
                                    metadata: message_metadata,
                                };

                                // Send the decrypted message to Discord
                                if let Err(e) = discord_sender.send(bridge_message).await {
                                    error!("Error forwarding message to Discord: {}", e);
                                } else {
                                    info!("Forwarded Nostr DM to Discord from: {}", username);
                                }
                            } else {
                                // Inform the user they need to subscribe first
                                let chat = bot_clone.get_chat(sender_pubkey).await;
                                let _ = chat.send_private_message(
                                    "Your message was not forwarded to Discord because you're not subscribed. Send !subscribe to start forwarding your messages."
                                ).await;
                                info!("Ignored message from non-subscribed user: {}", sender_pubkey);
                            }
                        } else {
                            error!("Failed to decrypt direct message from: {}", event.pubkey);
                        }
                    },
                    _ => {}, // Ignore other notifications
                }
            }
        });

        // Return the sender channel for sending messages to Nostr
        Ok(nostr_sender)
    }
}
