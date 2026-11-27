use crate::model::FileMeta;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

const MAGIC: &[u8] = b"TGDJ";

pub struct Journal {
    path: std::path::PathBuf,
    pub unique_id: String,
    pub total_size: u64,
    pub chunk_size: u64,
    pub num_chunks: u64,
    bitmap: Vec<u8>,
}

impl Journal {
    pub fn create(path: impl AsRef<Path>, meta: &FileMeta) -> Result<Self, std::io::Error> {
        let num_chunks = meta.size.div_ceil(meta.chunk_size);
        let bitmap = vec![0u8; (num_chunks as usize).div_ceil(8)];
        let j = Self {
            path: path.as_ref().to_path_buf(),
            unique_id: meta.unique_id.clone(),
            total_size: meta.size,
            chunk_size: meta.chunk_size,
            num_chunks,
            bitmap,
        };
        j.persist()?;
        Ok(j)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Option<Self>, std::io::Error> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(None);
        }
        let mut buf = Vec::new();
        fs::File::open(path)?.read_to_end(&mut buf)?;
        if buf.len() < 4 + 4 {
            return Ok(None);
        }
        if &buf[0..4] != MAGIC {
            return Ok(None);
        }
        let mut cur = 4;
        let uid_len = u32::from_le_bytes(buf[cur..cur + 4].try_into().unwrap()) as usize;
        cur += 4;
        if cur + uid_len + 24 > buf.len() {
            return Ok(None);
        }
        let unique_id = String::from_utf8_lossy(&buf[cur..cur + uid_len]).to_string();
        cur += uid_len;
        let total_size = u64::from_le_bytes(buf[cur..cur + 8].try_into().unwrap());
        cur += 8;
        let chunk_size = u64::from_le_bytes(buf[cur..cur + 8].try_into().unwrap());
        cur += 8;
        let num_chunks = u64::from_le_bytes(buf[cur..cur + 8].try_into().unwrap());
        cur += 8;
        let bitmap = buf[cur..].to_vec();
        Ok(Some(Self {
            path: path.to_path_buf(),
            unique_id,
            total_size,
            chunk_size,
            num_chunks,
            bitmap,
        }))
    }

    pub fn matches(&self, meta: &FileMeta) -> bool {
        self.unique_id == meta.unique_id
            && self.total_size == meta.size
            && self.chunk_size == meta.chunk_size
    }

    pub fn is_done(&self, idx: u64) -> bool {
        let byte = (idx / 8) as usize;
        if byte >= self.bitmap.len() {
            return false;
        }
        (self.bitmap[byte] >> (idx % 8)) & 1 == 1
    }

    pub fn mark(&mut self, idx: u64) -> Result<(), std::io::Error> {
        let byte = (idx / 8) as usize;
        if byte >= self.bitmap.len() {
            return Ok(());
        }
        self.bitmap[byte] |= 1 << (idx % 8);
        self.persist()
    }

    fn persist(&self) -> Result<(), std::io::Error> {
        let mut buf = Vec::new();
        buf.extend_from_slice(MAGIC);
        let uid = self.unique_id.as_bytes();
        buf.extend_from_slice(&(uid.len() as u32).to_le_bytes());
        buf.extend_from_slice(uid);
        buf.extend_from_slice(&self.total_size.to_le_bytes());
        buf.extend_from_slice(&self.chunk_size.to_le_bytes());
        buf.extend_from_slice(&self.num_chunks.to_le_bytes());
        buf.extend_from_slice(&self.bitmap);
        let mut f = fs::File::create(&self.path)?;
        f.write_all(&buf)?;
        f.flush()?;
        Ok(())
    }
}
