use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NostrMessageMetadata {
    pub username: String,
    pub pubkey: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BridgeMessage {
    /// From Discord to Nostr
    Discord {
        author: String,
        content: String,
    },
    
    /// From Nostr to Discord
    Nostr {
        content: String,
        metadata: NostrMessageMetadata,
    },
}
