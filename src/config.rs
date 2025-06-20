use anyhow::Result;
use dotenvy::dotenv;
use std::env;

pub struct Config {
    pub discord_token: String,
    pub discord_channel_id: u64,
    pub nostr_private_key: String,
    pub nostr_relays: Vec<String>,
    pub subscribers_file: Option<String>,
    pub metadata_cache_file: Option<String>,
}

impl Config {
    pub fn new() -> Result<Self> {
        // Load environment variables from .env file
        dotenv().ok();
        
        let discord_token = env::var("DISCORD_TOKEN")
            .expect("Expected DISCORD_TOKEN in the environment");
        
        let discord_channel_id = env::var("DISCORD_CHANNEL_ID")
            .expect("Expected DISCORD_CHANNEL_ID in the environment")
            .parse::<u64>()
            .expect("DISCORD_CHANNEL_ID must be a valid u64");
        
        let nostr_private_key = env::var("NOSTR_PRIVATE_KEY")
            .expect("Expected NOSTR_PRIVATE_KEY in the environment");
        
        // Parse comma-separated list of relays
        let nostr_relays_str = env::var("NOSTR_RELAYS")
            .expect("Expected NOSTR_RELAYS in the environment");
        
        let nostr_relays = nostr_relays_str
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();
        
        // Optional file to persist subscribers
        let subscribers_file = env::var("SUBSCRIBERS_FILE").ok();
        
        // Optional file to cache user metadata
        let metadata_cache_file = env::var("METADATA_CACHE_FILE").ok().or_else(|| {
            // Default to a file in the same directory as subscribers if it exists
            subscribers_file.as_ref().map(|s| {
                let path = std::path::Path::new(s);
                let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
                dir.join("metadata_cache.json").to_string_lossy().to_string()
            })
        });
        
        Ok(Self {
            discord_token,
            discord_channel_id,
            nostr_private_key,
            nostr_relays,
            subscribers_file,
            metadata_cache_file,
        })
    }
}
