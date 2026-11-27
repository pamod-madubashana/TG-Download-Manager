pub mod config;
pub mod error;
pub mod model;
pub mod progress;
pub mod source;
pub mod verify;

#[cfg(feature = "mock")]
pub use source::mock::MockSource;

#[cfg(feature = "telegram")]
pub mod bots;

pub mod scheduler;
pub mod storage;

pub use config::Config;
pub use error::{DownloadError, FetchError};
pub use model::{ChunkRange, DownloadPlan, FileMeta};
pub use source::ChunkSource;
