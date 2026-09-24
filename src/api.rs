#[path = "api/account.rs"]
mod account;
#[path = "api/folders.rs"]
mod folders;
#[path = "api/keys.rs"]
mod keys;
#[path = "api/retry.rs"]
mod retry;
#[path = "api/shares.rs"]
mod shares;
#[path = "api/stream.rs"]
mod stream;
#[path = "api/tokens.rs"]
mod tokens;
#[path = "api/transfer.rs"]
mod transfer;
#[path = "api/types.rs"]
mod types;
#[path = "api/upload.rs"]
mod upload;
#[path = "api/upload_session.rs"]
mod upload_session;

pub use types::*;

use anyhow::Result;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

pub(crate) const MAX_ATTEMPTS: u32 = 4;
pub(crate) const BASE_DELAY_MS: u64 = 500;
pub(crate) const MAX_DELAY_MS: u64 = 8_000;
pub(crate) const MAX_RETRY_AFTER_SECS: u64 = 60;
pub(crate) const UPLOAD_CHUNK_SIZE: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SpacesResponse {
    pub(crate) spaces: Vec<ApiSpace>,
}

struct Inner {
    base_url: String,
    token: String,
    space_id: Option<i64>,
    client: reqwest::Client,
    transfer: reqwest::Client,
}

/// HTTP client for the Nuage API. Cloning is cheap: every clone shares the same
/// connection pools.
#[derive(Clone)]
pub struct ApiClient {
    inner: Arc<Inner>,
}

impl ApiClient {
    pub fn new(base_url: &str, token: &str, space_id: Option<i64>) -> Result<Self> {
        let origin = Self::extract_origin(base_url);
        let mut headers = HeaderMap::new();
        if let Ok(val) = HeaderValue::from_str(&origin) {
            headers.insert("origin", val);
        }

        let client = reqwest::Client::builder()
            .default_headers(headers.clone())
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow::anyhow!("failed to build HTTP client: {}", e))?;

        let transfer = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| anyhow::anyhow!("failed to build transfer HTTP client: {}", e))?;

        Ok(Self {
            inner: Arc::new(Inner {
                base_url: base_url.trim_end_matches('/').to_string(),
                token: token.to_string(),
                space_id,
                client,
                transfer,
            }),
        })
    }

    fn base_url(&self) -> &str {
        &self.inner.base_url
    }

    /// Appends the selected space to an endpoint URL.
    ///
    /// The server answers from the personal space when no `space_id` reaches
    /// it, which is why a folder that exists only in a shared space used to be
    /// invisible to every command. Endpoints that belong to the account rather
    /// than to a space — the API tokens — are built without this.
    fn scoped_url(&self, url: String) -> String {
        match self.inner.space_id {
            Some(id) => format!("{url}?space_id={id}"),
            None => url,
        }
    }

    fn token(&self) -> &str {
        &self.inner.token
    }

    /// The space this client is scoped to, when one is.
    pub(crate) fn space_id(&self) -> Option<i64> {
        self.inner.space_id
    }

    fn client(&self) -> &reqwest::Client {
        &self.inner.client
    }

    fn transfer(&self) -> &reqwest::Client {
        &self.inner.transfer
    }

    fn extract_origin(base_url: &str) -> String {
        if let Ok(u) = reqwest::Url::parse(base_url) {
            let port = u.port().map(|p| format!(":{p}")).unwrap_or_default();
            format!("{}://{}{}", u.scheme(), u.host_str().unwrap_or(""), port)
        } else {
            base_url.to_string()
        }
    }
}
