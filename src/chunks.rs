use std::sync::Arc;

use indicatif::ProgressBar;
use reqwest::header::{HeaderValue, ACCEPT_RANGES, CONTENT_LENGTH};
use reqwest_middleware::ClientWithMiddleware;
use tokio::{fs::File, sync::Mutex};

use crate::chunk::Chunk;
use crate::chunk_vec::ChunkVec;

#[derive(Debug, Clone, Copy)]
pub struct Chunks {
    pub(crate) start: usize,
    pub(crate) total: usize,
    pub(crate) chunk_size: usize,
}

impl Chunks {
    pub fn start(&self) -> usize {
        self.start
    }

    pub fn total(&self) -> usize {
        self.total
    }

    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    pub async fn new(
        client: &ClientWithMiddleware,
        url: &str,
        workers: usize,
    ) -> anyhow::Result<Self> {
        let (accept_ranges, total) = Self::get_content_length(client, url).await?;
        if !accept_ranges || total < 1024 * 1024 * 4 {
            Ok(Self {
                start: 0,
                total,
                chunk_size: total,
            })
        } else {
            let chunk_size = total / workers + 1;
            Ok(Self {
                start: 0,
                total,
                chunk_size,
            })
        }
    }

    /// download all chunk and then save them
    pub async fn download_then_save(
        self,
        client: &ClientWithMiddleware,
        url: &str,
        f: Arc<Mutex<File>>,
        pb: ProgressBar,
    ) -> anyhow::Result<()> {
        pb.set_length(self.total as u64);
        let f_clone = f.clone();
        self.download(client, url).await?.save(f, pb).await?;
        self.verify(f_clone).await?;
        Ok(())
    }

    /// download and save concurrently
    pub async fn download_and_save(
        self,
        client: &ClientWithMiddleware,
        url: &str,
        f: Arc<Mutex<File>>,
        pb: ProgressBar,
    ) -> anyhow::Result<()> {
        pb.set_length(self.total as u64);
        let mut tasks = Vec::new();
        for chunk in self {
            let client = client.clone();
            let url = url.to_string();
            let f = f.clone();
            let pb = pb.clone();
            tasks.push(tokio::spawn(async move {
                let size = chunk.chunk_size;
                chunk.download_and_save(&client, &url, f).await?;
                pb.inc(size as u64);
                anyhow::Ok(())
            }))
        }
        for task in tasks {
            task.await??;
        }
        self.verify(f).await?;
        Ok(())
    }

    /// download all chunks
    pub async fn download(
        self,
        client: &ClientWithMiddleware,
        url: &str,
    ) -> anyhow::Result<ChunkVec> {
        let mut tasks = Vec::new();
        for chunk in self {
            let client = client.clone();
            let url = url.to_string();
            tasks.push(tokio::spawn(
                async move { chunk.download(&client, &url).await },
            ))
        }
        let mut chunks = Vec::new();
        for task in tasks {
            chunks.push(task.await??);
        }
        Ok(ChunkVec {
            chunks,
            total: self.total,
        })
    }

    async fn verify(&self, f: Arc<Mutex<File>>) -> anyhow::Result<()> {
        let file = f.lock().await;
        file.sync_all().await?;
        let metadata = file.metadata().await?;
        let size = metadata.len() as usize;
        if size != self.total {
            anyhow::bail!("Expected {} bytes, got {}", self.total, metadata.len());
        }
        anyhow::Ok(())
    }

    async fn get_content_length(
        client: &ClientWithMiddleware,
        url: &str,
    ) -> anyhow::Result<(bool, usize)> {
        let resp = client.head(url).send().await?;
        let headers = resp.headers();
        let accept_ranges = headers
            .get(ACCEPT_RANGES)
            .map(|v| v == HeaderValue::from_static("bytes"))
            .unwrap_or(false);

        let total = headers.get(CONTENT_LENGTH).map_or(0, |v| {
            v.to_str()
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0)
        });
        Ok((accept_ranges, total))
    }
}

impl Iterator for Chunks {
    type Item = Chunk;

    fn next(&mut self) -> Option<Self::Item> {
        if self.start >= self.total {
            return None;
        }
        let size = self.chunk_size.min(self.total - self.start);
        let chunk = Chunk {
            data: bytes::Bytes::new(),
            start: self.start,
            chunk_size: size,
        };
        self.start += size;
        Some(chunk)
    }
}
