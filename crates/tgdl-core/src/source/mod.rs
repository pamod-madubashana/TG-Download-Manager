use crate::error::FetchError;
use crate::model::ChunkRange;
use async_trait::async_trait;
use bytes::Bytes;

#[async_trait]
pub trait ChunkSource: Send + Sync {
    async fn fetch(&self, range: ChunkRange) -> Result<Bytes, FetchError>;
}

#[cfg(feature = "mock")]
pub mod mock;

#[cfg(feature = "telegram")]
pub mod telegram;
