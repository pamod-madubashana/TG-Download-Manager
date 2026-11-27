use tempfile::tempdir;
use tgdl_core::model::FileMeta;
use tgdl_core::storage::journal::Journal;

#[test]
fn journal_roundtrip() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("a.progress");
    let meta = FileMeta {
        unique_id: "u".into(),
        size: 100,
        chunk_size: 10,
    };
    let mut j = Journal::create(&path, &meta).unwrap();
    j.mark(0).unwrap();
    j.mark(3).unwrap();
    let j2 = Journal::open(&path).unwrap().unwrap();
    assert!(j2.matches(&meta));
    assert!(j2.is_done(0));
    assert!(j2.is_done(3));
    assert!(!j2.is_done(1));
}

#[test]
fn journal_mismatch() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("b.progress");
    let meta = FileMeta {
        unique_id: "u".into(),
        size: 100,
        chunk_size: 10,
    };
    Journal::create(&path, &meta).unwrap();
    let meta2 = FileMeta {
        unique_id: "u".into(),
        size: 200,
        chunk_size: 10,
    };
    let j = Journal::open(&path).unwrap().unwrap();
    assert!(!j.matches(&meta2));
}
