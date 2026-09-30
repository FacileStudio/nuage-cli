use super::{ApiClient, ApiFile, UploadCompleteResponse, UploadSession, UPLOAD_CHUNK_SIZE};
use anyhow::{Context, Result};
use tokio::io::AsyncReadExt;

async fn read_full(file: &mut tokio::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0usize;
    while filled < buf.len() {
        let n = file.read(&mut buf[filled..]).await?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}

impl ApiClient {
    /// Uploads `path` through the chunked session endpoints (init, parts,
    /// complete), reading the file from disk in 32 MiB chunks.
    ///
    /// If any step after init fails, the upload session is aborted before the
    /// original error is returned.
    pub async fn upload_file_chunked(
        &self,
        name: &str,
        mime: &str,
        folder_id: Option<i64>,
        path: &std::path::Path,
    ) -> Result<ApiFile> {
        let total_size = tokio::fs::metadata(path)
            .await
            .with_context(|| format!("cannot stat file for upload: {}", path.display()))?
            .len() as i64;

        let session = self.upload_init(name, mime, total_size, folder_id).await?;

        match self
            .upload_parts_and_complete(&session.session_id, path, None)
            .await
        {
            Ok(file) => Ok(file),
            Err(err) => {
                let _ = self.upload_abort(&session.session_id).await;
                Err(err)
            }
        }
    }

    /// Writes `path` into an existing file as its next version, through the
    /// chunked endpoints so the request body is never bounded by the single
    /// request limit.
    ///
    /// The file keeps its id, name, folder, share links and version history. A
    /// server that does not know the `file_id` target answers by creating a
    /// second file instead, which the caller recognises by the id that comes
    /// back.
    pub async fn version_file_chunked(
        &self,
        file_id: i64,
        name: &str,
        mime: &str,
        path: &std::path::Path,
    ) -> Result<ApiFile> {
        let total_size = tokio::fs::metadata(path)
            .await
            .with_context(|| format!("cannot stat file for upload: {}", path.display()))?
            .len() as i64;

        let session = self.upload_init(name, mime, total_size, None).await?;

        match self
            .upload_parts_and_complete(&session.session_id, path, Some(file_id))
            .await
        {
            Ok(file) => Ok(file),
            Err(err) => {
                let _ = self.upload_abort(&session.session_id).await;
                Err(err)
            }
        }
    }

    async fn upload_init(
        &self,
        name: &str,
        mime: &str,
        total_size: i64,
        folder_id: Option<i64>,
    ) -> Result<UploadSession> {
        let client = self.client();
        let url = self.scoped_url(format!("{}/files/upload/init", self.base_url()));
        let token = self.token();
        let mut body = serde_json::json!({
            "file_name": name,
            "mime_type": mime,
            "total_size": total_size,
            "folder_id": folder_id,
        });
        if let Some(sid) = self.space_id() {
            body["space_id"] = serde_json::json!(sid);
        }

        let resp = self
            .send_with_retry("failed to init chunked upload", || {
                client.post(&url).bearer_auth(token).json(&body).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            anyhow::bail!("POST /files/upload/init failed ({}): {}", status, body_text);
        }

        resp.json()
            .await
            .context("failed to parse upload init response")
    }

    async fn upload_parts_and_complete(
        &self,
        session_id: &str,
        path: &std::path::Path,
        target_file_id: Option<i64>,
    ) -> Result<ApiFile> {
        let mut file = tokio::fs::File::open(path)
            .await
            .with_context(|| format!("cannot open file for upload: {}", path.display()))?;

        let mut buf = vec![0u8; UPLOAD_CHUNK_SIZE];
        let mut part_number: u32 = 1;

        loop {
            let filled = read_full(&mut file, &mut buf)
                .await
                .with_context(|| format!("cannot read file for upload: {}", path.display()))?;

            if filled == 0 {
                break;
            }

            self.upload_part(session_id, part_number, &buf[..filled])
                .await?;
            part_number += 1;

            if filled < UPLOAD_CHUNK_SIZE {
                break;
            }
        }

        self.upload_complete(session_id, target_file_id).await
    }

    async fn upload_part(&self, session_id: &str, part_number: u32, chunk: &[u8]) -> Result<()> {
        let client = self.transfer();
        let url = self.scoped_url(format!(
            "{}/files/upload/{}/part/{}",
            self.base_url(),
            session_id,
            part_number
        ));
        let token = self.token();

        let resp = self
            .send_with_retry("failed to upload chunk", || {
                let body = chunk.to_vec();
                client.put(&url).bearer_auth(token).body(body).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "PUT /files/upload/{}/part/{} failed ({}): {}",
                session_id,
                part_number,
                status,
                body
            );
        }

        Ok(())
    }

    async fn upload_complete(
        &self,
        session_id: &str,
        target_file_id: Option<i64>,
    ) -> Result<ApiFile> {
        let client = self.client();
        let url = self.scoped_url(format!(
            "{}/files/upload/{}/complete",
            self.base_url(),
            session_id
        ));
        let token = self.token();

        let body = target_file_id.map(|id| serde_json::json!({ "file_id": id }));
        let resp = self
            .send_with_retry("failed to complete chunked upload", || match body {
                Some(ref body) => client.post(&url).bearer_auth(token).json(body).send(),
                None => client.post(&url).bearer_auth(token).send(),
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "POST /files/upload/{}/complete failed ({}): {}",
                session_id,
                status,
                body
            );
        }

        let complete: UploadCompleteResponse = resp
            .json()
            .await
            .context("failed to parse upload complete response")?;

        Ok(complete.file)
    }

    async fn upload_abort(&self, session_id: &str) -> Result<()> {
        let client = self.client();
        let url = self.scoped_url(format!("{}/files/upload/{}", self.base_url(), session_id));
        let token = self.token();

        let resp = self
            .send_with_retry("failed to abort chunked upload", || {
                client.delete(&url).bearer_auth(token).send()
            })
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "DELETE /files/upload/{} failed ({}): {}",
                session_id,
                status,
                body
            );
        }

        Ok(())
    }
}
