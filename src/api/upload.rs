use super::{ApiClient, ApiFile};
use anyhow::{Context, Result};
use reqwest::multipart;

fn build_upload_form(
    data: Vec<u8>,
    name: &str,
    mime: &str,
    folder_id: Option<i64>,
) -> reqwest::Result<multipart::Form> {
    let file_part = multipart::Part::bytes(data)
        .file_name(name.to_string())
        .mime_str(mime)?;
    let mut form = multipart::Form::new().part("file", file_part);
    if let Some(fid) = folder_id {
        form = form.text("folder_id", fid.to_string());
    }
    Ok(form)
}

impl ApiClient {
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
                    let form = build_upload_form(data, &name, &mime, folder_id)?;
                    client
                        .post(&url)
                        .bearer_auth(token)
                        .multipart(form)
                        .send()
                        .await
                }
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("POST /files failed ({}): {}", status, body);
        }

        resp.json().await.context("failed to parse upload response")
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
                    let form = build_upload_form(data, &name, &mime, None)?;
                    client
                        .post(&url)
                        .bearer_auth(token)
                        .multipart(form)
                        .send()
                        .await
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
}
