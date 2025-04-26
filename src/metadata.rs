use anyhow::{Result, anyhow};
use nostr_sdk::{Client, PublicKey, Metadata, Event, ToBech32, Kind, Filter, Timestamp};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tracing::{error, info, warn};
use serde::{Deserialize, Serialize};

// How long to cache metadata before refreshing (1 day)
const CACHE_LIFETIME: Duration = Duration::from_secs(60 * 60 * 24);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMetadata {
    pub pubkey: String,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub picture: Option<String>,
    pub nip05: Option<String>,
    pub about: Option<String>,
    pub last_updated: u64,
}

impl UserMetadata {
    pub fn new(pubkey: &PublicKey) -> Self {
        Self {
            pubkey: pubkey.to_bech32().unwrap_or_else(|_| pubkey.to_string()),
            name: None,
            display_name: None,
            picture: None,
            nip05: None,
            about: None,
            last_updated: 0,
        }
    }

    pub fn from_metadata(pubkey: &PublicKey, metadata: Metadata) -> Self {
        Self {
            pubkey: pubkey.to_bech32().unwrap_or_else(|_| pubkey.to_string()),
            name: metadata.name,
            display_name: metadata.display_name,
            picture: metadata.picture,
            nip05: metadata.nip05,
            about: metadata.about,
            last_updated: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    pub fn from_event(pubkey: &PublicKey, event: &Event) -> Result<Self> {
        let metadata = serde_json::from_str::<Metadata>(&event.content)
            .map_err(|e| anyhow!("Failed to parse metadata: {}", e))?;
        
        Ok(Self::from_metadata(pubkey, metadata))
    }

    pub fn get_best_name(&self) -> String {
        if let Some(display_name) = &self.display_name {
            if !display_name.trim().is_empty() {
                return display_name.clone();
            }
        }
        
        if let Some(name) = &self.name {
            if !name.trim().is_empty() {
                return name.clone();
            }
        }

        if let Some(nip05) = &self.nip05 {
            if !nip05.trim().is_empty() {
                return nip05.clone();
            }
        }

        // If no name is available, use the pubkey (shortened)
        if self.pubkey.starts_with("npub") && self.pubkey.len() > 12 {
            format!("{}...", &self.pubkey[0..12])
        } else {
            self.pubkey.clone()
        }
    }

    pub fn needs_refresh(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        // Check if metadata is older than the cache lifetime
        now > self.last_updated + CACHE_LIFETIME.as_secs()
    }

    pub fn should_fetch(&self) -> bool {
        // If we have no metadata or it needs a refresh
        self.name.is_none() && self.display_name.is_none() || self.needs_refresh()
    }
}

#[derive(Clone)]
pub struct MetadataCache {
    cache: Arc<Mutex<HashMap<String, UserMetadata>>>,
    file_path: Option<String>,
}

impl MetadataCache {
    pub fn new(file_path: Option<String>) -> Result<Self> {
        let mut cache = HashMap::new();
        
        // Try to load cache from file if it exists
        if let Some(path) = &file_path {
            if Path::new(path).exists() {
                if let Ok(file_content) = fs::read_to_string(path) {
                    match serde_json::from_str::<HashMap<String, UserMetadata>>(&file_content) {
                        Ok(loaded_cache) => {
                            info!("Loaded metadata cache with {} entries", loaded_cache.len());
                            cache = loaded_cache;
                        }
                        Err(e) => {
                            warn!("Failed to parse metadata cache file: {}", e);
                        }
                    }
                }
            }
        }
        
        Ok(Self {
            cache: Arc::new(Mutex::new(cache)),
            file_path,
        })
    }

    pub fn get(&self, pubkey: &PublicKey) -> Option<UserMetadata> {
        let key = pubkey.to_bech32().unwrap_or_else(|_| pubkey.to_string());
        let cache = self.cache.lock().unwrap();
        cache.get(&key).cloned()
    }

    pub fn put(&self, metadata: UserMetadata) {
        let key = metadata.pubkey.clone();
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(key, metadata);
        }
        // Save to file after releasing the lock
        self.save_to_file();
    }

    fn save_to_file(&self) {
        if let Some(path) = &self.file_path {
            // Create a snapshot of the cache to avoid holding the lock during file I/O
            let json_result = {
                let cache = self.cache.lock().unwrap();
                serde_json::to_string(&*cache)
            };
            
            // Handle file writing outside the lock
            match json_result {
                Ok(json) => {
                    if let Err(e) = fs::write(path, json) {
                        error!("Failed to write metadata cache to file: {}", e);
                    }
                }
                Err(e) => {
                    error!("Failed to serialize metadata cache: {}", e);
                }
            }
        }
    }

    pub async fn fetch_metadata(&self, client: &Client, pubkey: &PublicKey) -> Result<UserMetadata> {
        // Check if we already have recent metadata
        if let Some(metadata) = self.get(pubkey) {
            if !metadata.needs_refresh() {
                return Ok(metadata);
            }
        }
        
        // Fetch metadata from the network
        info!("Fetching metadata for {}", pubkey);
        
        // Request metadata
        let metadata_result = client.fetch_metadata(*pubkey, std::time::Duration::from_secs(15)).await?;
        
        if let Some(metadata) = metadata_result {
            // Create and store user metadata
            let user_metadata = UserMetadata::from_metadata(pubkey, metadata);
            self.put(user_metadata.clone());
            Ok(user_metadata)
        } else {
            // If no metadata is available, create a default entry
            let metadata = UserMetadata::new(pubkey);
            self.put(metadata.clone());
            Ok(metadata)
        }
    }
}
