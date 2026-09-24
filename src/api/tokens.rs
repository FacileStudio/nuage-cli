use super::{ApiClient, ApiToken, TokensListResponse};
use anyhow::{Context, Result};

impl ApiClient {
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
            anyhow::bail!(
                "POST /users/me/api-token failed ({}): {}",
                status,
                body_text
            );
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
            anyhow::bail!(
                "DELETE /users/me/api-token/{} failed ({}): {}",
                id,
                status,
                body
            );
        }

        Ok(())
    }
}
