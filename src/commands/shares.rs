use anyhow::{bail, Context, Result};
use clap::Args;

use crate::api::ShareResponse;
use crate::commands::paths::{resolve_path, ResolvedPath};
use crate::commands::space::load_api;
use crate::config;
use crate::ui;

#[derive(Args)]
pub struct ShareArgs {
    /// Remote path to share
    pub path: String,
    #[arg(short, long, default_value = "view", help = "Permission: view or edit")]
    pub permission: String,
    #[arg(short, long, help = "Expiration (RFC3339 or duration like 7d, 24h)")]
    pub expires: Option<String>,
}

#[derive(Args)]
pub struct UnshareArgs {
    /// Share ID to revoke
    pub id: i64,
}

fn share_target(resolved: &ResolvedPath) -> Result<(Option<i64>, Option<i64>)> {
    match resolved {
        ResolvedPath::File(f) => Ok((Some(f.id), None)),
        ResolvedPath::Folder(f) => Ok((None, Some(f.id))),
        ResolvedPath::Root => bail!("cannot share root"),
    }
}

pub fn parse_expiry(input: &str) -> Result<String> {
    let trimmed = input.trim();

    if trimmed.contains('T') || trimmed.contains('-') {
        return Ok(trimmed.to_string());
    }

    let (num_str, unit) = trimmed.split_at(trimmed.len() - 1);
    let num: u64 = num_str.parse().context("invalid duration number")?;

    let seconds = match unit {
        "m" => num * 60,
        "h" => num * 3600,
        "d" => num * 86400,
        "w" => num * 604800,
        _ => bail!("unknown duration unit: {} (use m, h, d, or w)", unit),
    };

    let expires = chrono::Utc::now() + chrono::Duration::seconds(seconds as i64);
    Ok(expires.to_rfc3339())
}

pub async fn cmd_share(args: &ShareArgs, json: bool) -> Result<()> {
    let api = load_api()?;
    let config = config::Config::load()?;

    let resolved = resolve_path(&api, &args.path).await?;
    let (file_id, folder_id) = share_target(&resolved)?;

    let expires_at = args.expires.as_deref().map(parse_expiry).transpose()?;
    let share = api
        .create_share(file_id, folder_id, &args.permission, expires_at.as_deref())
        .await?;

    if json {
        println!("{}", serde_json::to_string(&share)?);
    } else {
        let url = format!(
            "{}/s/{}",
            config.server_url.trim_end_matches('/'),
            share.token
        );
        println!("{}", url);
        if let Some(ref exp) = share.expires_at {
            println!("expires: {}", exp);
        }
    }

    Ok(())
}

pub async fn cmd_unshare(args: &UnshareArgs, json: bool) -> Result<()> {
    let api = load_api()?;
    api.delete_share(args.id).await?;

    if json {
        println!("{}", serde_json::json!({"deleted": true, "id": args.id}));
    } else {
        ui::success(&format!("Share {} revoked", args.id));
    }

    Ok(())
}

fn print_share(share: &ShareResponse) {
    let target = if share.file_id.is_some() {
        "file"
    } else {
        "folder"
    };
    let target_id = share.file_id.or(share.folder_id).unwrap_or(0);
    let exp = share.expires_at.as_deref().unwrap_or("never");
    println!(
        "#{:<4}  {}  {} {}  perm={}  expires={}",
        share.id, share.token, target, target_id, share.permission, exp
    );
}

pub async fn cmd_shares(json: bool) -> Result<()> {
    let api = load_api()?;
    let shares = api.list_shares().await?;

    if json {
        println!("{}", serde_json::to_string(&shares)?);
        return Ok(());
    }

    if shares.is_empty() {
        ui::step("No active shares");
        return Ok(());
    }

    for share in &shares {
        print_share(share);
    }

    Ok(())
}
