# Architecture

Core crate has no CLI/Telegram coupling beyond optional `telegram` feature. Scheduler and storage are Telegram-agnostic via `ChunkSource` trait.
