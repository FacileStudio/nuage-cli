use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::io::AsyncWriteExt;

pub(crate) async fn create_dest_file(dest: &Path) -> Result<tokio::fs::File> {
    tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("cannot create file: {}", dest.display()))
}

pub(crate) async fn read_chunk<B>(chunk: reqwest::Result<B>, dest: &Path, id: i64) -> Result<B> {
    match chunk {
        Ok(chunk) => Ok(chunk),
        Err(err) => {
            let _ = tokio::fs::remove_file(dest).await;
            Err(anyhow::Error::new(err)
                .context(format!("failed to read download stream for file {}", id)))
        }
    }
}

pub(crate) async fn write_chunk(
    out: &mut tokio::fs::File,
    chunk: &[u8],
    dest: &Path,
) -> Result<()> {
    if let Err(err) = out.write_all(chunk).await {
        let _ = tokio::fs::remove_file(dest).await;
        return Err(
            anyhow::Error::new(err).context(format!("cannot write file: {}", dest.display()))
        );
    }
    Ok(())
}

pub(crate) async fn flush_dest(out: tokio::fs::File, dest: &Path) -> Result<()> {
    let mut out = out;
    if let Err(err) = out.flush().await {
        let _ = tokio::fs::remove_file(dest).await;
        return Err(
            anyhow::Error::new(err).context(format!("cannot flush file: {}", dest.display()))
        );
    }
    Ok(())
}

pub(crate) async fn verify_integrity(
    hasher: Sha256,
    expected_hash: Option<&str>,
    id: i64,
    dest: &Path,
) -> Result<()> {
    let Some(expected) = expected_hash else {
        return Ok(());
    };
    let computed = format!("{:x}", hasher.finalize());
    if computed == expected {
        return Ok(());
    }
    let _ = tokio::fs::remove_file(dest).await;
    anyhow::bail!(
        "integrity check failed for file {}: expected {}, got {}",
        id,
        expected,
        computed
    );
}
