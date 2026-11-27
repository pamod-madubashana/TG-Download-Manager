use crate::error::DownloadError;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct TelegramConfig {
    pub api_id: i32,
    pub api_hash: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DownloadConfig {
    pub chunk_size_kib: u64,
    pub slots_per_bot: usize,
    pub max_attempts_per_chunk: u32,
    pub output_dir: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct BotConfig {
    pub name: String,
    pub token_env: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub telegram: TelegramConfig,
    pub download: DownloadConfig,
    pub bots: Vec<BotConfig>,
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, DownloadError> {
        let text = std::fs::read_to_string(path).map_err(DownloadError::Io)?;
        toml::from_str(&text).map_err(|e| DownloadError::Config(e.to_string()))
    }

    pub fn chunk_size_bytes(&self) -> u64 {
        self.download.chunk_size_kib * 1024
    }
}
