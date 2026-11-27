use crate::error::DownloadError;
use sha2::{Digest, Sha256};
use std::path::Path;

pub fn verify_size(path: &Path, expected: u64) -> Result<(), DownloadError> {
    let meta = std::fs::metadata(path).map_err(DownloadError::Io)?;
    if meta.len() != expected {
        return Err(DownloadError::Verify(format!(
            "size mismatch: {} vs {}",
            meta.len(),
            expected
        )));
    }
    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<String, DownloadError> {
    let mut file = std::fs::File::open(path).map_err(DownloadError::Io)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = std::io::Read::read(&mut file, &mut buf).map_err(DownloadError::Io)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let bytes = hasher.finalize();
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes.iter() {
        out.push_str(&format!("{:02x}", b));
    }
    Ok(out)
}
