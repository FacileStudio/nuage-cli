use super::{
    ApiClient, ApiSpace, CreateSpaceRequest, DeleteSpaceResponse, SpacesResponse,
    SyncChangesResponse, SyncStateResponse, UpdateSpaceRequest,
};
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

    /// Creates a space. Unscoped, like `list_spaces`: a space is an account-level
    /// object, not something selected inside another space.
    pub async fn create_space(&self, req: &CreateSpaceRequest) -> Result<ApiSpace> {
        let client = self.client();
        let url = format!("{}/spaces", self.base_url());
        let token = self.token();

        let resp = self
            .send_with_retry("failed to create space", || {
                client.post(&url).bearer_auth(token).json(req).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("POST /spaces failed ({}): {}", status, body);
        }

        resp.json()
            .await
            .context("failed to parse space response")
    }

    /// Renames a space. Unscoped for the same reason as `create_space`.
    pub async fn update_space(&self, id: i64, req: &UpdateSpaceRequest) -> Result<ApiSpace> {
        let client = self.client();
        let url = format!("{}/spaces/{}", self.base_url(), id);
        let token = self.token();

        let resp = self
            .send_with_retry(&format!("failed to update space {}", id), || {
                client.put(&url).bearer_auth(token).json(req).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("PUT /spaces/{} failed ({}): {}", id, status, body);
        }

        resp.json()
            .await
            .context("failed to parse space response")
    }

    /// Deletes a space. Unscoped for the same reason as `create_space`.
    pub async fn delete_space(&self, id: i64) -> Result<()> {
        let client = self.client();
        let url = format!("{}/spaces/{}", self.base_url(), id);
        let token = self.token();

        let resp = self
            .send_with_retry(&format!("failed to delete space {}", id), || {
                client.delete(&url).bearer_auth(token).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("DELETE /spaces/{} failed ({}): {}", id, status, body);
        }

        let parsed: DeleteSpaceResponse = resp
            .json()
            .await
            .context("failed to parse delete space response")?;
        if !parsed.deleted {
            anyhow::bail!("DELETE /spaces/{} did not confirm the deletion", id);
        }
        Ok(())
    }
}
