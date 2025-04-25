# Vecord

A private bridge bot between Discord and Vector (Nostr client) that enables channel-based message relaying between the two platforms.

## Features

- Listen for encrypted DMs on Vector and forward them to a Discord channel
- Users can subscribe/unsubscribe by sending `!subscribe` or `!unsubscribe` commands to the bot
- Forward messages from a Discord channel to Vector as private messages to subscribed users
- Secure end-to-end encryption using Vector's NIP-44 Giftwrapped protocol
- Optional persistence of subscriber list between restarts

## Disclaimer

Vecord, by nature, exposes all messages bridged via Vecord to Discord servers (i.e: Relays your messages), as such, there is effectively zero encryption and thus little privacy benefit when using a publicly-known Vector account.

This disadvantage can be softened simply through the use of an anonymous Vector account.

**Only use Vecord with the full understanding that Vector's encryption is significantly less useful when the end result is displayed on a Discord Channel!**

*Note: all non-relayed messages, such as Bot Commands, are fully encrypted and never touch Discord's servers, this applies only to the messages relayed as per your explicit opt-in consent in using Vecord*

## Prerequisites

- Rust and Cargo installed
- A Discord bot token
- A Discord server with a channel for the bridge
- A Vector (Nostr) account and private key

## Setup

1. Clone this repository
2. Copy the `.env.example` file to `.env` and fill in your configuration:

```bash
cp .env.example .env
```

3. Edit the `.env` file with your:
   - Discord bot token
   - Discord channel ID
   - Vector private key (hex format or nsec format)
   - List of Vector-compatible Nostr relays to connect to
   - Optional file path to store subscribers (to persist subscribers between restarts)

4. Build the project:

```bash
cargo build --release
```

## Running the Bridge Bot

```bash
cargo run --release
```

## Discord Bot Setup

1. Create a new Discord application at the [Discord Developer Portal](https://discord.com/developers/applications)
2. Add a bot to your application
3. Enable the "Message Content Intent" in the Bot settings
4. Generate a bot token and add it to your `.env` file
5. Invite the bot to your server using the OAuth2 URL Generator with the following permissions:
   - Read Messages/View Channels
   - Send Messages

## Vector Setup

1. Create a Vector account or generate a Nostr key pair if you don't have one
2. Add the private key to your `.env` file
3. Share the public key with users who want to communicate with your bridge
4. Ensure your Vector-compatible relays are configured in the `.env` file

## Subscription Commands

Users can interact with the bot using the following commands in private messages:

- `!subscribe` - Start receiving messages from the Discord channel
- `!unsubscribe` - Stop receiving messages from the Discord channel
- `!help` - Show the list of available commands

## Troubleshooting

- Ensure your Discord bot has the correct permissions in the channel
- Verify your Vector-compatible relays are online and accessible
- Check the logs for connection issues with Discord or Vector connections
- Make sure your Vector account has access to the relays specified in the configuration

## License

MIT

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
