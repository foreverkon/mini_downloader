use std::sync::Arc;

use reqwest::header::{HeaderValue, ACCEPT_RANGES, CONTENT_LENGTH, RANGE};
use reqwest_middleware::ClientWithMiddleware;
use tokio::{
    fs::File,
    io::{AsyncSeekExt, AsyncWriteExt},
    sync::Mutex,
};

#[derive(Debug, Clone, Copy)]
pub struct Chunks {
    start: usize,
    total: usize,
    chunk_size: usize,
}

pub struct Chunk {
    data: bytes::Bytes,
    start: usize,
    chunk_size: usize,
}

pub struct ChunkVec {
    chunks: Vec<Chunk>,
    #[allow(unused)]
    total: usize,
}

impl ChunkVec {
    /// save all chunk to file
    pub async fn save(self, f: Arc<Mutex<File>>) -> anyhow::Result<()> {
        let mut tasks = Vec::new();
        for chunk in self.chunks.into_iter() {
            let f = f.clone();
            tasks.push(tokio::spawn(async move { chunk.save(f).await }));
        }
        for task in tasks {
            task.await??;
        }
        anyhow::Ok(())
    }

    #[allow(unused)]
    pub async fn verify(&mut self) -> anyhow::Result<()> {
        self.chunks.sort_by_key(|c| c.start);
        let flag = {
            if let Some(first) = self.chunks.first() {
                first.start == 0
            } else if let Some(last) = self.chunks.last() {
                last.start + last.chunk_size == self.total
            } else {
                false
            }
        };

        if flag {
            anyhow::bail!("Incomplete chunk")
        }

        for chunk in self.chunks.windows(2) {
            let c1 = &chunk[0];
            let c2 = &chunk[1];
            if c1.start + c1.chunk_size != c2.start {
                anyhow::bail!("Incomplete chunk")
            }
        }
        anyhow::Ok(())
    }
}

impl Chunk {
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

impl Chunks {
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
    ) -> anyhow::Result<()> {
        let f1 = f.clone();
        self.download(client, url).await?.save(f).await?;
        self.verify(f1).await?;
        Ok(())
    }

    /// download and save concurrently
    pub async fn download_and_save(
        self,
        client: &ClientWithMiddleware,
        url: &str,
        f: Arc<Mutex<File>>,
    ) -> anyhow::Result<()> {
        let mut tasks = Vec::new();
        for chunk in self {
            let client = client.clone();
            let url = url.to_string();
            let f = f.clone();
            tasks.push(tokio::spawn(async move {
                chunk.download_and_save(&client, &url, f).await
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
