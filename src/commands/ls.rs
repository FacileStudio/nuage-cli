use anyhow::Result;
use clap::Args;
use serde::Serialize;

use crate::api::{ApiClient, ApiFile, ApiFolder};
use crate::commands::paths::{resolve_path, ResolvedPath};
use crate::commands::space::load_api;
use crate::sync::transfer;

#[derive(Args)]
pub struct LsArgs {
    #[arg(default_value = "/")]
    pub path: String,
    #[arg(short, long, help = "Show sizes and dates")]
    pub long: bool,
}

#[derive(Serialize)]
pub struct LsEntry {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mime_type: Option<String>,
    updated_at: String,
}

fn folder_entry(folder: &ApiFolder) -> LsEntry {
    LsEntry {
        name: folder.name.clone(),
        kind: "folder".into(),
        id: folder.id,
        size: None,
        mime_type: None,
        updated_at: folder.updated_at.clone(),
    }
}

fn file_entry(file: &ApiFile) -> LsEntry {
    LsEntry {
        name: file.name.clone(),
        kind: "file".into(),
        id: file.id,
        size: file.size,
        mime_type: file.mime_type.clone(),
        updated_at: file.updated_at.clone(),
    }
}

async fn collect_entries(api: &ApiClient, path: &str) -> Result<Vec<LsEntry>> {
    let mut entries: Vec<LsEntry> = Vec::new();

    match resolve_path(api, path).await? {
        ResolvedPath::Root => {
            let state = api.sync_state().await?;
            for f in state.folders.iter().filter(|f| f.parent_id.is_none()) {
                entries.push(folder_entry(f));
            }
            for f in state.files.iter().filter(|f| f.folder_id.is_none()) {
                entries.push(file_entry(f));
            }
        }
        ResolvedPath::Folder(folder) => {
            let detail = api.get_folder(folder.id).await?;
            for f in &detail.folders {
                entries.push(folder_entry(f));
            }
            for f in &detail.files {
                entries.push(file_entry(f));
            }
        }
        ResolvedPath::File(file) => entries.push(file_entry(&file)),
    }

    Ok(entries)
}

fn sort_entries(entries: &mut [LsEntry]) {
    entries.sort_by(|a, b| {
        let a_ord = if a.kind == "folder" { 0 } else { 1 };
        let b_ord = if b.kind == "folder" { 0 } else { 1 };
        a_ord.cmp(&b_ord).then(a.name.cmp(&b.name))
    });
}

fn print_entry(entry: &LsEntry, long: bool) {
    if !long {
        if entry.kind == "folder" {
            println!("{}/", entry.name);
        } else {
            println!("{}", entry.name);
        }
        return;
    }

    let size_str = if entry.kind == "folder" {
        "   <dir>".to_string()
    } else {
        format!(
            "{:>8}",
            entry
                .size
                .map(|s| transfer::format_size(s as u64))
                .unwrap_or_else(|| "--".into())
        )
    };
    let date = &entry.updated_at[..10];
    let display_name = if entry.kind == "folder" {
        format!("{}/", entry.name)
    } else {
        entry.name.clone()
    };
    println!("{}  {}  {}", size_str, date, display_name);
}

pub async fn cmd_ls(args: &LsArgs, json: bool) -> Result<()> {
    let api = load_api()?;
    let mut entries = collect_entries(&api, &args.path).await?;

    if json {
        println!("{}", serde_json::to_string(&entries)?);
        return Ok(());
    }

    sort_entries(&mut entries);
    for entry in &entries {
        print_entry(entry, args.long);
    }

    Ok(())
}
