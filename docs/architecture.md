# Architecture

Core crate has no CLI/Telegram coupling beyond optional `telegram` feature. Scheduler and storage are Telegram-agnostic via `ChunkSource` trait.

Settings are stored in `config.tgdm` next to the running binary. The file is AES-256-GCM encrypted with a key derived from the device's stable machine ID, so it cannot be moved to another device and read. The GUI Settings panel is the only supported way to edit API credentials, bot tokens/usernames, download options, and the queue chat.

The `telegram` feature creates a supergroup, adds every configured bot, and listens for new non-text messages in that chat. Text-only messages are ignored. Each file message is converted into a Telegram message link and enqueued as a download.
