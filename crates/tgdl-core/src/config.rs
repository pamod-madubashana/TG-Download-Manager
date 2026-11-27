use crate::error::DownloadError;
use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{AeadCore, AeadInPlace, KeyInit, OsRng},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const CONFIG_FILE: &str = "config.tgdm";
const MAGIC: &[u8; 4] = b"TGDC";
const VERSION: u8 = 1;
const HEADER_LEN: usize = 24;
const MAX_PLAINTEXT: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub api_id: i32,
    pub api_hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownloadConfig {
    pub chunk_size_kib: u64,
    pub slots_per_bot: usize,
    pub max_attempts_per_chunk: u32,
    pub output_dir: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BotConfig {
    pub name: String,
    pub username: String,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatConfig {
    pub title: String,
    pub chat_id: Option<i64>,
    pub access_hash: Option<i64>,
    pub listen: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub telegram: TelegramConfig,
    pub download: DownloadConfig,
    pub bots: Vec<BotConfig>,
    pub chat: ChatConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            telegram: TelegramConfig {
                api_id: 0,
                api_hash: String::new(),
            },
            download: DownloadConfig {
                chunk_size_kib: 512,
                slots_per_bot: 3,
                max_attempts_per_chunk: 6,
                output_dir: "./downloads".into(),
            },
            bots: (1..=3)
                .map(|i| BotConfig {
                    name: format!("bot{i}"),
                    username: String::new(),
                    token: String::new(),
                })
                .collect(),
            chat: ChatConfig {
                title: "TG Downloads".into(),
                chat_id: None,
                access_hash: None,
                listen: true,
            },
        }
    }
}

impl Config {
    pub fn path() -> Result<PathBuf, DownloadError> {
        let exe = std::env::current_exe()?;
        let dir = exe
            .parent()
            .ok_or_else(|| DownloadError::Config("binary directory missing".into()))?;
        Ok(dir.join(CONFIG_FILE))
    }

    pub fn load() -> Result<Self, DownloadError> {
        Self::load_from(Self::path()?)
    }

    pub fn load_from(path: impl AsRef<Path>) -> Result<Self, DownloadError> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = std::fs::read(path)?;
        let key = device_key()?;
        let plain = decrypt_bytes(&bytes, &key)?;
        toml::from_slice(&plain).map_err(|e| DownloadError::Config(e.to_string()))
    }

    pub fn save(&self) -> Result<(), DownloadError> {
        self.save_to(Self::path()?)
    }

    pub fn save_to(&self, path: impl AsRef<Path>) -> Result<(), DownloadError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let plain = toml::to_string(self)
            .map_err(|e| DownloadError::Config(e.to_string()))?
            .into_bytes();
        let key = device_key()?;
        let encrypted = encrypt_bytes(&plain, &key)?;
        let temp = path.with_extension("tgdm.tmp");
        std::fs::write(&temp, encrypted)?;
        if std::fs::rename(&temp, path).is_err() {
            if path.exists() {
                std::fs::remove_file(path)?;
            }
            std::fs::rename(&temp, path)?;
        }
        Ok(())
    }

    pub fn chunk_size_bytes(&self) -> u64 {
        self.download.chunk_size_kib * 1024
    }
}

fn device_key() -> Result<Key<Aes256Gcm>, DownloadError> {
    let id = device_id()?;
    Ok(derive_key(&id))
}

fn derive_key(device_id: &str) -> Key<Aes256Gcm> {
    let mut hasher = Sha256::new();
    hasher.update(b"tgdm-aes256gcm-key-v1\0");
    hasher.update((device_id.len() as u64).to_le_bytes());
    hasher.update(device_id.as_bytes());
    let digest = hasher.finalize();
    *Key::<Aes256Gcm>::from_slice(&digest)
}

fn encrypt_bytes(plain: &[u8], key: &Key<Aes256Gcm>) -> Result<Vec<u8>, DownloadError> {
    if plain.len() > MAX_PLAINTEXT {
        return Err(DownloadError::Config("config too large".into()));
    }
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let mut output = Vec::with_capacity(HEADER_LEN + plain.len() + 16);
    output.extend_from_slice(MAGIC);
    output.push(VERSION);
    output.extend_from_slice(&[0, 0, 0]);
    output.extend_from_slice(&nonce);
    output.extend_from_slice(&(plain.len() as u32).to_le_bytes());
    let header = output.clone();
    let mut body = plain.to_vec();
    body.reserve(16);
    Aes256Gcm::new(key)
        .encrypt_in_place(&nonce, &header, &mut body)
        .map_err(|_| DownloadError::Config("config encryption failed".into()))?;
    output.extend_from_slice(&body);
    Ok(output)
}

