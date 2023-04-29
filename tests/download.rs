use mini_downloader::prelude::*;

#[tokio::test]
async fn test_download_then_save() -> anyhow::Result<()> {
    let url = "https://raw.kiko-play-niptan.one/media/download/hvdb/3000-3099/RJ197855/%E7%AC%AC%EF%BC%91%E7%AB%A0%20mp3/%E3%81%AF%E3%81%98%E3%82%81%E3%81%AB.mp3";
    let task = DownloadTask::new(url, "test1.mp3");
    let task2 = DownloadTask::new(url, "test2.mp3");
    let task3 = DownloadTask::new(url, "test3.mp3");
    let task4 = DownloadTask::new(url, "test4.mp3");
    let tasks = vec![task, task2, task3, task4];

    let downloader = DownloaderBuilder::new().workers(12).build();

    downloader.download(tasks).await?;

    anyhow::Ok(())
}
