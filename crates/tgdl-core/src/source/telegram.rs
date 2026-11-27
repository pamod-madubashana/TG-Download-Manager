use crate::error::{DownloadError, FetchError};
use crate::model::ChunkRange;
use crate::source::ChunkSource;
use async_trait::async_trait;
use bytes::Bytes;
use grammers_client::Client;
use grammers_client::media::Downloadable;
use grammers_session::types::PeerRef;
use grammers_tl_types as tl;
use tokio::sync::RwLock;

pub struct TelegramSource {
    client: Client,
    peer: PeerRef,
    msg_id: i32,
    location: RwLock<Option<tl::enums::InputFileLocation>>,
    dc_id: RwLock<Option<i32>>,
    pub size: u64,
    pub file_name: String,
}

impl TelegramSource {
    pub async fn resolve(client: Client, link: &str) -> Result<Self, DownloadError> {
        let (peer, msg_id, media) = crate::bots::resolve::resolve_file(&client, link).await?;
        let size = media
            .size()
            .ok_or_else(|| DownloadError::Telegram("no size".into()))? as u64;
        let file_name = crate::bots::resolve::file_name(&media);
        let location = media.to_raw_input_location();
        let dc_id = media.dc_id();
        Ok(Self {
            client,
            peer,
            msg_id,
            location: RwLock::new(location),
            dc_id: RwLock::new(dc_id),
            size,
            file_name,
        })
    }

    pub fn meta_unique_id(&self) -> String {
        format!("{:?}:{}", self.peer.id, self.msg_id)
    }

    async fn refresh(&self) -> Result<(), FetchError> {
        let msgs = self
            .client
            .get_messages_by_id(self.peer, &[self.msg_id])
            .await
            .map_err(|e| FetchError::Transient(e.to_string()))?;
        let msg = msgs
            .into_iter()
            .flatten()
            .next()
            .ok_or_else(|| FetchError::Fatal("message gone".into()))?;
        let media = msg
            .media()
            .ok_or_else(|| FetchError::Fatal("no media".into()))?;
        let loc = media
            .to_raw_input_location()
            .ok_or_else(|| FetchError::Fatal("no location".into()))?;
        *self.location.write().await = Some(loc);
        *self.dc_id.write().await = media.dc_id();
        Ok(())
    }

    async fn ensure(&self) -> Result<(), FetchError> {
        if self.location.read().await.is_some() {
            return Ok(());
        }
        self.refresh().await
    }
}

#[async_trait]
impl ChunkSource for TelegramSource {
    async fn fetch(&self, range: ChunkRange) -> Result<Bytes, FetchError> {
        self.ensure().await?;
        let mut retried = false;
        loop {
            let location = self
                .location
                .read()
                .await
                .clone()
                .ok_or_else(|| FetchError::Fatal("no location".into()))?;
            let mut dc = self.dc_id.read().await.unwrap_or(2);
            let limit = range.length.min(512 * 1024) as i32;
            let request = tl::functions::upload::GetFile {
                precise: false,
                cdn_supported: false,
                location: location.clone(),
                offset: range.offset as i64,
                limit,
            };
            match self.client.invoke_in_dc(dc, &request).await {
                Ok(tl::enums::upload::File::File(f)) => {
                    return Ok(Bytes::from(f.bytes));
                }
                Ok(tl::enums::upload::File::CdnRedirect(_)) => {
                    return Err(FetchError::Fatal("cdn redirect unsupported".into()));
                }
                Err(e) => {
                    let rpc = match &e {
                        grammers_client::InvocationError::Rpc(r) => Some(r),
                        _ => None,
                    };
                    if let Some(r) = rpc {
                        let name = r.name.as_str();
                        if name.contains("FILE_REFERENCE") {
                            if retried {
                                return Err(FetchError::Fatal("file ref expired".into()));
                            }
                            retried = true;
                            self.refresh().await?;
                            continue;
                        }
                        if name.starts_with("FLOOD_WAIT") {
                            let secs = name
                                .strip_prefix("FLOOD_WAIT_")
                                .and_then(|s| s.parse::<u64>().ok())
                                .or_else(|| r.value.map(|v| v as u64))
                                .unwrap_or(1);
                            return Err(FetchError::FloodWait(std::time::Duration::from_secs(
                                secs,
                            )));
                        }
                        if r.code == 303 && let Some(v) = r.value {
                            dc = v as i32;
                            *self.dc_id.write().await = Some(dc);
                            continue;
                        }
                        return Err(FetchError::Transient(r.to_string()));
                    }
                    return Err(FetchError::Transient(e.to_string()));
                }
            }
        }
    }
}
