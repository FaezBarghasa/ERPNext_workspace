use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use futures_util::Stream;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};

pub mod local_fs {
    use super::*;

    pub async fn store_file_stream<S, E>(
        mut stream: S,
        tenant_id: &str,
        storage_root: &str,
    ) -> Result<String, std::io::Error>
    where
        S: Stream<Item = Result<Vec<u8>, E>> + Unpin,
        E: Into<std::io::Error>,
    {
        // 1. Prepare tenant directory
        let tenant_dir = PathBuf::from(storage_root).join(tenant_id);
        tokio::fs::create_dir_all(&tenant_dir).await?;

        // 2. We first write to a temp file, computing the SHA-256 hash
        let temp_filename = format!("temp_{}", uuid::Uuid::new_v4());
        let temp_path = tenant_dir.join(&temp_filename);
        
        let mut file = tokio::fs::File::create(&temp_path).await?;
        let mut hasher = Sha256::new();

        while let Some(chunk_res) = stream.next().await {
            let chunk = chunk_res.map_err(|e| e.into())?;
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        drop(file);

        // 3. Rename temp file to its hex SHA-256 hash (deduplication)
        let hash = format!("{:x}", hasher.finalize());
        let final_path = tenant_dir.join(&hash);

        if final_path.exists() {
            // Deduplicated: remove the temp file
            tokio::fs::remove_file(&temp_path).await?;
        } else {
            tokio::fs::rename(&temp_path, &final_path).await?;
        }

        Ok(hash)
    }
}
