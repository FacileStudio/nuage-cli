use anyhow::Result;
use clap::{Args, Subcommand};

use crate::api::{ApiClient, ApiKey, CreateKeyRequest};
use crate::commands::space::load_api;
use crate::ui;

#[derive(Subcommand)]
pub enum KeysCommand {
    /// List API keys
    List(KeysListArgs),
    /// Create a new API key
    Create(KeysCreateArgs),
    /// Revoke an API key
    Revoke(KeysRevokeArgs),
}

#[derive(Args)]
pub struct KeysListArgs {
    /// Filter keys by application name
    #[arg(short, long)]
    pub app: Option<String>,
}

#[derive(Args)]
pub struct KeysCreateArgs {
    /// Application name
    #[arg(short, long)]
    pub app: String,

    /// Create a public browser key instead of a secret key
    #[arg(long)]
    pub public: bool,

    /// Comma-separated allowed origins (for public keys)
    #[arg(long)]
    pub origins: Option<String>,

    /// Daily event quota limit (for public keys)
    #[arg(long)]
    pub quota: Option<i64>,
}

#[derive(Args)]
pub struct KeysRevokeArgs {
    /// Key ID to revoke
    pub id: i64,

    /// Confirm revocation without prompting
    #[arg(short, long)]
    pub yes: bool,
}

fn print_key_header() {
    println!(
        "{:<6} {:<16} {:<8} {:<12} {:<8} {:<24} CREATED",
        "ID", "APP", "KIND", "PREFIX", "STATUS", "QUOTA"
    );
}

fn print_key_row(k: &ApiKey) {
    let status = if k.revoked_at.is_some() {
        "revoked"
    } else {
        "active"
    };
    let quota = if k.daily_quota > 0 {
        format!("{}/day ({} used)", k.daily_quota, k.used_today.unwrap_or(0))
    } else {
        "unlimited".to_string()
    };
    let created = if k.created_at.len() >= 10 {
        &k.created_at[..10]
    } else {
        &k.created_at
    };
    println!(
        "#{:<5} {:<16} {:<8} {:<12} {:<8} {:<24} {}",
        k.id, k.app, k.kind, k.prefix, status, quota, created
    );
}

async fn keys_list(api: &ApiClient, args: &KeysListArgs, json: bool) -> Result<()> {
    let mut keys = api.list_keys(args.app.as_deref()).await?;
    if let Some(ref a) = args.app {
        keys.retain(|k| &k.app == a);
    }

    if json {
        println!("{}", serde_json::to_string(&keys)?);
        return Ok(());
    }

    if keys.is_empty() {
        ui::step("no API keys found");
        return Ok(());
    }

    print_key_header();
    for k in &keys {
        print_key_row(k);
    }
    Ok(())
}

fn parse_origins(origins: Option<&str>) -> Vec<String> {
    origins
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

async fn keys_create(api: &ApiClient, args: &KeysCreateArgs, json: bool) -> Result<()> {
    let kind = if args.public {
        "public".to_string()
    } else {
        "secret".to_string()
    };

    let req = CreateKeyRequest {
        app: args.app.clone(),
        kind,
        allowed_origins: parse_origins(args.origins.as_deref()),
        daily_quota: args.quota,
    };
    let resp = api.create_key(&req).await?;

    if json {
        println!("{}", serde_json::to_string(&resp)?);
        return Ok(());
    }

    ui::success(&format!(
        "created {} key for {} (id: {})",
        resp.key.kind, resp.key.app, resp.key.id
    ));
    println!("{}", resp.token);
    ui::hint("store this token securely; it will not be shown again");
    Ok(())
}

async fn keys_revoke(api: &ApiClient, id: i64, json: bool) -> Result<()> {
    api.revoke_key(id).await?;

    if json {
        println!("{}", serde_json::json!({ "revoked": id }));
    } else {
        ui::success(&format!("revoked key {}", id));
    }

    Ok(())
}

pub async fn cmd_keys(sub: KeysCommand, json: bool) -> Result<()> {
    let api = load_api()?;

    match sub {
        KeysCommand::List(args) => keys_list(&api, &args, json).await,
        KeysCommand::Create(args) => keys_create(&api, &args, json).await,
        KeysCommand::Revoke(args) => keys_revoke(&api, args.id, json).await,
    }
}
