use crate::bots::resolve::file_name;
use crate::error::DownloadError;
use grammers_client::sender::UpdatesConfiguration;
use grammers_client::update::Update;
use grammers_client::{Client, media::Media};
use grammers_session::updates::UpdatesLike;
use tokio::sync::mpsc::{Receiver, UnboundedSender};

#[derive(Debug, Clone)]
pub struct QueuedFile {
    pub link: String,
    pub name: String,
}

pub async fn listen(
    client: Client,
    updates: Receiver<UpdatesLike>,
    chat_id: i64,
    tx: UnboundedSender<QueuedFile>,
) -> Result<(), DownloadError> {
    let mut stream = client
        .stream_updates(updates, UpdatesConfiguration { catch_up: true })
        .await
        .map_err(|e| DownloadError::Telegram(e.to_string()))?;
    loop {
        let update = stream
            .next()
            .await
            .map_err(|e| DownloadError::Telegram(e.to_string()))?;
        if let Update::NewMessage(message) = update {
            if message.peer_id().bare_id_unchecked() != chat_id {
                continue;
            }
            let Some(media) = message.media() else {
                continue;
            };
            let name = match &media {
                Media::Document(_) | Media::Photo(_) => file_name(&media),
                _ => continue,
            };
            let link = format!("https://t.me/c/{chat_id}/{}", message.id());
            let _ = tx.send(QueuedFile { link, name });
        }
    }
}
