use super::{Shared, retry};
use crate::error::{DownloadError, FetchError};
use crate::model::DownloadPlan;
use crate::source::ChunkSource;
use bytes::Bytes;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

pub(super) async fn run(
    source_idx: usize,
    source: Arc<dyn ChunkSource>,
    planner: Arc<DownloadPlan>,
    shared: Arc<Shared>,
    tx: mpsc::Sender<(usize, usize, Bytes)>,
) {
    loop {
        if shared.abort.load(std::sync::atomic::Ordering::SeqCst)
            || shared.remaining.load(std::sync::atomic::Ordering::SeqCst) == 0
        {
            break;
        }
        wait_cooldown(&shared, source_idx).await;
        let idx = match shared.queue.pop() {
            Some(i) => i,
            None => {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                continue;
            }
        };
        let chunk = planner.chunks[idx];
        match source.fetch(chunk).await {
            Ok(bytes) => {
                if tx.send((source_idx, idx, bytes)).await.is_err() {
                    shared.set_error(DownloadError::Verify("writer closed".into()));
                    break;
                }
            }
            Err(FetchError::FloodWait(d)) => {
                *shared.cooldowns[source_idx].lock().unwrap() = Some(Instant::now() + d);
                if !shared.queue.retry(idx) {
                    shared.set_error(DownloadError::Verify("max attempts".into()));
                    break;
                }
            }
            Err(FetchError::FileRefExpired | FetchError::Transient(_)) => {
                let attempt = shared.queue.attempts(idx);
                if !shared.queue.retry(idx) {
                    shared.set_error(DownloadError::Verify("max attempts".into()));
                    break;
                }
                let delay = retry::backoff(attempt, shared.retry_base);
                tokio::time::sleep(delay).await;
            }
            Err(FetchError::Fatal(e)) => {
                shared.set_error(DownloadError::Source(FetchError::Fatal(e)));
                break;
            }
        }
    }
}

async fn wait_cooldown(shared: &Shared, source_idx: usize) {
    loop {
        let until = *shared.cooldowns[source_idx].lock().unwrap();
        match until {
            Some(t) => {
                let now = Instant::now();
                if now < t {
                    tokio::time::sleep(t - now).await;
                } else {
                    *shared.cooldowns[source_idx].lock().unwrap() = None;
                    break;
                }
            }
            None => break,
        }
    }
}
