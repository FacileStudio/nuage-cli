use anyhow::{bail, Result};
use clap::Args;

use crate::api::{ApiClient, ApiFile, ApiFolder};
use crate::commands::paths::{
    resolve_folder_id, resolve_parent_and_name, resolve_path, ResolvedPath,
};
use crate::commands::progress::confirm;
use crate::commands::space::load_api;
use crate::ui;

#[derive(Args)]
pub struct MkdirArgs {
    /// Remote folder path to create
    pub path: String,
}

#[derive(Args)]
pub struct MvArgs {
    /// Source remote path
    pub source: String,
    /// Destination remote path (new name or parent folder)
    pub dest: String,
}

#[derive(Args)]
pub struct RmArgs {
    /// Remote path to delete
    pub path: String,
    #[arg(short, long, help = "Skip confirmation")]
    pub force: bool,
}

pub async fn cmd_mkdir(args: &MkdirArgs, json: bool) -> Result<()> {
    let api = load_api()?;

    let (parent_path, folder_name) = resolve_parent_and_name(&args.path);
    if folder_name.is_empty() {
        bail!("folder name cannot be empty");
    }

    let parent_id = resolve_folder_id(&api, parent_path).await?;
    let folder = api.create_folder(folder_name, parent_id).await?;

    if json {
        println!("{}", serde_json::to_string(&folder)?);
    } else {
        ui::success(&format!("Created {}/", args.path.trim_end_matches('/')));
    }

    Ok(())
}

pub async fn cmd_mv(args: &MvArgs, json: bool) -> Result<()> {
    let api = load_api()?;

    let resolved = resolve_path(&api, &args.source).await?;
    let (dest_parent, _) = resolve_parent_and_name(&args.dest);
    let dest_folder_id = resolve_folder_id(&api, dest_parent).await?;

    match resolved {
        ResolvedPath::File(file) => move_file(&api, &file, args, dest_folder_id, json).await,
        ResolvedPath::Folder(folder) => {
            move_folder(&api, &folder, args, dest_folder_id, json).await
        }
        ResolvedPath::Root => bail!("cannot move root"),
    }
}

async fn move_file(
    api: &ApiClient,
    file: &ApiFile,
    args: &MvArgs,
    dest_folder_id: Option<i64>,
    json: bool,
) -> Result<()> {
    let (_, dest_name) = resolve_parent_and_name(&args.dest);
    let new_name = if dest_name.is_empty() {
        None
    } else {
        Some(dest_name)
    };

    let result = api
        .update_file(file.id, new_name, Some(dest_folder_id))
        .await?;

    if json {
        println!("{}", serde_json::to_string(&result)?);
    } else {
        ui::success(&format!("Moved {} → {}", file.name, args.dest));
    }

    Ok(())
}

async fn move_folder(
    api: &ApiClient,
    folder: &ApiFolder,
    args: &MvArgs,
    dest_folder_id: Option<i64>,
    json: bool,
) -> Result<()> {
    let (_, dest_name) = resolve_parent_and_name(&args.dest);
    let new_name = if dest_name.is_empty() {
        None
    } else {
        Some(dest_name)
    };

    let result = api
        .update_folder(folder.id, new_name, Some(dest_folder_id))
        .await?;

    if json {
        println!("{}", serde_json::to_string(&result)?);
    } else {
        ui::success(&format!("Moved {}/ → {}", folder.name, args.dest));
    }

    Ok(())
}

pub async fn cmd_rm(args: &RmArgs, json: bool) -> Result<()> {
    let api = load_api()?;

    match resolve_path(&api, &args.path).await? {
        ResolvedPath::File(file) => rm_file(&api, &file, args.force, json).await,
        ResolvedPath::Folder(folder) => rm_folder(&api, &folder, args.force, json).await,
        ResolvedPath::Root => bail!("cannot delete root"),
    }
}

async fn rm_file(api: &ApiClient, file: &ApiFile, force: bool, json: bool) -> Result<()> {
    if !force && !json && !confirm(&format!("delete {}?", file.name))? {
        ui::step("Cancelled");
        return Ok(());
    }

    api.delete_file(file.id).await?;

    if json {
        println!(
            "{}",
            serde_json::json!({"deleted": true, "name": file.name})
        );
    } else {
        ui::success(&format!("Deleted {}", file.name));
    }

    Ok(())
}

async fn rm_folder(api: &ApiClient, folder: &ApiFolder, force: bool, json: bool) -> Result<()> {
    if !force && !json && !confirm(&format!("delete {}/?", folder.name))? {
        ui::step("Cancelled");
        return Ok(());
    }

    api.delete_folder(folder.id).await?;

    if json {
        println!(
            "{}",
            serde_json::json!({"deleted": true, "name": folder.name})
        );
    } else {
        ui::success(&format!("Deleted {}/", folder.name));
    }

    Ok(())
}
