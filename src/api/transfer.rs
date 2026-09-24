use super::retry::{
    backoff_delay, is_retryable_anyhow, is_retryable_error, is_retryable_status, retry_after_delay,
};
use super::stream::{create_dest_file, flush_dest, read_chunk, verify_integrity, write_chunk};
use super::{ApiClient, MAX_ATTEMPTS};
use anyhow::Result;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;

struct DownloadRequest<'a> {
    id: i64,
    dest: &'a Path,
    expected_hash: Option<&'a str>,
    url: String,
}

enum DownloadOutcome {
    Done,
    Retry(Duration),
    Failed(anyhow::Error),
}

fn send_outcome(err: reqwest::Error, id: i64, attempt: u32) -> DownloadOutcome {
    if attempt < MAX_ATTEMPTS && is_retryable_error(&err) {
        return DownloadOutcome::Retry(backoff_delay(attempt));
    }
    DownloadOutcome::Failed(
        anyhow::Error::new(err).context(format!("failed to download file {}", id)),
    )
}

async fn status_outcome(resp: reqwest::Response, id: i64, attempt: u32) -> DownloadOutcome {
    let status = resp.status();
    if attempt < MAX_ATTEMPTS && is_retryable_status(status) {
        let delay = retry_after_delay(&resp).unwrap_or_else(|| backoff_delay(attempt));
        return DownloadOutcome::Retry(delay);
    }
    let body = resp.text().await.unwrap_or_default();
    DownloadOutcome::Failed(anyhow::anyhow!(
        "GET /files/{}/download failed ({}): {}",
        id,
        status,
        body
    ))
}

fn stream_outcome(err: anyhow::Error, attempt: u32) -> DownloadOutcome {
    if attempt < MAX_ATTEMPTS && is_retryable_anyhow(&err) {
        return DownloadOutcome::Retry(backoff_delay(attempt));
    }
    DownloadOutcome::Failed(err)
}

impl ApiClient {
    /// Streams `GET /files/{id}/download` straight to `dest`, hashing on the fly.
    ///
    /// The response body is never fully buffered in memory. When `expected_hash`
    /// is set and the computed SHA-256 digest differs, the partial file is
    /// removed and an error mentioning `integrity check failed` is returned.
    pub async fn download_to_file(
        &self,
        id: i64,
        dest: &std::path::Path,
        expected_hash: Option<&str>,
    ) -> Result<()> {
        let request = DownloadRequest {
            id,
            dest,
            expected_hash,
            url: self.scoped_url(format!("{}/files/{}/download", self.base_url(), id)),
        };

        let mut attempt: u32 = 1;
        loop {
            match self.download_attempt(&request, attempt).await {
                DownloadOutcome::Done => return Ok(()),
                DownloadOutcome::Failed(err) => return Err(err),
                DownloadOutcome::Retry(delay) => {
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }

    async fn download_attempt(
        &self,
        request: &DownloadRequest<'_>,
        attempt: u32,
    ) -> DownloadOutcome {
        let client = self.transfer();
        let token = self.token();
        let sent = client.get(&request.url).bearer_auth(token).send().await;

        let resp = match sent {
            Ok(resp) => resp,
            Err(err) => return send_outcome(err, request.id, attempt),
        };

        if !resp.status().is_success() {
            return status_outcome(resp, request.id, attempt).await;
        }

        stream_response_to_file(resp, request.id, request.dest, request.expected_hash)
            .await
            .map_or_else(
                |err| stream_outcome(err, attempt),
                |()| DownloadOutcome::Done,
            )
    }
}

async fn stream_response_to_file(
    resp: reqwest::Response,
    id: i64,
    dest: &Path,
    expected_hash: Option<&str>,
) -> Result<()> {
    let mut out = create_dest_file(dest).await?;
    let mut hasher = Sha256::new();
    let mut stream = resp.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = read_chunk(chunk, dest, id).await?;
        write_chunk(&mut out, &chunk, dest).await?;
        hasher.update(&chunk);
    }

    flush_dest(out, dest).await?;
    verify_integrity(hasher, expected_hash, id, dest).await
}
