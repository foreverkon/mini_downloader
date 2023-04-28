use std::{path::PathBuf, str::FromStr};

#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub url: String,
    pub filename: PathBuf,
}

impl DownloadTask {
    pub fn new(url: &str, filename: &str) -> Self {
        Self {
            url: url.to_string(),
            filename: PathBuf::from(filename),
        }
    }
}

impl FromStr for DownloadTask {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let url = reqwest::Url::parse(s)?;
        let filename = url
            .path_segments()
            .and_then(|segments| segments.last())
            .unwrap_or("");
        if filename.is_empty() {
            anyhow::bail!("Cannot infer filename from {}", s);
        }
        Ok(DownloadTask {
            url: s.to_string(),
            filename: PathBuf::from(filename),
        })
    }
}
