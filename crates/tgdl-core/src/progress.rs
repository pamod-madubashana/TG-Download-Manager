use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub struct Progress {
    pub total: u64,
    downloaded: AtomicU64,
    start: Instant,
}

impl Progress {
    pub fn new(total: u64) -> Self {
        Self {
            total,
            downloaded: AtomicU64::new(0),
            start: Instant::now(),
        }
    }

    pub fn add(&self, n: u64) {
        self.downloaded.fetch_add(n, Ordering::Relaxed);
    }

    pub fn downloaded(&self) -> u64 {
        self.downloaded.load(Ordering::Relaxed)
    }

    pub fn speed_bps(&self) -> f64 {
        let elapsed = self.start.elapsed().as_secs_f64().max(0.001);
        self.downloaded() as f64 / elapsed
    }
}
