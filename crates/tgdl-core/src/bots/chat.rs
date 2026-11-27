use crate::config::Config;
use crate::error::DownloadError;
use grammers_client::Client;
use grammers_tl_types as tl;

#[derive(Debug, Clone, Copy)]
pub struct ChatInfo {
    pub id: i64,
    pub access_hash: i64,
}

pub async fn ensure_chat(client: &Client, cfg: &Config) -> Result<ChatInfo, DownloadError> {
    if let (Some(id), Some(access_hash)) = (cfg.chat.chat_id, cfg.chat.access_hash) {
        return Ok(ChatInfo { id, access_hash });
    }
    let title = if cfg.chat.title.trim().is_empty() {
        "TG Downloads".to_string()
    } else {
        cfg.chat.title.trim().to_string()
    };
    let updates = client
        .invoke(&tl::functions::channels::CreateChannel {
            broadcast: false,
            megagroup: true,
            for_import: false,
            forum: false,
            title,
            about: String::new(),
            geo_point: None,
            address: None,
            ttl_period: None,
        })
        .await
        .map_err(|e| DownloadError::Telegram(e.to_string()))?;
    extract_chat(&updates).ok_or_else(|| DownloadError::Telegram("chat id not returned".into()))
}

pub async fn add_bots(client: &Client, chat: ChatInfo, cfg: &Config) -> Result<(), DownloadError> {
    let mut users = Vec::new();
    for bot in &cfg.bots {
        let username = bot.username.trim().trim_start_matches('@');
        if username.is_empty() {
            continue;
        }
        users.push(resolve_user(client, username).await?);
    }
    if users.is_empty() {
        return Ok(());
    }
    if chat.access_hash != 0 {
        client
            .invoke(&tl::functions::channels::InviteToChannel {
                channel: tl::enums::InputChannel::Channel(tl::types::InputChannel {
                    channel_id: chat.id,
                    access_hash: chat.access_hash,
                }),
                users,
            })
            .await
            .map_err(|e| DownloadError::Telegram(e.to_string()))?;
    } else {
        for user in users {
            client
                .invoke(&tl::functions::messages::AddChatUser {
                    chat_id: chat.id,
                    user_id: user,
                    fwd_limit: 0,
                })
                .await
                .map_err(|e| DownloadError::Telegram(e.to_string()))?;
        }
    }
    Ok(())
}

async fn resolve_user(
    client: &Client,
    username: &str,
) -> Result<tl::enums::InputUser, DownloadError> {
    let resolved = client
        .invoke(&tl::functions::contacts::ResolveUsername {
            username: username.to_string(),
            referer: None,
        })
        .await
        .map_err(|e| DownloadError::Telegram(e.to_string()))?;
    let tl::enums::contacts::ResolvedPeer::Peer(peer) = resolved;
    for user in peer.users {
        if let tl::enums::User::User(user) = user
            && let Some(access_hash) = user.access_hash
        {
            return Ok(tl::enums::InputUser::User(tl::types::InputUser {
                user_id: user.id,
                access_hash,
            }));
        }
    }
    Err(DownloadError::Telegram(format!(
        "bot @{username} has no access hash"
    )))
}

fn extract_chat(updates: &tl::enums::Updates) -> Option<ChatInfo> {
    match updates {
        tl::enums::Updates::Updates(updates) => chat_from(&updates.chats),
        tl::enums::Updates::Combined(updates) => chat_from(&updates.chats),
        _ => None,
    }
}

fn chat_from(chats: &[tl::enums::Chat]) -> Option<ChatInfo> {
    for chat in chats {
        match chat {
            tl::enums::Chat::Channel(channel) => {
                return Some(ChatInfo {
                    id: channel.id,
                    access_hash: channel.access_hash.unwrap_or_default(),
                });
            }
            tl::enums::Chat::Chat(chat) => {
                return Some(ChatInfo {
                    id: chat.id,
                    access_hash: 0,
                });
            }
            _ => {}
        }
    }
    None
}
