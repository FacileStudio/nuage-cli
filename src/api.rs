use anyhow::{Context, Result};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::multipart;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MAX_ATTEMPTS: u32 = 4;
const BASE_DELAY_MS: u64 = 500;
const MAX_DELAY_MS: u64 = 8_000;
const MAX_RETRY_AFTER_SECS: u64 = 60;
const UPLOAD_CHUNK_SIZE: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiFile {
    pub id: i64,
    pub name: String,
    pub hash: Option<String>,
    pub size: Option<i64>,
    pub folder_id: Option<i64>,
    #[serde(default)]
    pub space_id: Option<i64>,
    pub mime_type: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiFolder {
    pub id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub space_id: Option<i64>,
    pub updated_at: String,
}

/// A space the signed-in user belongs to.
///
/// `role` is the caller's role in it, not the space's own attribute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiSpace {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub role: String,
}

#[derive(Debug, Clone, Deserialize)]
struct SpacesResponse {
    spaces: Vec<ApiSpace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStateResponse {
    pub files: Vec<ApiFile>,
    pub folders: Vec<ApiFolder>,
    pub server_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncChangesResponse {
    pub files: FileChanges,
    pub folders: FolderChanges,
    pub server_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChanges {
    pub changed: Vec<ApiFile>,
    pub deleted: Vec<DeletedItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderChanges {
    pub changed: Vec<ApiFolder>,
    pub deleted: Vec<DeletedItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletedItem {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoldersListResponse {
    pub folders: Vec<ApiFolder>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderDetailResponse {
    pub folder: ApiFolder,
    pub files: Vec<ApiFile>,
    pub folders: Vec<ApiFolder>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareResponse {
    pub id: i64,
    pub token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_id: Option<i64>,
    pub permission: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharesListResponse {
    pub shares: Vec<ShareResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiToken {
    pub id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    pub name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokensListResponse {
    pub tokens: Vec<ApiToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: i64,
    pub app: String,
    pub kind: String,
    pub prefix: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_origins: Vec<String>,
    #[serde(default)]
    pub daily_quota: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_today: Option<i64>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateKeyRequest {
    pub app: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_origins: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_quota: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateKeyResponse {
    pub key: ApiKey,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListKeysResponse {
    pub keys: Vec<ApiKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultItem {
    pub id: i64,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub path: String,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub size: Option<i64>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchApiResponse {
    pub results: Vec<SearchResultItem>,
    pub total: i32,
}

/// Response of `POST /files/upload/init`.
#[derive(Debug, Clone, Deserialize)]
pub struct UploadSession {
    pub session_id: String,
}

/// Response of `POST /files/upload/{sessionId}/complete`, which nests the
/// created file under a `file` key.
#[derive(Debug, Clone, Deserialize)]
pub struct UploadCompleteResponse {
    pub file: ApiFile,
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

fn is_retryable_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request()
}

fn is_retryable_anyhow(err: &anyhow::Error) -> bool {
    err.downcast_ref::<reqwest::Error>()
        .map(is_retryable_error)
        .unwrap_or(false)
}

fn is_retryable_status(status: reqwest::StatusCode) -> bool {
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

fn backoff_delay(attempt: u32) -> Duration {
    let factor = 1u64.checked_shl(attempt.saturating_sub(1)).unwrap_or(u64::MAX);
    let base = BASE_DELAY_MS.saturating_mul(factor).min(MAX_DELAY_MS);
    let total = base.saturating_add(jitter_ms(base / 2 + 1)).min(MAX_DELAY_MS);
    Duration::from_millis(total)
}

fn retry_after_delay(resp: &reqwest::Response) -> Option<Duration> {
    let raw = resp.headers().get(reqwest::header::RETRY_AFTER)?;
    let secs = raw.to_str().ok()?.trim().parse::<u64>().ok()?;
    Some(Duration::from_secs(secs.min(MAX_RETRY_AFTER_SECS)))
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

    async fn send_with_retry<F, Fut>(&self, what: &str, build: F) -> Result<reqwest::Response>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = reqwest::Result<reqwest::Response>>,
    {
        let mut attempt: u32 = 1;
        loop {
            match build().await {
                Ok(resp) => {
                    if attempt < MAX_ATTEMPTS && is_retryable_status(resp.status()) {
                        let delay = retry_after_delay(&resp).unwrap_or_else(|| backoff_delay(attempt));
                        tokio::time::sleep(delay).await;
                        attempt += 1;
                        continue;
                    }
                    return Ok(resp);
                }
                Err(err) => {
                    if attempt < MAX_ATTEMPTS && is_retryable_error(&err) {
                        tokio::time::sleep(backoff_delay(attempt)).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(anyhow::Error::new(err).context(what.to_string()));
                }
            }
        }
    }

    pub async fn sync_state(&self) -> Result<SyncStateResponse> {
        let client = self.client();
        let url = self.scoped_url(format!("{}/sync/state", self.base_url()));
        let token = self.token();

        let resp = self
            .send_with_retry("failed to fetch sync state", || {
                client.get(&url).bearer_auth(token).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GET /sync/state failed ({}): {}", status, body);
        }

        resp.json()
            .await
            .context("failed to parse sync state response")
    }

    pub async fn sync_changes(&self, since: &str) -> Result<SyncChangesResponse> {
        let client = self.client();
        let url = self.scoped_url(format!("{}/sync/changes", self.base_url()));
        let token = self.token();

        let resp = self
            .send_with_retry("failed to fetch sync changes", || {
                client
                    .get(&url)
                    .query(&[("since", since)])
                    .bearer_auth(token)
                    .send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GET /sync/changes failed ({}): {}", status, body);
        }

        resp.json()
            .await
            .context("failed to parse sync changes response")
    }

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
        let client = self.transfer();
        let url = self.scoped_url(format!("{}/files/{}/download", self.base_url(), id));
        let token = self.token();

        let mut attempt: u32 = 1;
        loop {
            let sent = client.get(&url).bearer_auth(token).send().await;

            let resp = match sent {
                Ok(resp) => resp,
                Err(err) => {
                    if attempt < MAX_ATTEMPTS && is_retryable_error(&err) {
                        tokio::time::sleep(backoff_delay(attempt)).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(anyhow::Error::new(err)
                        .context(format!("failed to download file {}", id)));
                }
            };

            let status = resp.status();
            if !status.is_success() {
                if attempt < MAX_ATTEMPTS && is_retryable_status(status) {
                    let delay = retry_after_delay(&resp).unwrap_or_else(|| backoff_delay(attempt));
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                    continue;
                }
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("GET /files/{}/download failed ({}): {}", id, status, body);
            }

            match stream_response_to_file(resp, id, dest, expected_hash).await {
                Ok(()) => return Ok(()),
                Err(err) => {
                    if attempt < MAX_ATTEMPTS && is_retryable_anyhow(&err) {
                        tokio::time::sleep(backoff_delay(attempt)).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
            }
        }
    }

    pub async fn upload_file(
        &self,
        name: &str,
        mime: &str,
        folder_id: Option<i64>,
        data: Vec<u8>,
    ) -> Result<ApiFile> {
        let client = self.transfer();
        let url = self.scoped_url(format!("{}/files", self.base_url()));
        let token = self.token();

        let resp = self
            .send_with_retry("failed to upload file", || {
                let data = data.clone();
                let name = name.to_string();
                let mime = mime.to_string();
                let url = url.clone();
                async move {
                    let file_part = multipart::Part::bytes(data)
                        .file_name(name)
                        .mime_str(&mime)?;
                    let mut form = multipart::Form::new().part("file", file_part);
                    if let Some(fid) = folder_id {
                        form = form.text("folder_id", fid.to_string());
                    }
                    client.post(&url).bearer_auth(token).multipart(form).send().await
                }
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("POST /files failed ({}): {}", status, body);
        }

        resp.json()
            .await
            .context("failed to parse upload response")
    }

    /// Replaces the content of an existing file via `POST /files/{id}/reupload`,
    /// preserving its id, share links and version history.
    pub async fn reupload_file(
        &self,
        id: i64,
        name: &str,
        mime: &str,
        data: Vec<u8>,
    ) -> Result<ApiFile> {
        let client = self.transfer();
        let url = self.scoped_url(format!("{}/files/{}/reupload", self.base_url(), id));
        let token = self.token();

        let resp = self
            .send_with_retry("failed to reupload file", || {
                let data = data.clone();
                let name = name.to_string();
                let mime = mime.to_string();
                let url = url.clone();
                async move {
                    let file_part = multipart::Part::bytes(data)
                        .file_name(name)
                        .mime_str(&mime)?;
                    let form = multipart::Form::new().part("file", file_part);
                    client.post(&url).bearer_auth(token).multipart(form).send().await
                }
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("POST /files/{}/reupload failed ({}): {}", id, status, body);
        }

        resp.json()
            .await
            .with_context(|| format!("failed to parse reupload response for file {}", id))
    }

    /// Uploads `path` through the chunked session endpoints (init, parts,
    /// complete), reading the file from disk in 32 MiB chunks.
    ///
    /// If any step after init fails, the upload session is aborted before the
    /// original error is returned.
    pub async fn upload_file_chunked(
        &self,
        name: &str,
        mime: &str,
        folder_id: Option<i64>,
        path: &std::path::Path,
    ) -> Result<ApiFile> {
        let total_size = tokio::fs::metadata(path)
            .await
            .with_context(|| format!("cannot stat file for upload: {}", path.display()))?
            .len() as i64;

        let session = self.upload_init(name, mime, total_size, folder_id).await?;

        match self.upload_parts_and_complete(&session.session_id, path).await {
            Ok(file) => Ok(file),
            Err(err) => {
                let _ = self.upload_abort(&session.session_id).await;
                Err(err)
            }
        }
    }

    async fn upload_init(
        &self,
        name: &str,
        mime: &str,
        total_size: i64,
        folder_id: Option<i64>,
    ) -> Result<UploadSession> {
        let client = self.client();
        let url = self.scoped_url(format!("{}/files/upload/init", self.base_url()));
        let token = self.token();
        let body = serde_json::json!({
            "file_name": name,
            "mime_type": mime,
            "total_size": total_size,
            "folder_id": folder_id,
        });

        let resp = self
            .send_with_retry("failed to init chunked upload", || {
                client.post(&url).bearer_auth(token).json(&body).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("POST /files/upload/init failed ({}): {}", status, body_text);
        }

        resp.json()
            .await
            .context("failed to parse upload init response")
    }

    async fn upload_parts_and_complete(
        &self,
        session_id: &str,
        path: &std::path::Path,
    ) -> Result<ApiFile> {
        let mut file = tokio::fs::File::open(path)
            .await
            .with_context(|| format!("cannot open file for upload: {}", path.display()))?;

        let mut buf = vec![0u8; UPLOAD_CHUNK_SIZE];
        let mut part_number: u32 = 1;

        loop {
            let filled = read_full(&mut file, &mut buf)
                .await
                .with_context(|| format!("cannot read file for upload: {}", path.display()))?;

            if filled == 0 {
                break;
            }

            self.upload_part(session_id, part_number, &buf[..filled]).await?;
            part_number += 1;

            if filled < UPLOAD_CHUNK_SIZE {
                break;
            }
        }

        self.upload_complete(session_id).await
    }

    async fn upload_part(&self, session_id: &str, part_number: u32, chunk: &[u8]) -> Result<()> {
        let client = self.transfer();
        let url = self.scoped_url(format!(
            "{}/files/upload/{}/part/{}",
            self.base_url(),
            session_id,
            part_number
        ));
        let token = self.token();

        let resp = self
            .send_with_retry("failed to upload chunk", || {
                let body = chunk.to_vec();
                client.put(&url).bearer_auth(token).body(body).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "PUT /files/upload/{}/part/{} failed ({}): {}",
                session_id,
                part_number,
                status,
                body
            );
        }

        Ok(())
    }

    async fn upload_complete(&self, session_id: &str) -> Result<ApiFile> {
        let client = self.client();
        let url = self.scoped_url(format!("{}/files/upload/{}/complete", self.base_url(), session_id));
        let token = self.token();

        let resp = self
            .send_with_retry("failed to complete chunked upload", || {
                client.post(&url).bearer_auth(token).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "POST /files/upload/{}/complete failed ({}): {}",
                session_id,
                status,
                body
            );
        }

        let complete: UploadCompleteResponse = resp
            .json()
            .await
            .context("failed to parse upload complete response")?;

        Ok(complete.file)
    }

    async fn upload_abort(&self, session_id: &str) -> Result<()> {
        let client = self.client();
        let url = self.scoped_url(format!("{}/files/upload/{}", self.base_url(), session_id));
        let token = self.token();

        let resp = self
            .send_with_retry("failed to abort chunked upload", || {
                client.delete(&url).bearer_auth(token).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "DELETE /files/upload/{} failed ({}): {}",
                session_id,
                status,
                body
            );
        }

        Ok(())
    }

    pub async fn create_folder(&self, name: &str, parent_id: Option<i64>) -> Result<ApiFolder> {
        let mut body = serde_json::json!({ "name": name });
        if let Some(pid) = parent_id {
            body["parent_id"] = serde_json::json!(pid);
        }

        let client = self.client();
        let url = self.scoped_url(format!("{}/folders", self.base_url()));
        let token = self.token();

        let resp = self
            .send_with_retry("failed to create folder", || {
                client.post(&url).bearer_auth(token).json(&body).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("POST /folders failed ({}): {}", status, body_text);
        }

        resp.json()
            .await
            .context("failed to parse create folder response")
    }

    pub async fn update_file(&self, id: i64, name: Option<&str>, folder_id: Option<Option<i64>>) -> Result<ApiFile> {
        let mut body = serde_json::Map::new();
        if let Some(n) = name {
            body.insert("name".into(), serde_json::json!(n));
        }
        if let Some(fid) = folder_id {
            body.insert("folder_id".into(), serde_json::json!(fid.unwrap_or(0)));
        }

        let client = self.client();
        let url = self.scoped_url(format!("{}/files/{}", self.base_url(), id));
        let token = self.token();

        let resp = self
            .send_with_retry(&format!("failed to update file {}", id), || {
                client.put(&url).bearer_auth(token).json(&body).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("PUT /files/{} failed ({}): {}", id, status, body_text);
        }

        resp.json()
            .await
            .with_context(|| format!("failed to parse update file {} response", id))
    }

    pub async fn update_folder(&self, id: i64, name: Option<&str>, parent_id: Option<Option<i64>>) -> Result<ApiFolder> {
        let mut body = serde_json::Map::new();
        if let Some(n) = name {
            body.insert("name".into(), serde_json::json!(n));
        }
        if let Some(pid) = parent_id {
            body.insert("parent_id".into(), serde_json::json!(pid.unwrap_or(0)));
        }

        let client = self.client();
        let url = self.scoped_url(format!("{}/folders/{}", self.base_url(), id));
        let token = self.token();

        let resp = self
            .send_with_retry(&format!("failed to update folder {}", id), || {
                client.put(&url).bearer_auth(token).json(&body).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("PUT /folders/{} failed ({}): {}", id, status, body_text);
        }

        resp.json()
            .await
            .with_context(|| format!("failed to parse update folder {} response", id))
    }

    pub async fn delete_file(&self, id: i64) -> Result<()> {
        let client = self.client();
        let url = self.scoped_url(format!("{}/files/{}", self.base_url(), id));
        let token = self.token();

        let resp = self
            .send_with_retry(&format!("failed to delete file {}", id), || {
                client.delete(&url).bearer_auth(token).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("DELETE /files/{} failed ({}): {}", id, status, body);
        }

        Ok(())
    }

    pub async fn delete_folder(&self, id: i64) -> Result<()> {
        let client = self.client();
        let url = self.scoped_url(format!("{}/folders/{}", self.base_url(), id));
        let token = self.token();

        let resp = self
            .send_with_retry(&format!("failed to delete folder {}", id), || {
                client.delete(&url).bearer_auth(token).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("DELETE /folders/{} failed ({}): {}", id, status, body);
        }

        Ok(())
    }

    pub async fn test_connection(&self) -> Result<()> {
        self.sync_state().await?;
        Ok(())
    }

    pub async fn list_folders(&self) -> Result<Vec<ApiFolder>> {
        let client = self.client();
        let url = self.scoped_url(format!("{}/folders", self.base_url()));
        let token = self.token();

        let resp = self
            .send_with_retry("failed to list folders", || {
                client.get(&url).bearer_auth(token).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GET /folders failed ({}): {}", status, body);
        }

        let list: FoldersListResponse = resp.json().await.context("failed to parse folders list")?;
        Ok(list.folders)
    }

    pub async fn get_folder(&self, id: i64) -> Result<FolderDetailResponse> {
        let client = self.client();
        let url = self.scoped_url(format!("{}/folders/{}", self.base_url(), id));
        let token = self.token();

        let resp = self
            .send_with_retry(&format!("failed to get folder {}", id), || {
                client.get(&url).bearer_auth(token).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GET /folders/{} failed ({}): {}", id, status, body);
        }

        resp.json()
            .await
            .with_context(|| format!("failed to parse folder {} response", id))
    }

    pub async fn download_file_stream(&self, id: i64) -> Result<reqwest::Response> {
        let client = self.transfer();
        let url = self.scoped_url(format!("{}/files/{}/download", self.base_url(), id));
        let token = self.token();

        let resp = self
            .send_with_retry(&format!("failed to download file {}", id), || {
                client.get(&url).bearer_auth(token).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GET /files/{}/download failed ({}): {}", id, status, body);
        }

        Ok(resp)
    }

    pub async fn create_share(
        &self,
        file_id: Option<i64>,
        folder_id: Option<i64>,
        permission: &str,
        expires_at: Option<&str>,
    ) -> Result<ShareResponse> {
        let mut body = serde_json::Map::new();
        if let Some(fid) = file_id {
            body.insert("file_id".into(), serde_json::json!(fid));
        }
        if let Some(fid) = folder_id {
            body.insert("folder_id".into(), serde_json::json!(fid));
        }
        body.insert("permission".into(), serde_json::json!(permission));
        if let Some(exp) = expires_at {
            body.insert("expires_at".into(), serde_json::json!(exp));
        }

        let client = self.client();
        let url = self.scoped_url(format!("{}/shares", self.base_url()));
        let token = self.token();

        let resp = self
            .send_with_retry("failed to create share", || {
                client.post(&url).bearer_auth(token).json(&body).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("POST /shares failed ({}): {}", status, body_text);
        }

        resp.json().await.context("failed to parse share response")
    }

    /// Lists the spaces the signed-in user belongs to.
    ///
    /// Deliberately unscoped: it is the command that tells you which space to
    /// select, so scoping it to a selection would be circular.
    pub async fn list_spaces(&self) -> Result<Vec<ApiSpace>> {
        let client = self.client();
        let url = format!("{}/spaces", self.base_url());
        let token = self.token();

        let resp = self
            .send_with_retry("failed to list spaces", || {
                client.get(&url).bearer_auth(token).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GET /spaces failed ({}): {}", status, body);
        }

        let parsed: SpacesResponse = resp
            .json()
            .await
            .context("failed to parse spaces response")?;
        Ok(parsed.spaces)
    }

    pub async fn list_shares(&self) -> Result<Vec<ShareResponse>> {
        let client = self.client();
        let url = self.scoped_url(format!("{}/shares/by-me", self.base_url()));
        let token = self.token();

        let resp = self
            .send_with_retry("failed to list shares", || {
                client.get(&url).bearer_auth(token).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GET /shares/by-me failed ({}): {}", status, body);
        }

        let list: SharesListResponse = resp.json().await.context("failed to parse shares list")?;
        Ok(list.shares)
    }

    pub async fn delete_share(&self, id: i64) -> Result<()> {
        let client = self.client();
        let url = self.scoped_url(format!("{}/shares/{}", self.base_url(), id));
        let token = self.token();

        let resp = self
            .send_with_retry(&format!("failed to delete share {}", id), || {
                client.delete(&url).bearer_auth(token).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("DELETE /shares/{} failed ({}): {}", id, status, body);
        }

        Ok(())
    }

    pub async fn search(
        &self,
        query: &str,
        filter_type: Option<&str>,
        folder_id: Option<i64>,
        limit: u32,
    ) -> Result<Vec<SearchResultItem>> {
        let mut params = vec![
            ("q".to_string(), query.to_string()),
            ("limit".to_string(), limit.to_string()),
        ];
        if let Some(t) = filter_type {
            params.push(("type".to_string(), t.to_string()));
        }
        if let Some(fid) = folder_id {
            params.push(("folder_id".to_string(), fid.to_string()));
        }

        let client = self.client();
        let url = self.scoped_url(format!("{}/search", self.base_url()));
        let token = self.token();

        let resp = self
            .send_with_retry("failed to search", || {
                client.get(&url).query(&params).bearer_auth(token).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GET /search failed ({}): {}", status, body);
        }

        let api_resp: SearchApiResponse = resp.json().await.context("failed to parse search response")?;
        Ok(api_resp.results)
    }

    pub async fn list_tokens(&self) -> Result<Vec<ApiToken>> {
        let client = self.client();
        let url = format!("{}/users/me/api-token", self.base_url());
        let token = self.token();

        let resp = self
            .send_with_retry("failed to list tokens", || {
                client.get(&url).bearer_auth(token).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GET /users/me/api-token failed ({}): {}", status, body);
        }

        let list: TokensListResponse = resp.json().await.context("failed to parse tokens list")?;
        Ok(list.tokens)
    }

    pub async fn create_token(&self, name: &str) -> Result<ApiToken> {
        let body = serde_json::json!({ "name": name });
        let client = self.client();
        let url = format!("{}/users/me/api-token", self.base_url());
        let token = self.token();

        let resp = self
            .send_with_retry("failed to create token", || {
                client.post(&url).bearer_auth(token).json(&body).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("POST /users/me/api-token failed ({}): {}", status, body_text);
        }

        resp.json().await.context("failed to parse token response")
    }

    pub async fn delete_token(&self, id: i64) -> Result<()> {
        let client = self.client();
        let url = format!("{}/users/me/api-token/{}", self.base_url(), id);
        let token = self.token();

        let resp = self
            .send_with_retry(&format!("failed to delete token {}", id), || {
                client.delete(&url).bearer_auth(token).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("DELETE /users/me/api-token/{} failed ({}): {}", id, status, body);
        }

        Ok(())
    }

    pub async fn list_keys(&self, app: Option<&str>) -> Result<Vec<ApiKey>> {
        let client = self.client();
        let base = format!("{}/apikeys", self.base_url());
        let url = match app {
            Some(a) if !a.is_empty() => format!("{base}?app={a}"),
            _ => base,
        };
        let token = self.token();

        let resp = self
            .send_with_retry("failed to list keys", || {
                client.get(&url).bearer_auth(token).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GET /apikeys failed ({}): {}", status, body);
        }

        let list: ListKeysResponse = resp.json().await.context("failed to parse keys list")?;
        Ok(list.keys)
    }

    pub async fn create_key(&self, req: &CreateKeyRequest) -> Result<CreateKeyResponse> {
        let client = self.client();
        let url = format!("{}/apikeys", self.base_url());
        let token = self.token();

        let resp = self
            .send_with_retry("failed to create key", || {
                client.post(&url).bearer_auth(token).json(req).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("POST /apikeys failed ({}): {}", status, body_text);
        }

        resp.json().await.context("failed to parse create key response")
    }

    pub async fn revoke_key(&self, id: i64) -> Result<()> {
        let client = self.client();
        let url = format!("{}/apikeys/{}", self.base_url(), id);
        let token = self.token();

        let resp = self
            .send_with_retry(&format!("failed to revoke key {}", id), || {
                client.delete(&url).bearer_auth(token).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("DELETE /apikeys/{} failed ({}): {}", id, status, body);
        }

        Ok(())
    }
}

async fn read_full(file: &mut tokio::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0usize;
    while filled < buf.len() {
        let n = file.read(&mut buf[filled..]).await?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}

async fn stream_response_to_file(
    resp: reqwest::Response,
    id: i64,
    dest: &Path,
    expected_hash: Option<&str>,
) -> Result<()> {
    let mut out = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("cannot create file: {}", dest.display()))?;

    let mut hasher = Sha256::new();
    let mut stream = resp.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(err) => {
                let _ = tokio::fs::remove_file(dest).await;
                return Err(anyhow::Error::new(err)
                    .context(format!("failed to read download stream for file {}", id)));
            }
        };

        if let Err(err) = out.write_all(&chunk).await {
            let _ = tokio::fs::remove_file(dest).await;
            return Err(anyhow::Error::new(err)
                .context(format!("cannot write file: {}", dest.display())));
        }

        hasher.update(&chunk);
    }

    if let Err(err) = out.flush().await {
        let _ = tokio::fs::remove_file(dest).await;
        return Err(anyhow::Error::new(err)
            .context(format!("cannot flush file: {}", dest.display())));
    }
    drop(out);

    if let Some(expected) = expected_hash {
        let computed = format!("{:x}", hasher.finalize());
        if computed != expected {
            let _ = tokio::fs::remove_file(dest).await;
            anyhow::bail!(
                "integrity check failed for file {}: expected {}, got {}",
                id,
                expected,
                computed
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_list_keys() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let n = socket.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);
            assert!(req.starts_with("GET /apikeys?app=web "));
            let body = r#"{"keys":[{"id":1,"app":"web","kind":"secret","prefix":"nuage_sec_","allowed_origins":[],"daily_quota":0,"created_at":"2026-09-01T12:00:00Z"}]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(resp.as_bytes()).await.unwrap();
        });

        let client = ApiClient::new(&format!("http://127.0.0.1:{port}"), "dummy", None).unwrap();
        let keys = client.list_keys(Some("web")).await.unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].id, 1);
        assert_eq!(keys[0].app, "web");
        assert_eq!(keys[0].kind, "secret");
    }

    #[tokio::test]
    async fn test_create_key() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 2048];
            let n = socket.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);
            assert!(req.starts_with("POST /apikeys "));
            assert!(req.contains(r#""app":"studio""#));
            assert!(req.contains(r#""kind":"public""#));
            let body = r#"{"key":{"id":2,"app":"studio","kind":"public","prefix":"nuage_pub_","allowed_origins":["app.example.com"],"daily_quota":1000,"created_at":"2026-09-01T12:00:00Z"},"token":"nuage_pub_secretvalue"}"#;
            let resp = format!(
                "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(resp.as_bytes()).await.unwrap();
        });

        let client = ApiClient::new(&format!("http://127.0.0.1:{port}"), "dummy", None).unwrap();
        let resp = client
            .create_key(&CreateKeyRequest {
                app: "studio".to_string(),
                kind: "public".to_string(),
                allowed_origins: vec!["app.example.com".to_string()],
                daily_quota: Some(1000),
            })
            .await
            .unwrap();
        assert_eq!(resp.key.id, 2);
        assert_eq!(resp.key.app, "studio");
        assert_eq!(resp.token, "nuage_pub_secretvalue");
    }

    #[tokio::test]
    async fn test_revoke_key() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let n = socket.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);
            assert!(req.starts_with("DELETE /apikeys/42 "));
            let resp = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
            socket.write_all(resp.as_bytes()).await.unwrap();
        });

        let client = ApiClient::new(&format!("http://127.0.0.1:{port}"), "dummy", None).unwrap();
        let result = client.revoke_key(42).await;
        assert!(result.is_ok());
    }
}
