use std::sync::Arc;
use tempfile::tempdir;
use tgdl_core::model::{DownloadPlan, FileMeta};
use tgdl_core::scheduler::{Scheduler, SchedulerOptions};
use tgdl_core::source::mock::MockSource;

#[tokio::test]
async fn handles_flood_wait() {
    let dir = tempdir().unwrap();
    let bytes: Vec<u8> = (0..2048).map(|i| (i % 97) as u8).collect();
    let src = MockSource::flooding(bytes.clone());
    let plan = DownloadPlan::new(FileMeta {
        unique_id: "flood".into(),
        size: bytes.len() as u64,
        chunk_size: 1024,
    });
    let part = dir.path().join("out.part");
    let opts = SchedulerOptions {
        slots_per_bot: 2,
        retry_base: std::time::Duration::from_millis(1),
        ..Default::default()
    };
    Scheduler::run(vec![Arc::new(src)], &plan, &part, None, opts)
        .await
        .unwrap();
    assert_eq!(std::fs::read(&part).unwrap(), bytes);
}
