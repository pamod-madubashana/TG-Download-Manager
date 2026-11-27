use std::sync::Arc;
use tempfile::tempdir;
use tgdl_core::model::{DownloadPlan, FileMeta};
use tgdl_core::scheduler::{Scheduler, SchedulerOptions};
use tgdl_core::source::mock::MockSource;

fn data() -> Vec<u8> {
    (0..4096u32).map(|i| (i % 251) as u8).collect()
}

fn meta(len: usize) -> FileMeta {
    FileMeta {
        unique_id: "test".into(),
        size: len as u64,
        chunk_size: 1024,
    }
}

#[tokio::test]
async fn downloads_all_chunks() {
    let dir = tempdir().unwrap();
    let bytes = data();
    let src = MockSource::new(bytes.clone());
    let plan = DownloadPlan::new(meta(bytes.len()));
    let part = dir.path().join("out.part");
    let opts = SchedulerOptions {
        slots_per_bot: 4,
        retry_base: std::time::Duration::from_millis(1),
        ..Default::default()
    };
    Scheduler::run(vec![Arc::new(src)], &plan, &part, None, opts)
        .await
        .unwrap();
    assert_eq!(std::fs::read(&part).unwrap(), bytes);
}

#[tokio::test]
async fn retries_flaky_source() {
    let dir = tempdir().unwrap();
    let bytes = data();
    let src = MockSource::failing(bytes.clone(), 3);
    let plan = DownloadPlan::new(meta(bytes.len()));
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
