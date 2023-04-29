#![allow(unused)]

pub mod chunk;
pub mod chunk_vec;
pub mod chunks;
pub mod download_task;
pub mod downloader;

pub mod prelude {
    pub use crate::download_task::DownloadTask;
    pub use crate::downloader::{DownloadPolicy, Downloader, DownloaderBuilder};
}
