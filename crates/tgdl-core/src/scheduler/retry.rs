use std::time::Duration;

pub fn backoff(attempt: u32, base: Duration) -> Duration {
    let shift = attempt.min(10);
    base.saturating_mul(1u32 << shift)
        .min(Duration::from_secs(30))
}
