use crate::message::BridgeMessage;
use serenity::all::{
    ChannelId, Context, EventHandler, Message, MessageType, Ready,
};
use tokio::sync::mpsc;

pub struct Handler {
    channel_id: ChannelId,
    message_sender: mpsc::Sender<BridgeMessage>,
}

impl Handler {
    pub fn new(channel_id: ChannelId, message_sender: mpsc::Sender<BridgeMessage>) -> Self {
        Self {
            channel_id,
            message_sender,
        }
    }
}

#[serenity::async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _: Context, ready: Ready) {
        println!("Connected to Discord as {}", ready.user.name);
    }

    async fn message(&self, _ctx: Context, msg: Message) {
        // Only process messages from the specified channel
        if msg.channel_id != self.channel_id {
            return;
        }

        // Ignore bot messages to prevent loops
        if msg.author.bot {
            return;
        }

        // Ignore system messages
        if msg.kind != MessageType::Regular && msg.kind != MessageType::InlineReply {
            return;
        }

        // Create a BridgeMessage for Nostr
        let author_name = msg.author.name.clone();
        let content = msg.content.clone();
        let bridge_message = BridgeMessage::Discord {
            author: author_name,
            content,
        };

        // Send the message to be bridged to Nostr
        if let Err(e) = self.message_sender.send(bridge_message).await {
            eprintln!("Error sending message to Nostr: {}", e);
        }
    }
}
