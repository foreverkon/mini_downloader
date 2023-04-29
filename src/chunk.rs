use std::sync::Arc;

use indicatif::ProgressBar;
use reqwest::header::RANGE;
use reqwest_middleware::ClientWithMiddleware;
use tokio::{
    fs::File,
    io::{AsyncSeekExt, AsyncWriteExt},
    sync::Mutex,
};

pub struct Chunk {
    pub(crate) data: bytes::Bytes,
    pub(crate) start: usize,
    pub(crate) chunk_size: usize,
}

impl Chunk {
    pub fn data(&self) -> &bytes::Bytes {
        &self.data
    }

    pub fn start(&self) -> usize {
        self.start
    }

    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    /// download one chunk from the url
    pub async fn download(
        mut self,
        client: &ClientWithMiddleware,
        url: &str,
    ) -> anyhow::Result<Self> {
        let resp = client
            .get(url)
            .header(
                RANGE,
                format!("bytes={}-{}", self.start, self.start + self.chunk_size - 1),
            )
            .send()
            .await?;
        let data = resp.bytes().await?;
        self.data = data;
        if self.data.len() != self.chunk_size {
            anyhow::bail!(
                "Expected {} bytes, got {}",
                self.chunk_size,
                self.data.len()
            );
        }
        anyhow::Ok(self)
    }

    /// save one chunk to file
    pub async fn save(&self, f: Arc<Mutex<File>>) -> anyhow::Result<()> {
        let mut file = f.lock().await;
        file.seek(std::io::SeekFrom::Start(self.start as u64))
            .await?;
        file.write_all(&self.data).await?;
        Ok(())
    }

    /// download and then save one chunk
    pub async fn download_and_save(
        self,
        client: &ClientWithMiddleware,
        url: &str,
        f: Arc<Mutex<File>>,
    ) -> anyhow::Result<()> {
        self.download(client, url).await?.save(f).await?;
        Ok(())
    }
}
