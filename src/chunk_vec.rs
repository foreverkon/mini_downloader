use std::sync::Arc;

use indicatif::ProgressBar;
use tokio::{fs::File, sync::Mutex};

use crate::chunk::Chunk;

pub struct ChunkVec {
    pub(crate) chunks: Vec<Chunk>,
    pub(crate) total: usize,
}

impl ChunkVec {
    pub fn chunks(&self) -> &Vec<Chunk> {
        &self.chunks
    }

    pub fn total(&self) -> usize {
        self.total
    }

    /// save all chunk to file
    pub async fn save(self, f: Arc<Mutex<File>>, pb: ProgressBar) -> anyhow::Result<()> {
        let mut tasks = Vec::new();
        for chunk in self.chunks.into_iter() {
            let f = f.clone();
            let pb = pb.clone();
            tasks.push(tokio::spawn(async move {
                chunk.save(f).await?;
                pb.inc(chunk.chunk_size as u64);
                anyhow::Ok(())
            }));
        }
        for task in tasks {
            task.await??;
        }
        anyhow::Ok(())
    }

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
