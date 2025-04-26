use crate::config::Config;
use crate::message::{BridgeMessage, NostrMessageMetadata};
use crate::metadata::{MetadataCache, UserMetadata};
use anyhow::{Result, anyhow};
use nostr_sdk::{
    Client, ClientBuilder, Filter, FromBech32, Keys, Kind, Options, PublicKey, SecretKey, ToBech32,
    nips::nip59::UnwrappedGift
};
use std::time::Duration;
use std::str::FromStr;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::fs;
use std::io::{Read, Write};
use tokio::sync::mpsc;
use tracing::{error, info};

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
                    if let Ok(bech32) = pubkey.to_bech32() {
                        if let Err(e) = writeln!(file, "{}", bech32) {
                            error!("Failed to write subscriber to file: {}", e);
                        }
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
    client: Option<Client>,
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
            client: None,
        })
    }

    pub async fn start(
        &mut self,
        discord_sender: mpsc::Sender<BridgeMessage>,
    ) -> Result<mpsc::Sender<BridgeMessage>> {
        // Create a new client builder with our keys
        let client = ClientBuilder::new().signer(self.keys.clone()).opts(Options::new().gossip(false)).build();
        
        // Add relays
        for relay in &self.relays {
            client.add_relay(relay).await?;
        }
        
        // Connect to relays
        client.connect().await;
        
        // Wait for connections to establish
        tokio::time::sleep(Duration::from_secs(1)).await;
        
        // Create a channel for sending messages to Nostr
        let (nostr_sender, mut nostr_receiver) = mpsc::channel::<BridgeMessage>(100);
        
        // Clone client for the sender task
        let client_clone = client.clone();
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
                        if let Err(e) = client_clone.send_private_msg(pubkey, &nostr_message, []).await {
                            error!("Error sending private message to Nostr user {}: {}", pubkey, e);
                        } else {
                            info!("Sent Discord message to Nostr user: {}", pubkey);
                        }
                    }
                }
            }
        });

        // Get our pubkey for filtering own messages
        let my_pubkey = self.keys.public_key();

        // Subscribe to Incoming Giftwraps (kind 1059)
        let _ = client
            .subscribe(Filter::new().pubkey(my_pubkey).kind(Kind::GiftWrap).limit(0), None)
            .await;
        
        // Store the client
        self.client = Some(client.clone());
        
        // Clone for the notification handler
        let subscribers_clone = self.subscribers.clone();
        let metadata_cache_clone = self.metadata_cache.clone();
        let client_clone = client.clone();
        
        // Spawn a task to handle incoming Nostr private messages
        tokio::spawn(async move {
            let mut notifications = client.notifications();
            
            while let Ok(notification) = notifications.recv().await {
                match notification {
                    nostr_sdk::RelayPoolNotification::Event{ event, relay_url: _, subscription_id: _ } => {

                        // Skip our own events to prevent loops
                        if event.pubkey == my_pubkey {
                            continue;
                        }

                        // Try to decrypt the message
                        if let Ok(UnwrappedGift { rumor, sender }) = client.unwrap_gift_wrap(&event).await {
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
                                    let _ = client_clone.send_private_msg(
                                        sender_pubkey, 
                                        "You are now subscribed to the Discord channel. You will receive all messages from the Discord channel. Send !unsubscribe to stop receiving messages.", 
                                        []
                                    ).await;
                                } else {
                                    // Already subscribed
                                    let _ = client_clone.send_private_msg(
                                        sender_pubkey, 
                                        "You are already subscribed to the Discord channel.", 
                                        []
                                    ).await;
                                }
                                continue;
                            } else if message_content == "!unsubscribe" {
                                if subscribers_clone.remove(&sender_pubkey) {
                                    info!("Unsubscribed: {}", sender_pubkey);
                                    // Send confirmation
                                    let _ = client_clone.send_private_msg(
                                        sender_pubkey, 
                                        "You have been unsubscribed from the Discord channel. You will no longer receive messages.", 
                                        []
                                    ).await;
                                } else {
                                    // Not subscribed
                                    let _ = client_clone.send_private_msg(
                                        sender_pubkey, 
                                        "You are not currently subscribed to the Discord channel.", 
                                        []
                                    ).await;
                                }
                                continue;
                            } else if message_content == "!help" {
                                // Send help information
                                let _ = client_clone.send_private_msg(
                                    sender_pubkey, 
                                    "Available commands:\n!subscribe - Start receiving Discord messages\n!unsubscribe - Stop receiving Discord messages\n!help - Show this help message", 
                                    []
                                ).await;
                                continue;
                            }
                            
                            // Only relay messages from subscribed users
                            if subscribers_clone.contains(&sender_pubkey) {
                                // Try to fetch user metadata
                                let metadata = match metadata_cache_clone.fetch_metadata(&client, &sender_pubkey).await {
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
                                let _ = client_clone.send_private_msg(
                                    sender_pubkey, 
                                    "Your message was not forwarded to Discord because you're not subscribed. Send !subscribe to start forwarding your messages.", 
                                    []
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
