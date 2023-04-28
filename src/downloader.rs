use std::path::PathBuf;
use std::sync::Arc;

use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use tokio::sync::Mutex;

use crate::chunk::Chunks;
use crate::download_task::DownloadTask;

pub enum DownloadPolicy {
    /// download all chunks and then save them to disk
    DownloadThenSave,

    /// download and save concurrently
    DownloadAndSave,
}

pub struct Downloader {
    workers: usize,
    retry: usize,
    dir: PathBuf,
    client: ClientWithMiddleware,
    policy: DownloadPolicy,
}

impl Default for Downloader {
    fn default() -> Self {
        DownloaderBuilder::default().build()
    }
}

impl Downloader {
    pub fn workers(&self) -> usize {
        self.workers
    }

    pub fn retry(&self) -> usize {
        self.retry
    }

    pub fn client(&self) -> &ClientWithMiddleware {
        &self.client
    }

    pub async fn download<T>(&self, tasks: T) -> anyhow::Result<()>
    where
        T: IntoIterator<Item = DownloadTask>,
    {
        let mut futures: Vec<tokio::task::JoinHandle<anyhow::Result<()>>> = Vec::new();

        for DownloadTask { url, filename } in tasks {
            let client = self.client.clone();
            let workers = self.workers;
            let path = self.dir.join(filename);

            let task = match self.policy {
                DownloadPolicy::DownloadThenSave => tokio::spawn(async move {
                    let file = tokio::fs::File::create(path).await?;
                    let file = Arc::new(Mutex::new(file));
                    let chunks = Chunks::new(&client, &url, workers).await?;
                    chunks.download_then_save(&client, &url, file).await?;
                    anyhow::Ok(())
                }),
                DownloadPolicy::DownloadAndSave => tokio::spawn(async move {
                    let file = tokio::fs::File::create(path).await?;
                    let file = Arc::new(Mutex::new(file));
                    let chunks = Chunks::new(&client, &url, workers).await?;
                    chunks.download_and_save(&client, &url, file).await?;
                    anyhow::Ok(())
                }),
            };

            futures.push(task);
        }
        for future in futures {
            future.await??;
        }
        anyhow::Ok(())
    }
}

#[derive(Default)]
pub struct DownloaderBuilder {
    workers: Option<usize>,
    retry: Option<usize>,
    dir: Option<PathBuf>,
    policy: Option<DownloadPolicy>,
}

impl DownloaderBuilder {
    const DEFAULT_WORKERS: usize = 4;
    const DEFAULT_RETRY: usize = 2;
    const DEFAULT_DIR: &str = "./";
    const DEFAULT_POLICY: DownloadPolicy = DownloadPolicy::DownloadAndSave;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn workers(mut self, workers: usize) -> Self {
        self.workers = Some(workers);
        self
    }

    pub fn retry(mut self, retry: usize) -> Self {
        self.retry = Some(retry);
        self
    }

    pub fn dir(mut self, dir: &str) -> Self {
        self.dir = Some(PathBuf::from(dir));
        self
    }

    pub fn policy(mut self, policy: DownloadPolicy) -> Self {
        self.policy = Some(policy);
        self
    }

    pub fn build(self) -> Downloader {
        Downloader {
            client: self.build_client(self.retry),
            policy: self.policy.unwrap_or(Self::DEFAULT_POLICY),
            workers: self.workers.unwrap_or(Self::DEFAULT_WORKERS),
            retry: self.retry.unwrap_or(Self::DEFAULT_RETRY),
            dir: self.dir.unwrap_or(PathBuf::from(Self::DEFAULT_DIR)),
        }
    }

    fn build_client(&self, retry: Option<usize>) -> ClientWithMiddleware {
        let retry_policy = ExponentialBackoff::builder()
            .build_with_max_retries(retry.unwrap_or(Self::DEFAULT_RETRY) as u32);
        let client = reqwest::ClientBuilder::new().build().unwrap();
        ClientBuilder::new(client)
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build()
    }
}
