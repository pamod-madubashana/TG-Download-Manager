pub type ChunkId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkRange {
    pub offset: u64,
    pub length: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMeta {
    pub unique_id: String,
    pub size: u64,
    pub chunk_size: u64,
}

#[derive(Clone, Debug)]
pub struct DownloadPlan {
    pub meta: FileMeta,
    pub chunks: Vec<ChunkRange>,
}

impl DownloadPlan {
    pub fn new(meta: FileMeta) -> Self {
        let chunk_size = meta.chunk_size.max(1);
        let n = meta.size.div_ceil(chunk_size);
        let mut chunks = Vec::with_capacity(n as usize);
        for i in 0..n {
            let off = i * chunk_size;
            let len = (meta.size - off).min(chunk_size);
            chunks.push(ChunkRange {
                offset: off,
                length: len,
            });
        }
        Self { meta, chunks }
    }
}
