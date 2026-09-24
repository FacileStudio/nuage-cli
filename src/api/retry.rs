use super::{ApiClient, BASE_DELAY_MS, MAX_ATTEMPTS, MAX_DELAY_MS, MAX_RETRY_AFTER_SECS};
use anyhow::Result;
use std::future::Future;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) fn is_retryable_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request()
}

pub(crate) fn is_retryable_anyhow(err: &anyhow::Error) -> bool {
    err.downcast_ref::<reqwest::Error>()
        .map(is_retryable_error)
        .unwrap_or(false)
}

pub(crate) fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    status.as_u16() == 429 || status.is_server_error()
}

fn jitter_ms(bound: u64) -> u64 {
    if bound == 0 {
        return 0;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    nanos % bound
}

pub(crate) fn backoff_delay(attempt: u32) -> Duration {
    let factor = 1u64
        .checked_shl(attempt.saturating_sub(1))
        .unwrap_or(u64::MAX);
    let base = BASE_DELAY_MS.saturating_mul(factor).min(MAX_DELAY_MS);
    let total = base
        .saturating_add(jitter_ms(base / 2 + 1))
        .min(MAX_DELAY_MS);
    Duration::from_millis(total)
}

pub(crate) fn retry_after_delay(resp: &reqwest::Response) -> Option<Duration> {
    let raw = resp.headers().get(reqwest::header::RETRY_AFTER)?;
    let secs = raw.to_str().ok()?.trim().parse::<u64>().ok()?;
    Some(Duration::from_secs(secs.min(MAX_RETRY_AFTER_SECS)))
}

pub(crate) async fn sleep_before_retry(attempt: u32, resp: Option<&reqwest::Response>) {
    let delay = resp
        .and_then(retry_after_delay)
        .unwrap_or_else(|| backoff_delay(attempt));
    tokio::time::sleep(delay).await;
}

impl ApiClient {
    pub(crate) async fn send_with_retry<F, Fut>(
        &self,
        what: &str,
        build: F,
    ) -> Result<reqwest::Response>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = reqwest::Result<reqwest::Response>>,
    {
        let mut attempt: u32 = 1;
        loop {
            match build().await {
                Ok(resp) if attempt < MAX_ATTEMPTS && is_retryable_status(resp.status()) => {
                    sleep_before_retry(attempt, Some(&resp)).await;
                }
                Ok(resp) => return Ok(resp),
                Err(err) if attempt < MAX_ATTEMPTS && is_retryable_error(&err) => {
                    sleep_before_retry(attempt, None).await;
                }
                Err(err) => return Err(anyhow::Error::new(err).context(what.to_string())),
            }
            attempt += 1;
        }
    }
}
