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
        let mut cfg: Self =
            toml::from_str(&text).map_err(|e| DownloadError::Config(e.to_string()))?;
        if cfg.telegram.api_id <= 0 {
            cfg.telegram.api_id = std::env::var("TGDL_API_ID")
                .ok()
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| DownloadError::Config("api_id missing".into()))?;
        }
        if cfg.telegram.api_hash == "..." || cfg.telegram.api_hash.is_empty() {
            cfg.telegram.api_hash = std::env::var("TGDL_API_HASH")
                .map_err(|_| DownloadError::Config("api_hash missing".into()))?;
        }
        Ok(cfg)
    }

    pub fn chunk_size_bytes(&self) -> u64 {
        self.download.chunk_size_kib * 1024
    }
}
