mod edit;
mod rows;

use anyhow::{bail, Result};
use clap::{Args, Subcommand};

use crate::api::{ApiClient, CreateSpaceRequest, UpdateSpaceRequest};
use crate::commands::progress::confirm;
use crate::commands::space::{is_personal, PERSONAL};
use crate::config;
use crate::ui;

use edit::{forget_mapping, rename_mapping, resolve_space_ref};
use rows::{print_space_row, space_json};

#[derive(Subcommand)]
pub enum SpacesCommand {
    /// List spaces available to this account
    List,
    /// Create a new space
    Create(SpacesCreateArgs),
    /// Rename a space
    Rename(SpacesRenameArgs),
    /// Delete a space
    Rm(SpacesRmArgs),
}

#[derive(Args)]
pub struct SpacesCreateArgs {
    /// Name of the new space
    pub name: String,

    /// Description of the new space
    #[arg(short, long)]
    pub description: Option<String>,
}

#[derive(Args)]
pub struct SpacesRenameArgs {
    /// Space name or id
    #[arg(value_name = "NAME_OR_ID")]
    pub name_or_id: String,

    /// New name for the space
    pub new_name: String,
}

#[derive(Args)]
pub struct SpacesRmArgs {
    /// Space name or id
    #[arg(value_name = "NAME_OR_ID")]
    pub name_or_id: String,

    /// Confirm deletion without prompting
    #[arg(short, long)]
    pub yes: bool,
}

pub async fn cmd_spaces(sub: SpacesCommand, json: bool) -> Result<()> {
    match sub {
        SpacesCommand::List => cmd_spaces_list(json).await,
        SpacesCommand::Create(args) => cmd_spaces_create(&args, json).await,
        SpacesCommand::Rename(args) => cmd_spaces_rename(&args, json).await,
        SpacesCommand::Rm(args) => cmd_spaces_rm(&args, json).await,
    }
}

/// The personal tree is listed alongside the real spaces even though the
/// server does not return it, because a target you cannot see is a target
/// you cannot switch back to.
async fn cmd_spaces_list(json: bool) -> Result<()> {
    let config = config::Config::load()?;
    let api = ApiClient::new(&config.server_url, &config.token, None)?;
    let spaces = api.list_spaces().await?;
    let dirs = &config.spaces;

    if json {
        let rows: Vec<serde_json::Value> =
            spaces.iter().map(|s| space_json(s, dirs.get(&s.name))).collect();
        println!("{}", serde_json::json!({ "spaces": rows }));
        return Ok(());
    }

    let personal_dir = dirs.get(PERSONAL);
    print_space_row(
        "-",
        PERSONAL,
        personal_dir.is_some(),
        personal_dir.map(String::as_str).unwrap_or("your own files"),
    );

    for space in &spaces {
        let dir = dirs.get(&space.name);
        print_space_row(
            &space.id.to_string(),
            &space.name,
            dir.is_some(),
            dir.map(String::as_str).unwrap_or(&space.role),
        );
    }

    Ok(())
}

async fn cmd_spaces_create(args: &SpacesCreateArgs, json: bool) -> Result<()> {
    let config = config::Config::load()?;
    let api = ApiClient::new(&config.server_url, &config.token, None)?;
    let req = CreateSpaceRequest {
        name: args.name.clone(),
        description: args.description.clone().unwrap_or_default(),
    };
    let space = api.create_space(&req).await?;

    if json {
        println!("{}", serde_json::to_string(&space)?);
        return Ok(());
    }

    print_space_row(&space.id.to_string(), &space.name, false, &space.description);
    Ok(())
}

async fn cmd_spaces_rename(args: &SpacesRenameArgs, json: bool) -> Result<()> {
    let config = config::Config::load()?;
    let api = ApiClient::new(&config.server_url, &config.token, None)?;
    let (id, old_name) = resolve_space_ref(&api, &args.name_or_id).await?;
    let req = UpdateSpaceRequest {
        name: Some(args.new_name.clone()),
        description: None,
    };
    let space = api.update_space(id, &req).await?;

    if let Some(old) = old_name.as_deref() {
        rename_mapping(old, &args.new_name)?;
    }

    if json {
        println!("{}", serde_json::to_string(&space)?);
        return Ok(());
    }

    print_space_row(&space.id.to_string(), &space.name, false, &space.description);
    Ok(())
}

async fn cmd_spaces_rm(args: &SpacesRmArgs, json: bool) -> Result<()> {
    if is_personal(&args.name_or_id) {
        bail!("`{PERSONAL}` is your own file tree, not a space, so it cannot be deleted");
    }

    let config = config::Config::load()?;
    let api = ApiClient::new(&config.server_url, &config.token, None)?;
    let (id, name) = resolve_space_ref(&api, &args.name_or_id).await?;

    if !args.yes && !json && !confirm(&format!("delete space {id}?"))? {
        ui::step("Cancelled");
        return Ok(());
    }

    api.delete_space(id).await?;

    if let Some(name) = name.as_deref() {
        forget_mapping(name)?;
    }

    if json {
        println!("{}", serde_json::json!({ "deleted": id }));
    } else {
        ui::success(&format!("deleted space {id}"));
    }

    Ok(())
}
