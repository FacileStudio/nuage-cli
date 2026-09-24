use super::{ApiClient, ShareResponse, SharesListResponse};
use anyhow::{Context, Result};

impl ApiClient {
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
}
