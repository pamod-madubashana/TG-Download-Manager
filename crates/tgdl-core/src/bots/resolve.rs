use crate::error::DownloadError;
use grammers_client::Client;
use grammers_client::media::Media;
use grammers_session::types::PeerRef;

pub async fn resolve_file(
    client: &Client,
    link: &str,
) -> Result<(PeerRef, i32, Media), DownloadError> {
    // Parse t.me link.
    let path = link
        .split("t.me/")
        .nth(1)
        .unwrap_or(link)
        .trim_end_matches('/');
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    // t.me/<username>/<msg>  or t.me/c/<id>/<msg>
    let (peer, msg_id) = if segments.len() >= 2 && segments[0] == "c" {
        // numeric channel: try session cache via resolve_peer with default auth (best effort)
        let chat_id: i64 = segments[1]
            .parse()
            .map_err(|_| DownloadError::Config("bad chat id".into()))?;
        let msg_id: i32 = segments[2]
            .parse()
            .map_err(|_| DownloadError::Config("bad msg id".into()))?;
        let peer_id = grammers_session::types::PeerId::channel(chat_id)
            .ok_or_else(|| DownloadError::Config("bad channel id".into()))?;
        let peer_ref = grammers_session::types::PeerRef {
            id: peer_id,
            auth: grammers_session::types::PeerAuth::default(),
        };
        (peer_ref, msg_id)
    } else if segments.len() >= 2 {
        let username = segments[0];
        let msg_id: i32 = segments[1]
            .parse()
            .map_err(|_| DownloadError::Config("bad msg id".into()))?;
        let peer = client
            .resolve_username(username)
            .await
            .map_err(|e| DownloadError::Telegram(e.to_string()))?
            .ok_or_else(|| DownloadError::Telegram("username not found".into()))?;
        let pr = peer
            .to_ref()
            .await
            .map_err(|e| DownloadError::Telegram(e.to_string()))?
            .ok_or_else(|| DownloadError::Telegram("no peer ref".into()))?;
        (pr, msg_id)
    } else {
        return Err(DownloadError::Config("invalid link".into()));
    };

    let msgs = client
        .get_messages_by_id(peer, &[msg_id])
        .await
        .map_err(|e| DownloadError::Telegram(e.to_string()))?;
    let msg = msgs
        .into_iter()
        .flatten()
        .next()
        .ok_or_else(|| DownloadError::Telegram("message not found".into()))?;
    let media = msg
        .media()
        .ok_or_else(|| DownloadError::Telegram("no media".into()))?;
    Ok((peer, msg_id, media))
}

pub fn file_name(media: &Media) -> String {
    use grammers_client::media::Media;
    match media {
        Media::Document(d) => {
            if let Some(grammers_tl_types::enums::Document::Document(doc)) = &d.raw.document {
                for attr in &doc.attributes {
                    if let grammers_tl_types::enums::DocumentAttribute::Filename(f) = attr {
                        return f.file_name.clone();
                    }
                }
            }
            "document.bin".to_string()
        }
        Media::Photo(_) => "photo.jpg".to_string(),
        Media::Sticker(_) => "sticker.webp".to_string(),
        _ => "download.bin".to_string(),
    }
}
