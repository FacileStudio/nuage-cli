use anyhow::{bail, Context, Result};
use clap::Args;
use std::io::{self, IsTerminal, Read};
use std::path::Path;

use crate::api::ApiFile;
use crate::commands::paths::{resolve_folder_id, resolve_parent_and_name};
use crate::commands::space::load_api;
use crate::sync::transfer;
use crate::ui;

#[derive(Args)]
pub struct UploadArgs {
    /// Local file path, or "-" to read from stdin
    pub source: String,
    /// Remote destination path
    #[arg(default_value = "/")]
    pub dest: String,
}

fn read_source(source: &str) -> Result<Vec<u8>> {
    if source == "-" {
        if io::stdin().is_terminal() {
            bail!("stdin is a terminal -- pipe data or use a file path");
        }
        let mut buf = Vec::new();
        io::stdin().read_to_end(&mut buf)?;
        return Ok(buf);
    }

    let path = Path::new(source);
    if !path.exists() {
        bail!("file not found: {}", source);
    }
    std::fs::read(path).with_context(|| format!("cannot read: {}", source))
}

fn remote_name(source: &str, dest_name: &str) -> Result<String> {
    if !dest_name.is_empty() && dest_name != "/" {
        return Ok(dest_name.to_string());
    }

    if source == "-" {
        return Ok("stdin".to_string());
    }

    Ok(Path::new(source)
        .file_name()
        .context("source has no filename")?
        .to_string_lossy()
        .to_string())
}

fn source_mime(source: &str) -> String {
    if source == "-" {
        "application/octet-stream".to_string()
    } else {
        transfer::mime_from_extension(Path::new(source))
    }
}

fn report_upload(result: &ApiFile, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string(result)?);
    } else {
        let size = result
            .size
            .map(|s| transfer::format_size(s as u64))
            .unwrap_or_default();
        ui::success(&format!("Uploaded {} ({})", result.name, size));
    }
    Ok(())
}

pub async fn cmd_upload(args: &UploadArgs, json: bool) -> Result<()> {
    let api = load_api()?;

    let (parent_path, dest_name) = resolve_parent_and_name(&args.dest);
    let folder_id = resolve_folder_id(&api, parent_path).await?;

    let data = read_source(&args.source)?;
    let file_name = remote_name(&args.source, dest_name)?;
    let mime = source_mime(&args.source);

    let result = api.upload_file(&file_name, &mime, folder_id, data).await?;

    report_upload(&result, json)
}
