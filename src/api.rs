use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::multipart;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiFile {
    pub id: i64,
    pub name: String,
    pub hash: Option<String>,
    pub size: Option<i64>,
    pub folder_id: Option<i64>,
    pub mime_type: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiFolder {
    pub id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    pub updated_at: String,
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

pub struct ApiClient {
    base_url: String,
    token: String,
    client: reqwest::Client,
}

impl ApiClient {
    pub fn new(base_url: &str, token: &str) -> Self {
        let origin = Self::extract_origin(base_url);
        let mut headers = HeaderMap::new();
        if let Ok(val) = HeaderValue::from_str(&origin) {
            headers.insert("origin", val);
        }

        let client = reqwest::Client::builder()
            .default_headers(headers.clone())
            .timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");

        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.to_string(),
            client,
        }
    }

    fn transfer_client(&self) -> reqwest::Client {
        let origin = Self::extract_origin(&self.base_url);
        let mut headers = HeaderMap::new();
        if let Ok(val) = HeaderValue::from_str(&origin) {
            headers.insert("origin", val);
        }

        reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(300))
            .build()
            .expect("failed to build transfer HTTP client")
    }

    fn extract_origin(base_url: &str) -> String {
        if let Ok(u) = reqwest::Url::parse(base_url) {
            let port = u.port().map(|p| format!(":{p}")).unwrap_or_default();
            format!("{}://{}{}", u.scheme(), u.host_str().unwrap_or(""), port)
        } else {
            base_url.to_string()
        }
    }

    pub async fn sync_state(&self) -> Result<SyncStateResponse> {
        let resp = self
            .client
            .get(format!("{}/sync/state", self.base_url))
            .bearer_auth(&self.token)
            .send()
            .await
            .context("failed to fetch sync state")?;

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
        let resp = self
            .client
            .get(format!("{}/sync/changes", self.base_url))
            .query(&[("since", since)])
            .bearer_auth(&self.token)
            .send()
            .await
            .context("failed to fetch sync changes")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GET /sync/changes failed ({}): {}", status, body);
        }

        resp.json()
            .await
            .context("failed to parse sync changes response")
    }

    pub async fn download_file(&self, id: i64) -> Result<bytes::Bytes> {
        let client = self.transfer_client();
        let resp = client
            .get(format!("{}/files/{}/download", self.base_url, id))
            .bearer_auth(&self.token)
            .send()
            .await
            .with_context(|| format!("failed to download file {}", id))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GET /files/{}/download failed ({}): {}", id, status, body);
        }

        resp.bytes()
            .await
            .with_context(|| format!("failed to read bytes for file {}", id))
    }

    pub async fn upload_file(
        &self,
        name: &str,
        mime: &str,
        folder_id: Option<i64>,
        data: Vec<u8>,
    ) -> Result<ApiFile> {
        let client = self.transfer_client();

        let file_part = multipart::Part::bytes(data)
            .file_name(name.to_string())
            .mime_str(mime)
            .context("invalid mime type")?;

        let mut form = multipart::Form::new().part("file", file_part);

        if let Some(fid) = folder_id {
            form = form.text("folder_id", fid.to_string());
        }

        let resp = client
            .post(format!("{}/files", self.base_url))
            .bearer_auth(&self.token)
            .multipart(form)
            .send()
            .await
            .context("failed to upload file")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("POST /files failed ({}): {}", status, body);
        }

        resp.json()
            .await
            .context("failed to parse upload response")
    }

    pub async fn create_folder(&self, name: &str, parent_id: Option<i64>) -> Result<ApiFolder> {
        let mut body = serde_json::json!({ "name": name });
        if let Some(pid) = parent_id {
            body["parent_id"] = serde_json::json!(pid);
        }

        let resp = self
            .client
            .post(format!("{}/folders", self.base_url))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .context("failed to create folder")?;

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

        let resp = self
            .client
            .put(format!("{}/files/{}", self.base_url, id))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("failed to update file {}", id))?;

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

        let resp = self
            .client
            .put(format!("{}/folders/{}", self.base_url, id))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("failed to update folder {}", id))?;

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
        let resp = self
            .client
            .delete(format!("{}/files/{}", self.base_url, id))
            .bearer_auth(&self.token)
            .send()
            .await
            .with_context(|| format!("failed to delete file {}", id))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("DELETE /files/{} failed ({}): {}", id, status, body);
        }

        Ok(())
    }

    pub async fn delete_folder(&self, id: i64) -> Result<()> {
        let resp = self
            .client
            .delete(format!("{}/folders/{}", self.base_url, id))
            .bearer_auth(&self.token)
            .send()
            .await
            .with_context(|| format!("failed to delete folder {}", id))?;

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
        let resp = self
            .client
            .get(format!("{}/folders", self.base_url))
            .bearer_auth(&self.token)
            .send()
            .await
            .context("failed to list folders")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GET /folders failed ({}): {}", status, body);
        }

        let list: FoldersListResponse = resp.json().await.context("failed to parse folders list")?;
        Ok(list.folders)
    }

    pub async fn get_folder(&self, id: i64) -> Result<FolderDetailResponse> {
        let resp = self
            .client
            .get(format!("{}/folders/{}", self.base_url, id))
            .bearer_auth(&self.token)
            .send()
            .await
            .with_context(|| format!("failed to get folder {}", id))?;

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
        let client = self.transfer_client();
        let resp = client
            .get(format!("{}/files/{}/download", self.base_url, id))
            .bearer_auth(&self.token)
            .send()
            .await
            .with_context(|| format!("failed to download file {}", id))?;

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

        let resp = self
            .client
            .post(format!("{}/shares", self.base_url))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .context("failed to create share")?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("POST /shares failed ({}): {}", status, body_text);
        }

        resp.json().await.context("failed to parse share response")
    }

    pub async fn list_shares(&self) -> Result<Vec<ShareResponse>> {
        let resp = self
            .client
            .get(format!("{}/shares/by-me", self.base_url))
            .bearer_auth(&self.token)
            .send()
            .await
            .context("failed to list shares")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("GET /shares/by-me failed ({}): {}", status, body);
        }

        let list: SharesListResponse = resp.json().await.context("failed to parse shares list")?;
        Ok(list.shares)
    }

    pub async fn delete_share(&self, id: i64) -> Result<()> {
        let resp = self
            .client
            .delete(format!("{}/shares/{}", self.base_url, id))
            .bearer_auth(&self.token)
            .send()
            .await
            .with_context(|| format!("failed to delete share {}", id))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("DELETE /shares/{} failed ({}): {}", id, status, body);
        }

        Ok(())
    }

    pub async fn list_tokens(&self) -> Result<Vec<ApiToken>> {
        let resp = self
            .client
            .get(format!("{}/users/me/api-token", self.base_url))
            .bearer_auth(&self.token)
            .send()
            .await
            .context("failed to list tokens")?;

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
        let resp = self
            .client
            .post(format!("{}/users/me/api-token", self.base_url))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .context("failed to create token")?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("POST /users/me/api-token failed ({}): {}", status, body_text);
        }

        resp.json().await.context("failed to parse token response")
    }

    pub async fn delete_token(&self, id: i64) -> Result<()> {
        let resp = self
            .client
            .delete(format!("{}/users/me/api-token/{}", self.base_url, id))
            .bearer_auth(&self.token)
            .send()
            .await
            .with_context(|| format!("failed to delete token {}", id))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("DELETE /users/me/api-token/{} failed ({}): {}", id, status, body);
        }

        Ok(())
    }
}
