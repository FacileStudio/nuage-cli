use anyhow::Result;
use clap::{Args, Subcommand};

use crate::api::ApiClient;
use crate::commands::space::load_api;
use crate::ui;

#[derive(Subcommand)]
pub enum TokenCommand {
    /// Create a new API token
    Create(TokenCreateArgs),
    /// List API tokens
    List,
    /// Revoke an API token
    Revoke(TokenRevokeArgs),
}

#[derive(Args)]
pub struct TokenCreateArgs {
    /// Token name
    #[arg(short, long)]
    pub name: String,
}

#[derive(Args)]
pub struct TokenRevokeArgs {
    /// Token ID to revoke
    pub id: i64,
}

async fn create_token(api: &ApiClient, args: &TokenCreateArgs, json: bool) -> Result<()> {
    let token = api.create_token(&args.name).await?;

    if json {
        println!("{}", serde_json::to_string(&token)?);
        return Ok(());
    }

    println!("id:    {}", token.id);
    println!("name:  {}", token.name);
    if let Some(ref val) = token.token {
        println!("token: {}", val);
        println!("\nsave this token -- it won't be shown again.");
    }
    Ok(())
}

async fn list_tokens(api: &ApiClient, json: bool) -> Result<()> {
    let tokens = api.list_tokens().await?;

    if json {
        println!("{}", serde_json::to_string(&tokens)?);
    } else if tokens.is_empty() {
        ui::step("No API tokens");
    } else {
        for t in &tokens {
            println!("#{:<4}  {}  created {}", t.id, t.name, &t.created_at[..10]);
        }
    }

    Ok(())
}

async fn revoke_token(api: &ApiClient, id: i64, json: bool) -> Result<()> {
    api.delete_token(id).await?;

    if json {
        println!("{}", serde_json::json!({"deleted": true, "id": id}));
    } else {
        ui::success(&format!("Token {} revoked", id));
    }

    Ok(())
}

pub async fn cmd_token(sub: TokenCommand, json: bool) -> Result<()> {
    let api = load_api()?;

    match sub {
        TokenCommand::Create(args) => create_token(&api, &args, json).await,
        TokenCommand::List => list_tokens(&api, json).await,
        TokenCommand::Revoke(args) => revoke_token(&api, args.id, json).await,
    }
}
