use super::{
    ApiClient, ApiFile, ApiFolder, FolderDetailResponse, FoldersListResponse,
    SearchApiResponse, SearchResultItem,
};
use anyhow::{Context, Result};

impl ApiClient {
    /// Creates a folder, in the client's space when one is selected.
    ///
    /// `POST /folders` reads `space_id` from the body and ignores the query
    /// string, unlike the file endpoints. Sending only the query prefix creates
    /// the folder in the caller's personal space while the rest of the run is
    /// scoped elsewhere, and every upload that then names that folder is
    /// refused with `parent folder not found`.
    pub async fn create_folder(&self, name: &str, parent_id: Option<i64>) -> Result<ApiFolder> {
        let mut body = serde_json::json!({ "name": name });
        if let Some(pid) = parent_id {
            body["parent_id"] = serde_json::json!(pid);
        }
        if let Some(sid) = self.space_id() {
            body["space_id"] = serde_json::json!(sid);
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

    pub async fn update_file(
        &self,
        id: i64,
        name: Option<&str>,
        folder_id: Option<Option<i64>>,
    ) -> Result<ApiFile> {
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

        let list: FoldersListResponse =
            resp.json().await.context("failed to parse folders list")?;
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

        let api_resp: SearchApiResponse = resp
            .json()
            .await
            .context("failed to parse search response")?;
        Ok(api_resp.results)
    }
}
