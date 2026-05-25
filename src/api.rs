use anyhow::{Context, Result};
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

pub struct ApiClient {
    base_url: String,
    token: String,
    client: reqwest::Client,
}

impl ApiClient {
    pub fn new(base_url: &str, token: &str) -> Self {
        let client = reqwest::Client::builder()
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
        reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .expect("failed to build transfer HTTP client")
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
}