fn decrypt_bytes(bytes: &[u8], key: &Key<Aes256Gcm>) -> Result<Vec<u8>, DownloadError> {
    if bytes.len() < HEADER_LEN + 16 {
        return Err(DownloadError::Config("config file truncated".into()));
    }
    if &bytes[0..4] != MAGIC || bytes[4] != VERSION {
        return Err(DownloadError::Config("unsupported config file".into()));
    }
    let plain_len = u32::from_le_bytes(
        bytes[20..24]
            .try_into()
            .map_err(|_| DownloadError::Config("bad config header".into()))?,
    ) as usize;
    if plain_len > MAX_PLAINTEXT || bytes.len() != HEADER_LEN + plain_len + 16 {
        return Err(DownloadError::Config("bad config length".into()));
    }
    let nonce = Nonce::from_slice(&bytes[8..20]);
    let mut body = bytes[HEADER_LEN..].to_vec();
    Aes256Gcm::new(key)
        .decrypt_in_place(nonce, &bytes[..HEADER_LEN], &mut body)
        .map_err(|_| {
            DownloadError::Config("config belongs to another device or is corrupt".into())
        })?;
    Ok(body)
}

fn normalize_id(value: &str) -> Result<String, DownloadError> {
    let id = value.trim().to_ascii_lowercase();
    if id.is_empty() {
        Err(DownloadError::Config("device id is empty".into()))
    } else {
        Ok(id)
    }
}

#[cfg(target_os = "windows")]
fn device_id() -> Result<String, DownloadError> {
    use winreg::{
        RegKey,
        enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY},
    };
    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(
            r"SOFTWARE\Microsoft\Cryptography",
            KEY_READ | KEY_WOW64_64KEY,
        )
        .map_err(|e| DownloadError::Config(format!("device id unavailable: {e}")))?;
    let guid: String = key
        .get_value("MachineGuid")
        .map_err(|e| DownloadError::Config(format!("device id unavailable: {e}")))?;
    normalize_id(&guid)
}

#[cfg(target_os = "linux")]
fn device_id() -> Result<String, DownloadError> {
    for path in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
        if let Ok(value) = std::fs::read_to_string(path) {
            if let Ok(id) = normalize_id(&value) {
                return Ok(id);
            }
        }
    }
    Err(DownloadError::Config("device id unavailable".into()))
}

#[cfg(target_os = "macos")]
fn device_id() -> Result<String, DownloadError> {
    let output = std::process::Command::new("ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
        .map_err(|e| DownloadError::Config(format!("device id unavailable: {e}")))?;
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        if line.contains("IOPlatformUUID") {
            let parts: Vec<&str> = line.split('"').collect();
            if parts.len() > 3 {
                return normalize_id(parts[3]);
            }
        }
    }
    Err(DownloadError::Config("device id unavailable".into()))
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn device_id() -> Result<String, DownloadError> {
    Err(DownloadError::Config(
        "device id unsupported on this platform".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.tgdm");
        let mut config = Config::default();
        config.telegram.api_id = 123;
        config.telegram.api_hash = "secret".into();
        config.bots[0].token = "bot-token".into();
        config.chat.chat_id = Some(-100123);
        config.save_to(&path).unwrap();
        assert_ne!(
            std::fs::read(&path).unwrap(),
            toml::to_string(&config).unwrap().into_bytes()
        );
        assert_eq!(Config::load_from(&path).unwrap(), config);
    }

    #[test]
    fn config_is_bound_to_device() {
        let bytes = encrypt_bytes(b"secret", &derive_key("device-a")).unwrap();
        assert!(decrypt_bytes(&bytes, &derive_key("device-a")).is_ok());
        assert!(decrypt_bytes(&bytes, &derive_key("device-b")).is_err());
    }

    #[test]
    fn missing_config_uses_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::load_from(dir.path().join("config.tgdm")).unwrap();
        assert_eq!(config, Config::default());
    }
}
