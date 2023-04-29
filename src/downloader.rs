use std::path::{PathBuf, Path};
use std::sync::Arc;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use tokio::sync::Mutex;

use crate::chunks::Chunks;
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
        let m = MultiProgress::new();
        let style =
            ProgressStyle::with_template("{elapsed:>3} [{bar:20.cyan/blue}] {percent:>3}% {msg:<}")
                .unwrap()
                .progress_chars("##-");

        for DownloadTask { url, filename } in tasks {
            let client = self.client.clone();
            let workers = self.workers;
            let path = self.dir.join(&filename);

            let pb = m.add(ProgressBar::new(u64::MAX));
            pb.set_style(style.clone());
            pb.set_message(format!("{} ..", filename.display()));

            let task = match self.policy {
                DownloadPolicy::DownloadThenSave => tokio::spawn(async move {
                    let (chunks, file) = tokio::join!(
                        Chunks::new(&client, &url, workers),
                        tokio::fs::File::create(path),
                    );

                    let chunks = chunks?;
                    let file = Arc::new(Mutex::new(file?));

                    match chunks
                        .download_then_save(&client, &url, file, pb.clone())
                        .await
                    {
                        Ok(_) => {
                            pb.finish_with_message(format!("{} \u{2705}", filename.display()));
                            anyhow::Ok(())
                        }
                        Err(e) => {
                            pb.finish_with_message(format!("{} \u{274C}", filename.display()));
                            Err(e)
                        }
                    }
                }),
                DownloadPolicy::DownloadAndSave => tokio::spawn(async move {
                    let (chunks, file) = tokio::join!(
                        Chunks::new(&client, &url, workers),
                        tokio::fs::File::create(path),
                    );

                    let chunks = chunks?;
                    let file = Arc::new(Mutex::new(file?));

                    match chunks
                        .download_and_save(&client, &url, file, pb.clone())
                        .await
                    {
                        Ok(_) => {
                            pb.finish_with_message(format!("{} \u{2705}", filename.display()));
                            anyhow::Ok(())
                        }
                        Err(e) => {
                            pb.finish_with_message(format!("{} \u{274C}", filename.display()));
                            Err(e)
                        }
                    }
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

    pub fn dir<P: AsRef<Path>>(mut self, dir: P) -> Self {
        self.dir = Some(dir.as_ref().to_path_buf());
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
