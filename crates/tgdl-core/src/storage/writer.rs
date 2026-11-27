use std::path::Path;
use tokio::fs;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

pub async fn write_chunk(path: &Path, offset: u64, bytes: &[u8]) -> Result<(), std::io::Error> {
    let mut file = fs::OpenOptions::new().write(true).open(path).await?;
    file.seek(std::io::SeekFrom::Start(offset)).await?;
    file.write_all(bytes).await?;
    Ok(())
}
