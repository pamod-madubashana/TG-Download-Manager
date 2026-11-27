use crate::error::FetchError;
use crate::model::ChunkRange;
use crate::source::ChunkSource;
use async_trait::async_trait;
use bytes::Bytes;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone)]
pub struct MockSource {
    data: Bytes,
    fail_first: Arc<AtomicUsize>,
    flood_first: bool,
    flooded: Arc<std::sync::atomic::AtomicBool>,
    fetches: Arc<AtomicUsize>,
}

impl MockSource {
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            data: Bytes::from(data),
            fail_first: Arc::new(AtomicUsize::new(0)),
            flood_first: false,
            flooded: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            fetches: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn failing(data: Vec<u8>, failures: usize) -> Self {
        let s = Self::new(data);
        s.fail_first.store(failures, Ordering::SeqCst);
        s
    }

    pub fn flooding(data: Vec<u8>) -> Self {
        let mut s = Self::new(data);
        s.flood_first = true;
        s
    }

    pub fn fetch_count(&self) -> usize {
        self.fetches.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ChunkSource for MockSource {
    async fn fetch(&self, range: ChunkRange) -> Result<Bytes, FetchError> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        if self.flood_first && !self.flooded.swap(true, Ordering::SeqCst) {
            return Err(FetchError::FloodWait(std::time::Duration::from_millis(1)));
        }
        let remaining = self.fail_first.load(Ordering::SeqCst);
        if remaining > 0 {
            self.fail_first.fetch_sub(1, Ordering::SeqCst);
            return Err(FetchError::Transient("mock failure".into()));
        }
        let start = range.offset as usize;
        let end = start + range.length as usize;
        if end > self.data.len() {
            return Err(FetchError::Fatal("out of range".into()));
        }
        Ok(self.data.slice(start..end))
    }
}
