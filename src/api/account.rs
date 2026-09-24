use super::{ApiClient, ApiSpace, SpacesResponse, SyncChangesResponse, SyncStateResponse};
use anyhow::{Context, Result};

impl ApiClient {
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

    pub async fn test_connection(&self) -> Result<()> {
        self.sync_state().await?;
        Ok(())
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
}
