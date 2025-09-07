use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NostrMessageMetadata {
    pub username: String,
    pub pubkey: String,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageAttachment {
    pub bytes: Vec<u8>,
    pub extension: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BridgeMessage {
    /// From Discord to Nostr
    Discord {
        author: String,
        content: String,
        /// Optional first image attachment (bytes + file extension such as "png", "jpg")
        image: Option<ImageAttachment>,
    },
    
    /// From Nostr to Discord
    Nostr {
        content: String,
        metadata: NostrMessageMetadata,
    },
}
