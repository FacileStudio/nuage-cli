use super::{ApiClient, ApiKey, CreateKeyRequest, CreateKeyResponse, ListKeysResponse};
use anyhow::{Context, Result};

impl ApiClient {
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

        resp.json()
            .await
            .context("failed to parse create key response")
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

#[cfg(test)]
mod tests {
    use crate::api::{ApiClient, CreateKeyRequest};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn json_response(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
    }

    async fn spawn_stub(check: impl Fn(&str) + Send + 'static, response: String) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 2048];
            let n = socket.read(&mut buf).await.unwrap();
            check(&String::from_utf8_lossy(&buf[..n]));
            socket.write_all(response.as_bytes()).await.unwrap();
        });

        port
    }

    #[tokio::test]
    async fn test_list_keys() {
        let body = r#"{"keys":[{"id":1,"app":"web","kind":"secret","prefix":"nuage_sec_","allowed_origins":[],"daily_quota":0,"created_at":"2026-09-01T12:00:00Z"}]}"#;
        let port = spawn_stub(
            |req| assert!(req.starts_with("GET /apikeys?app=web ")),
            json_response("200 OK", body),
        )
        .await;

        let client = ApiClient::new(&format!("http://127.0.0.1:{port}"), "dummy", None).unwrap();
        let keys = client.list_keys(Some("web")).await.unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].id, 1);
        assert_eq!(keys[0].app, "web");
        assert_eq!(keys[0].kind, "secret");
    }

    #[tokio::test]
    async fn test_create_key() {
        let body = r#"{"key":{"id":2,"app":"studio","kind":"public","prefix":"nuage_pub_","allowed_origins":["app.example.com"],"daily_quota":1000,"created_at":"2026-09-01T12:00:00Z"},"token":"nuage_pub_secretvalue"}"#;
        let port = spawn_stub(
            |req| {
                assert!(req.starts_with("POST /apikeys "));
                assert!(req.contains(r#""app":"studio""#));
                assert!(req.contains(r#""kind":"public""#));
            },
            json_response("201 Created", body),
        )
        .await;

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
        let port = spawn_stub(
            |req| assert!(req.starts_with("DELETE /apikeys/42 ")),
            json_response("200 OK", ""),
        )
        .await;

        let client = ApiClient::new(&format!("http://127.0.0.1:{port}"), "dummy", None).unwrap();
        let result = client.revoke_key(42).await;
        assert!(result.is_ok());
    }
}
