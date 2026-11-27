use tgdl_core::model::{ChunkRange, FileMeta};
use tgdl_core::source::ChunkSource;
use tgdl_core::source::mock::MockSource;

#[tokio::test]
async fn mock_returns_range() {
    let data = b"hello world".to_vec();
    let src = MockSource::new(data.clone());
    let r = src
        .fetch(ChunkRange {
            offset: 0,
            length: 5,
        })
        .await
        .unwrap();
    assert_eq!(&r[..], b"hello");
    let r2 = src
        .fetch(ChunkRange {
            offset: 6,
            length: 5,
        })
        .await
        .unwrap();
    assert_eq!(&r2[..], b"world");
}

#[tokio::test]
async fn plan_splits() {
    let meta = FileMeta {
        unique_id: "x".into(),
        size: 10,
        chunk_size: 3,
    };
    let plan = tgdl_core::DownloadPlan::new(meta);
    assert_eq!(plan.chunks.len(), 4);
}
