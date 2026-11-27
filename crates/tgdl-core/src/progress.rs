use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub struct Progress {
    pub total: u64,
    downloaded: AtomicU64,
    per_source: Vec<AtomicU64>,
    start: Instant,
}

impl Progress {
    pub fn new(total: u64, sources: usize) -> Self {
        Self {
            total,
            downloaded: AtomicU64::new(0),
            per_source: (0..sources).map(|_| AtomicU64::new(0)).collect(),
            start: Instant::now(),
        }
    }

    pub fn add(&self, n: u64) {
        self.downloaded.fetch_add(n, Ordering::Relaxed);
    }

    pub fn add_for(&self, source: usize, n: u64) {
        if let Some(counter) = self.per_source.get(source) {
            counter.fetch_add(n, Ordering::Relaxed);
        }
        self.add(n);
    }

    pub fn downloaded(&self) -> u64 {
        self.downloaded.load(Ordering::Relaxed)
    }

    pub fn source_bytes(&self, source: usize) -> u64 {
        self.per_source
            .get(source)
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    pub fn elapsed(&self) -> std::time::Duration {
        self.start.elapsed()
    }

    pub fn speed_bps(&self) -> f64 {
        let elapsed = self.elapsed().as_secs_f64().max(0.001);
        self.downloaded() as f64 / elapsed
    }
}
