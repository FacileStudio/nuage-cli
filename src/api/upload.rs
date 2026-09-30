use super::{ApiClient, ApiFile};
use anyhow::{Context, Result};
use reqwest::multipart;
use std::path::Path;
use tokio_util::io::ReaderStream;

/// Adds the two scope fields a multipart file request carries.
///
/// Both are read from the body, not from the query string: a request scoped only
/// by `?space_id=` writes into the caller's personal space — silently, at the
/// space's root — and is refused with `folder not found` when the folder belongs
/// to a space.
fn scope_form(
    mut form: multipart::Form,
    folder_id: Option<i64>,
    space_id: Option<i64>,
) -> multipart::Form {
    if let Some(fid) = folder_id {
        form = form.text("folder_id", fid.to_string());
    }
    if let Some(sid) = space_id {
        form = form.text("space_id", sid.to_string());
    }
    form
}

fn build_upload_form(
    data: Vec<u8>,
    name: &str,
    mime: &str,
    folder_id: Option<i64>,
    space_id: Option<i64>,
) -> reqwest::Result<multipart::Form> {
    let file_part = multipart::Part::bytes(data)
        .file_name(name.to_string())
        .mime_str(mime)?;
    Ok(scope_form(
        multipart::Form::new().part("file", file_part),
        folder_id,
        space_id,
    ))
}

/// Builds a form whose file part streams from disk, so a reupload never reads
/// the file into memory. The handle is opened here and consumed once, so a
/// failed request is retried by rebuilding the form rather than by the client.
fn build_stream_form(path: &Path, name: &str, mime: &str) -> Result<multipart::Form> {
    let size = std::fs::metadata(path)
        .with_context(|| format!("cannot stat file for upload: {}", path.display()))?
        .len();

    let file = std::fs::File::open(path)
        .with_context(|| format!("cannot open file for upload: {}", path.display()))?;
    let stream = ReaderStream::new(tokio::fs::File::from_std(file));
    let part = multipart::Part::stream_with_length(reqwest::Body::wrap_stream(stream), size)
        .file_name(name.to_string())
        .mime_str(mime)?;

    Ok(multipart::Form::new().part("file", part))
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

        let space_id = self.space_id();
        let resp = self
            .send_with_retry("failed to upload file", || {
                let data = data.clone();
                let name = name.to_string();
                let mime = mime.to_string();
                let url = url.clone();
                async move {
                    let form = build_upload_form(data, &name, &mime, folder_id, space_id)?;
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
    ///
    /// The body is a stream over the file, so the caller's memory holds none of
    /// it. That also means the request cannot be rebuilt for a retry the way a
    /// buffered body can: a failure is returned to the sync pass, which attempts
    /// the file again on its next run.
    pub async fn reupload_file(
        &self,
        id: i64,
        name: &str,
        mime: &str,
        path: &Path,
    ) -> Result<ApiFile> {
        let client = self.transfer();
        let url = self.scoped_url(format!("{}/files/{}/reupload", self.base_url(), id));
        let token = self.token();

        let form = build_stream_form(path, name, mime)?;
        let resp = client
            .post(&url)
            .bearer_auth(token)
            .multipart(form)
            .send()
            .await
            .with_context(|| format!("failed to reupload file {}", id))?;

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
