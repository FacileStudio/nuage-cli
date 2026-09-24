use anyhow::{bail, Context, Result};
use clap::Args;
use futures_util::StreamExt;
use indicatif::ProgressBar;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::api::{ApiClient, ApiFile};
use crate::commands::paths::{resolve_path, ResolvedPath};
use crate::commands::progress::{make_progress_bar, show_progress};
use crate::commands::space::load_api;
use crate::sync::transfer;

#[derive(Args)]
pub struct DownloadArgs {
    /// Remote file path
    pub remote_path: String,
    /// Local destination (file or directory)
    #[arg(default_value = ".")]
    pub local_dest: String,
}

async fn resolve_file(api: &ApiClient, path: &str) -> Result<ApiFile> {
    match resolve_path(api, path).await? {
        ResolvedPath::File(f) => Ok(f),
        ResolvedPath::Folder(_) => bail!("{} is a folder, not a file", path),
        ResolvedPath::Root => bail!("cannot download root"),
    }
}

fn local_target(dest: &str, name: &str) -> PathBuf {
    let path = PathBuf::from(dest);
    if path.is_dir() {
        path.join(name)
    } else {
        path
    }
}

async fn stream_into(
    out: &mut std::fs::File,
    resp: reqwest::Response,
    pb: Option<&ProgressBar>,
) -> Result<u64> {
    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("download stream error")?;
        out.write_all(&chunk)?;
        downloaded += chunk.len() as u64;
        if let Some(pb) = pb {
            pb.inc(chunk.len() as u64);
        }
    }

    Ok(downloaded)
}

async fn fetch_to_file(
    api: &ApiClient,
    file: &ApiFile,
    local_path: &Path,
    json: bool,
) -> Result<u64> {
    let resp = api.download_file_stream(file.id).await?;
    let total = resp
        .content_length()
        .unwrap_or(file.size.unwrap_or(0) as u64);

    let pb = if show_progress(json) && total > 1024 * 100 {
        Some(make_progress_bar(total))
    } else {
        None
    };

    if let Some(parent) = local_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp_path = transfer::temp_path_for(local_path, file.id);
    let mut out = std::fs::File::create(&tmp_path)
        .with_context(|| format!("cannot create: {}", tmp_path.display()))?;

    let downloaded = stream_into(&mut out, resp, pb.as_ref()).await?;

    drop(out);
    std::fs::rename(&tmp_path, local_path)?;

    if let Some(pb) = pb {
        pb.finish_and_clear();
    }

    Ok(downloaded)
}

fn report_download(name: &str, downloaded: u64, local_path: &Path, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "name": name,
                "size": downloaded,
                "path": local_path.to_string_lossy(),
            }))?
        );
    } else {
        println!(
            "downloaded {} ({}) -> {}",
            name,
            transfer::format_size(downloaded),
            local_path.display()
        );
    }
    Ok(())
}

pub async fn cmd_download(args: &DownloadArgs, json: bool) -> Result<()> {
    let api = load_api()?;

    let file = resolve_file(&api, &args.remote_path).await?;
    let local_path = local_target(&args.local_dest, &file.name);
    let downloaded = fetch_to_file(&api, &file, &local_path, json).await?;

    report_download(&file.name, downloaded, &local_path, json)
}
