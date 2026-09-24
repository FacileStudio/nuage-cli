use anyhow::Result;
use clap::Args;

use crate::api::SearchResultItem;
use crate::commands::paths::resolve_folder_id;
use crate::commands::space::load_api;
use crate::sync::transfer;
use crate::ui;

#[derive(Args)]
pub struct SearchArgs {
    /// Search query
    pub query: String,
    #[arg(short = 't', long, help = "Filter by type: file or folder")]
    pub r#type: Option<String>,
    #[arg(short, long, help = "Scope search to a folder path")]
    pub folder: Option<String>,
    #[arg(short, long, default_value = "50", help = "Max results")]
    pub limit: u32,
}

fn print_results(results: &[SearchResultItem]) {
    let max_path = results
        .iter()
        .map(|r| r.path.len())
        .max()
        .unwrap_or(20)
        .min(60);

    for r in results {
        let kind = if r.kind == "folder" { "dir " } else { "file" };
        let size_str = if r.kind == "folder" {
            "   <dir>".to_string()
        } else {
            format!("{:>8}", transfer::format_size(r.size.unwrap_or(0) as u64))
        };
        let date = &r.updated_at[..10.min(r.updated_at.len())];
        let display_path = if r.kind == "folder" {
            format!("{}/", r.path)
        } else {
            r.path.clone()
        };
        println!(
            "{}  {}  {}  {:<width$}",
            kind,
            size_str,
            date,
            display_path,
            width = max_path
        );
    }
}

pub async fn cmd_search(args: &SearchArgs, json: bool) -> Result<()> {
    let api = load_api()?;

    let folder_id = match &args.folder {
        Some(path) => resolve_folder_id(&api, path).await?,
        None => None,
    };

    let results = api
        .search(&args.query, args.r#type.as_deref(), folder_id, args.limit)
        .await?;

    if json {
        println!("{}", serde_json::to_string(&results)?);
        return Ok(());
    }

    if results.is_empty() {
        ui::step("No results");
        return Ok(());
    }

    print_results(&results);
    Ok(())
}
