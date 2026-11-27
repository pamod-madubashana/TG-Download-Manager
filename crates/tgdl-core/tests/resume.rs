use std::io::Write;
use std::sync::Arc;
use tempfile::tempdir;
use tgdl_core::model::{DownloadPlan, FileMeta};
use tgdl_core::scheduler::{Scheduler, SchedulerOptions};
use tgdl_core::source::mock::MockSource;
use tgdl_core::storage::journal::Journal;

#[tokio::test]
async fn resumes_and_skips_done_chunks() {
    let dir = tempdir().unwrap();
    let bytes: Vec<u8> = (0..2048).map(|i| (i % 53) as u8).collect();
    let meta = FileMeta {
        unique_id: "resume".into(),
        size: bytes.len() as u64,
        chunk_size: 1024,
    };
    let plan = DownloadPlan::new(meta.clone());

    let part = dir.path().join("out.part");
    let progress = dir.path().join("out.progress");
    let mut f = std::fs::File::create(&part).unwrap();
    f.write_all(&bytes[..1024]).unwrap();
    f.set_len(bytes.len() as u64).unwrap();
    drop(f);

    let mut journal = Journal::create(&progress, &meta).unwrap();
    journal.mark(0).unwrap();

    let src = MockSource::new(bytes.clone());
    let opts = SchedulerOptions {
        slots_per_bot: 2,
        retry_base: std::time::Duration::from_millis(1),
        ..Default::default()
    };
    Scheduler::run(
        vec![Arc::new(src.clone())],
        &plan,
        &part,
        Some(&progress),
        opts,
    )
    .await
    .unwrap();

    assert_eq!(src.fetch_count(), 1);
    assert_eq!(std::fs::read(&part).unwrap(), bytes);
}
