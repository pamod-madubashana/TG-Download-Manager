use crate::config::Config;
use crate::error::DownloadError;
use grammers_client::Client;
use grammers_client::sender::SenderPool;
use grammers_session::storages::SqliteSession;
use std::sync::Arc;

pub struct BotHandle {
    pub name: String,
    pub client: Client,
    pub _session: Arc<SqliteSession>,
    pub _runner: tokio::task::JoinHandle<()>,
}

pub struct BotPool {
    pub bots: Vec<BotHandle>,
}

impl BotPool {
    pub async fn from_config(cfg: &Config) -> Result<Self, DownloadError> {
        std::fs::create_dir_all("sessions").map_err(DownloadError::Io)?;
        let mut bots = Vec::new();
        for b in &cfg.bots {
            let token = std::env::var(&b.token_env)
                .map_err(|_| DownloadError::Config(format!("env {} not set", b.token_env)))?;
            let session_path = format!("sessions/{}.session", b.name);
            let session = Arc::new(
                SqliteSession::open(&session_path)
                    .await
                    .map_err(|e| DownloadError::Telegram(e.to_string()))?,
            );
            let SenderPool { runner, handle, .. } =
                SenderPool::new(Arc::clone(&session), cfg.telegram.api_id);
            let runner_handle = tokio::spawn(async move {
                let _ = runner.run().await;
            });
            let client = Client::new(handle);
            if !client
                .is_authorized()
                .await
                .map_err(|e| DownloadError::Telegram(e.to_string()))?
            {
                client
                    .bot_sign_in(&token, &cfg.telegram.api_hash)
                    .await
                    .map_err(|e| DownloadError::Telegram(e.to_string()))?;
            }
            bots.push(BotHandle {
                name: b.name.clone(),
                client,
                _session: session,
                _runner: runner_handle,
            });
        }
        Ok(Self { bots })
    }

    pub fn first(&self) -> Option<&BotHandle> {
        self.bots.first()
    }
}
