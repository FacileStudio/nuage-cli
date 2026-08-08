mod api;
mod ui;
mod config;
mod daemon;
mod hash;
mod ignore;
mod sync;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use tracing::{error, info, warn};

use api::{ApiClient, ApiFile, ApiFolder};
use sync::transfer;

#[derive(Parser)]
#[command(
    name = "nuage",
    version,
    about = "File sync daemon and client for Nuage"
)]
struct Cli {
    #[arg(long, global = true, help = "Output as JSON")]
    json: bool,

    #[arg(long, global = true, help = "Disable colored output")]
    no_color: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start background daemon
    Start,
    /// Stop background daemon
    Stop,
    /// Restart background daemon
    Restart,
    /// Run one-time sync
    Sync(SyncArgs),
    /// Start foreground watcher (for debugging)
    Watch,
    /// Show sync and daemon status
    Status,
    /// Show daemon logs
    Logs(LogsArgs),
    /// Interactive login setup
    Login,
    /// Upgrade nuage-cli
    Upgrade,
    /// List files and folders at a remote path
    Ls(LsArgs),
    /// Upload a file to the server
    Upload(UploadArgs),
    /// Download a file from the server
    Download(DownloadArgs),
    /// Create a remote folder
    Mkdir(MkdirArgs),
    /// Move or rename a file/folder
    Mv(MvArgs),
    /// Delete a file or folder
    Rm(RmArgs),
    /// Create a share link
    Share(ShareArgs),
    /// Revoke a share link
    Unshare(UnshareArgs),
    /// List your shares
    Shares,
    /// Search files and folders
    Search(SearchArgs),
    /// Manage API tokens
    #[command(subcommand)]
    Token(TokenCommand),
}

#[derive(clap::Args)]
struct LogsArgs {
    #[arg(short, long)]
    follow: bool,
}

#[derive(clap::Args)]
struct SyncArgs {
    #[arg(long, help = "Show what would change without applying anything")]
    dry_run: bool,
    #[arg(
        long,
        help = "Allow propagating an unusually large batch of local deletions"
    )]
    allow_bulk_delete: bool,
    #[arg(long, help = "Clear quarantined files and retry them")]
    retry_failed: bool,
    #[arg(
        long,
        help = "Drop tracking records whose local file is gone, without deleting anything on the server, then re-enumerate"
    )]
    repair_state: bool,
}

#[derive(clap::Args)]
struct LsArgs {
    #[arg(default_value = "/")]
    path: String,
    #[arg(short, long, help = "Show sizes and dates")]
    long: bool,
}

#[derive(clap::Args)]
struct UploadArgs {
    /// Local file path, or "-" to read from stdin
    source: String,
    /// Remote destination path
    #[arg(default_value = "/")]
    dest: String,
}

#[derive(clap::Args)]
struct DownloadArgs {
    /// Remote file path
    remote_path: String,
    /// Local destination (file or directory)
    #[arg(default_value = ".")]
    local_dest: String,
}

#[derive(clap::Args)]
struct MkdirArgs {
    /// Remote folder path to create
    path: String,
}

#[derive(clap::Args)]
struct MvArgs {
    /// Source remote path
    source: String,
    /// Destination remote path (new name or parent folder)
    dest: String,
}

#[derive(clap::Args)]
struct RmArgs {
    /// Remote path to delete
    path: String,
    #[arg(short, long, help = "Skip confirmation")]
    force: bool,
}

#[derive(clap::Args)]
struct ShareArgs {
    /// Remote path to share
    path: String,
    #[arg(short, long, default_value = "view", help = "Permission: view or edit")]
    permission: String,
    #[arg(short, long, help = "Expiration (RFC3339 or duration like 7d, 24h)")]
    expires: Option<String>,
}

#[derive(clap::Args)]
struct UnshareArgs {
    /// Share ID to revoke
    id: i64,
}

#[derive(clap::Args)]
struct SearchArgs {
    /// Search query
    query: String,
    #[arg(short = 't', long, help = "Filter by type: file or folder")]
    r#type: Option<String>,
    #[arg(short, long, help = "Scope search to a folder path")]
    folder: Option<String>,
    #[arg(short, long, default_value = "50", help = "Max results")]
    limit: u32,
}

#[derive(Subcommand)]
enum TokenCommand {
    /// Create a new API token
    Create(TokenCreateArgs),
    /// List API tokens
    List,
    /// Revoke an API token
    Revoke(TokenRevokeArgs),
}

#[derive(clap::Args)]
struct TokenCreateArgs {
    /// Token name
    #[arg(short, long)]
    name: String,
}

#[derive(clap::Args)]
struct TokenRevokeArgs {
    /// Token ID to revoke
    id: i64,
}

#[derive(Serialize)]
struct LsEntry {
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

fn main() {
    let cli = Cli::parse();
    if cli.no_color || cli.json {
        ui::disable_color();
    }

    if let Err(e) = run(cli) {
        ui::error(&format!("{e:#}"));
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {

    match cli.command {
        Some(Command::Start) => cmd_start(),
        Some(Command::Stop) => cmd_stop(),
        Some(Command::Restart) => cmd_restart(),
        Some(Command::Logs(args)) => cmd_logs(args.follow),
        other => {
            daemon::init_terminal_logging();

            let rt = tokio::runtime::Runtime::new()
                .context("failed to create async runtime")?;

            rt.block_on(async {
                match other {
                    None | Some(Command::Watch) => cmd_watch().await,
                    Some(Command::Sync(args)) => cmd_sync(&args).await,
                    Some(Command::Status) => cmd_status().await,
                    Some(Command::Login) => cmd_login().await,
                    Some(Command::Upgrade) => cmd_upgrade().await,
                    Some(Command::Ls(args)) => cmd_ls(&args, cli.json).await,
                    Some(Command::Upload(args)) => cmd_upload(&args, cli.json).await,
                    Some(Command::Download(args)) => cmd_download(&args, cli.json).await,
                    Some(Command::Mkdir(args)) => cmd_mkdir(&args, cli.json).await,
                    Some(Command::Mv(args)) => cmd_mv(&args, cli.json).await,
                    Some(Command::Rm(args)) => cmd_rm(&args, cli.json).await,
                    Some(Command::Share(args)) => cmd_share(&args, cli.json).await,
                    Some(Command::Unshare(args)) => cmd_unshare(&args, cli.json).await,
                    Some(Command::Shares) => cmd_shares(cli.json).await,
                    Some(Command::Search(args)) => cmd_search(&args, cli.json).await,
                    Some(Command::Token(sub)) => cmd_token(sub, cli.json).await,
                    _ => unreachable!(),
                }
            })
        }
    }
}

fn load_api() -> Result<ApiClient> {
    let config = config::Config::load()?;
    ApiClient::new(&config.server_url, &config.token)
}

fn build_engine() -> Result<sync::SyncEngine> {
    let config = config::Config::load()?;
    let sync_dir = config.sync_dir_expanded()?;

    std::fs::create_dir_all(&sync_dir)
        .with_context(|| format!("cannot create sync directory: {}", sync_dir.display()))?;

    let api_client = api::ApiClient::new(&config.server_url, &config.token)?;
    let state = sync::state::SyncState::new(&sync_dir)?;
    let ignore = ignore::IgnoreRules::new(config.ignore_patterns.clone());

    sync::SyncEngine::new(config, api_client, state, ignore)
}

fn show_progress(json: bool) -> bool {
    !json && io::stderr().is_terminal()
}

fn make_progress_bar(total: u64) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::with_template("{spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );
    pb
}

fn parse_expiry(input: &str) -> Result<String> {
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

// --- Path resolution ---

enum ResolvedPath {
    Root,
    Folder(ApiFolder),
    File(ApiFile),
}

async fn resolve_path(api: &ApiClient, path: &str) -> Result<ResolvedPath> {
    let parts: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    if parts.is_empty() {
        return Ok(ResolvedPath::Root);
    }

    let root_folders = api.list_folders().await?;

    let first = parts[0];
    let root_match = root_folders.iter().find(|f| f.name == first);

    if parts.len() == 1 {
        if let Some(folder) = root_match {
            return Ok(ResolvedPath::Folder(folder.clone()));
        }
        let state = api.sync_state().await?;
        if let Some(file) = state.files.iter().find(|f| f.name == first && f.folder_id.is_none()) {
            return Ok(ResolvedPath::File(file.clone()));
        }
        bail!("not found: {}", path);
    }

    let root_folder = root_match.ok_or_else(|| anyhow::anyhow!("folder not found: {}", first))?;
    let mut current_id = root_folder.id;

    for (i, part) in parts[1..].iter().enumerate() {
        let is_last = i == parts.len() - 2;
        let detail = api.get_folder(current_id).await?;

        if let Some(folder) = detail.folders.iter().find(|f| f.name == *part) {
            if is_last {
                return Ok(ResolvedPath::Folder(folder.clone()));
            }
            current_id = folder.id;
        } else if is_last {
            if let Some(file) = detail.files.iter().find(|f| f.name == *part) {
                return Ok(ResolvedPath::File(file.clone()));
            }
            bail!("not found: {}", path);
        } else {
            bail!("folder not found: {}", *part);
        }
    }

    unreachable!()
}

fn resolve_parent_and_name(path: &str) -> (&str, &str) {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(pos) => {
            let parent = &trimmed[..pos];
            let name = &trimmed[pos + 1..];
            if parent.is_empty() {
                ("/", name)
            } else {
                (parent, name)
            }
        }
        None => ("/", trimmed),
    }
}

async fn resolve_folder_id(api: &ApiClient, path: &str) -> Result<Option<i64>> {
    match resolve_path(api, path).await? {
        ResolvedPath::Root => Ok(None),
        ResolvedPath::Folder(f) => Ok(Some(f.id)),
        ResolvedPath::File(_) => bail!("{} is a file, not a folder", path),
    }
}

// --- File management commands ---

async fn cmd_ls(args: &LsArgs, json: bool) -> Result<()> {
    let api = load_api()?;

    let mut entries: Vec<LsEntry> = Vec::new();

    match resolve_path(&api, &args.path).await? {
        ResolvedPath::Root => {
            let state = api.sync_state().await?;
            for f in state.folders.iter().filter(|f| f.parent_id.is_none()) {
                entries.push(LsEntry {
                    name: f.name.clone(),
                    kind: "folder".into(),
                    id: f.id,
                    size: None,
                    mime_type: None,
                    updated_at: f.updated_at.clone(),
                });
            }
            for f in state.files.iter().filter(|f| f.folder_id.is_none()) {
                entries.push(LsEntry {
                    name: f.name.clone(),
                    kind: "file".into(),
                    id: f.id,
                    size: f.size,
                    mime_type: f.mime_type.clone(),
                    updated_at: f.updated_at.clone(),
                });
            }
        }
        ResolvedPath::Folder(folder) => {
            let detail = api.get_folder(folder.id).await?;
            for f in &detail.folders {
                entries.push(LsEntry {
                    name: f.name.clone(),
                    kind: "folder".into(),
                    id: f.id,
                    size: None,
                    mime_type: None,
                    updated_at: f.updated_at.clone(),
                });
            }
            for f in &detail.files {
                entries.push(LsEntry {
                    name: f.name.clone(),
                    kind: "file".into(),
                    id: f.id,
                    size: f.size,
                    mime_type: f.mime_type.clone(),
                    updated_at: f.updated_at.clone(),
                });
            }
        }
        ResolvedPath::File(file) => {
            entries.push(LsEntry {
                name: file.name.clone(),
                kind: "file".into(),
                id: file.id,
                size: file.size,
                mime_type: file.mime_type.clone(),
                updated_at: file.updated_at.clone(),
            });
        }
    }

    if json {
        println!("{}", serde_json::to_string(&entries)?);
        return Ok(());
    }

    entries.sort_by(|a, b| {
        let type_ord = if a.kind == "folder" { 0 } else { 1 };
        let type_ord_b = if b.kind == "folder" { 0 } else { 1 };
        type_ord.cmp(&type_ord_b).then(a.name.cmp(&b.name))
    });

    for entry in &entries {
        if args.long {
            let size_str = if entry.kind == "folder" {
                "   <dir>".to_string()
            } else {
                format!("{:>8}", entry.size.map(|s| transfer::format_size(s as u64)).unwrap_or_else(|| "--".into()))
            };
            let date = &entry.updated_at[..10];
            let display_name = if entry.kind == "folder" {
                format!("{}/", entry.name)
            } else {
                entry.name.clone()
            };
            println!("{}  {}  {}", size_str, date, display_name);
        } else if entry.kind == "folder" {
            println!("{}/", entry.name);
        } else {
            println!("{}", entry.name);
        }
    }

    Ok(())
}

async fn cmd_upload(args: &UploadArgs, json: bool) -> Result<()> {
    let api = load_api()?;

    let (parent_path, dest_name) = resolve_parent_and_name(&args.dest);
    let folder_id = resolve_folder_id(&api, parent_path).await?;

    let data = if args.source == "-" {
        if io::stdin().is_terminal() {
            bail!("stdin is a terminal -- pipe data or use a file path");
        }
        let mut buf = Vec::new();
        io::stdin().read_to_end(&mut buf)?;
        buf
    } else {
        let path = Path::new(&args.source);
        if !path.exists() {
            bail!("file not found: {}", args.source);
        }
        std::fs::read(path).with_context(|| format!("cannot read: {}", args.source))?
    };

    let file_name = if dest_name.is_empty() || dest_name == "/" {
        if args.source == "-" {
            "stdin".to_string()
        } else {
            Path::new(&args.source)
                .file_name()
                .context("source has no filename")?
                .to_string_lossy()
                .to_string()
        }
    } else {
        dest_name.to_string()
    };

    let mime = if args.source == "-" {
        "application/octet-stream".to_string()
    } else {
        transfer::mime_from_extension(Path::new(&args.source))
    };

    let result = api.upload_file(&file_name, &mime, folder_id, data).await?;

    if json {
        println!("{}", serde_json::to_string(&result)?);
    } else {
        let size = result.size.map(|s| transfer::format_size(s as u64)).unwrap_or_default();
        ui::success(&format!("Uploaded {} ({})", result.name, size));
    }

    Ok(())
}

async fn cmd_download(args: &DownloadArgs, json: bool) -> Result<()> {
    let api = load_api()?;

    let file = match resolve_path(&api, &args.remote_path).await? {
        ResolvedPath::File(f) => f,
        ResolvedPath::Folder(_) => bail!("{} is a folder, not a file", args.remote_path),
        ResolvedPath::Root => bail!("cannot download root"),
    };

    let mut local_path = PathBuf::from(&args.local_dest);
    if local_path.is_dir() {
        local_path = local_path.join(&file.name);
    }

    let resp = api.download_file_stream(file.id).await?;
    let total = resp.content_length().unwrap_or(file.size.unwrap_or(0) as u64);

    let use_progress = show_progress(json) && total > 1024 * 100;

    let pb = if use_progress {
        Some(make_progress_bar(total))
    } else {
        None
    };

    if let Some(parent) = local_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp_path = sync::transfer::temp_path_for(&local_path, file.id);
    let mut out = std::fs::File::create(&tmp_path)
        .with_context(|| format!("cannot create: {}", tmp_path.display()))?;

    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("download stream error")?;
        out.write_all(&chunk)?;
        downloaded += chunk.len() as u64;
        if let Some(ref pb) = pb {
            pb.inc(chunk.len() as u64);
        }
    }

    drop(out);
    std::fs::rename(&tmp_path, &local_path)?;

    if let Some(pb) = pb {
        pb.finish_and_clear();
    }

    if json {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "name": file.name,
                "size": downloaded,
                "path": local_path.to_string_lossy(),
            }))?
        );
    } else {
        println!(
            "downloaded {} ({}) -> {}",
            file.name,
            transfer::format_size(downloaded),
            local_path.display()
        );
    }

    Ok(())
}

async fn cmd_mkdir(args: &MkdirArgs, json: bool) -> Result<()> {
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

async fn cmd_mv(args: &MvArgs, json: bool) -> Result<()> {
    let api = load_api()?;

    let resolved = resolve_path(&api, &args.source).await?;

    let (dest_parent, dest_name) = resolve_parent_and_name(&args.dest);
    let dest_folder_id = resolve_folder_id(&api, dest_parent).await?;

    match resolved {
        ResolvedPath::File(file) => {
            let new_name = if dest_name.is_empty() { None } else { Some(dest_name) };
            let folder_arg = Some(dest_folder_id);
            let result = api.update_file(file.id, new_name, folder_arg).await?;
            if json {
                println!("{}", serde_json::to_string(&result)?);
            } else {
                ui::success(&format!("Moved {} → {}", file.name, args.dest));
            }
        }
        ResolvedPath::Folder(folder) => {
            let new_name = if dest_name.is_empty() { None } else { Some(dest_name) };
            let parent_arg = Some(dest_folder_id);
            let result = api.update_folder(folder.id, new_name, parent_arg).await?;
            if json {
                println!("{}", serde_json::to_string(&result)?);
            } else {
                ui::success(&format!("Moved {}/ → {}", folder.name, args.dest));
            }
        }
        ResolvedPath::Root => bail!("cannot move root"),
    }

    Ok(())
}

async fn cmd_rm(args: &RmArgs, json: bool) -> Result<()> {
    let api = load_api()?;

    let resolved = resolve_path(&api, &args.path).await?;

    match resolved {
        ResolvedPath::File(file) => {
            if !args.force && !json {
                print!("delete {}? [y/N] ", file.name);
                io::stdout().flush()?;
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    ui::step("Cancelled");
                    return Ok(());
                }
            }
            api.delete_file(file.id).await?;
            if json {
                println!("{}", serde_json::json!({"deleted": true, "name": file.name}));
            } else {
                ui::success(&format!("Deleted {}", file.name));
            }
        }
        ResolvedPath::Folder(folder) => {
            if !args.force && !json {
                print!("delete {}/? [y/N] ", folder.name);
                io::stdout().flush()?;
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    ui::step("Cancelled");
                    return Ok(());
                }
            }
            api.delete_folder(folder.id).await?;
            if json {
                println!("{}", serde_json::json!({"deleted": true, "name": folder.name}));
            } else {
                ui::success(&format!("Deleted {}/", folder.name));
            }
        }
        ResolvedPath::Root => bail!("cannot delete root"),
    }

    Ok(())
}

// --- Search command ---

async fn cmd_search(args: &SearchArgs, json: bool) -> Result<()> {
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

    let max_path = results.iter().map(|r| r.path.len()).max().unwrap_or(20).min(60);

    for r in &results {
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

    Ok(())
}

// --- Share commands ---

async fn cmd_share(args: &ShareArgs, json: bool) -> Result<()> {
    let api = load_api()?;
    let config = config::Config::load()?;

    let resolved = resolve_path(&api, &args.path).await?;

    let (file_id, folder_id) = match &resolved {
        ResolvedPath::File(f) => (Some(f.id), None),
        ResolvedPath::Folder(f) => (None, Some(f.id)),
        ResolvedPath::Root => bail!("cannot share root"),
    };

    let expires_at = args.expires.as_deref().map(parse_expiry).transpose()?;
    let share = api
        .create_share(file_id, folder_id, &args.permission, expires_at.as_deref())
        .await?;

    if json {
        println!("{}", serde_json::to_string(&share)?);
    } else {
        let url = format!("{}/s/{}", config.server_url.trim_end_matches('/'), share.token);
        println!("{}", url);
        if let Some(ref exp) = share.expires_at {
            println!("expires: {}", exp);
        }
    }

    Ok(())
}

async fn cmd_unshare(args: &UnshareArgs, json: bool) -> Result<()> {
    let api = load_api()?;
    api.delete_share(args.id).await?;

    if json {
        println!("{}", serde_json::json!({"deleted": true, "id": args.id}));
    } else {
        ui::success(&format!("Share {} revoked", args.id));
    }

    Ok(())
}

async fn cmd_shares(json: bool) -> Result<()> {
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
        let target = if share.file_id.is_some() {
            "file"
        } else {
            "folder"
        };
        let target_id = share.file_id.or(share.folder_id).unwrap_or(0);
        let exp = share
            .expires_at
            .as_deref()
            .unwrap_or("never");
        println!(
            "#{:<4}  {}  {} {}  perm={}  expires={}",
            share.id, share.token, target, target_id, share.permission, exp
        );
    }

    Ok(())
}

// --- Token commands ---

async fn cmd_token(sub: TokenCommand, json: bool) -> Result<()> {
    let api = load_api()?;

    match sub {
        TokenCommand::Create(args) => {
            let token = api.create_token(&args.name).await?;
            if json {
                println!("{}", serde_json::to_string(&token)?);
            } else {
                println!("id:    {}", token.id);
                println!("name:  {}", token.name);
                if let Some(ref val) = token.token {
                    println!("token: {}", val);
                    println!("\nsave this token -- it won't be shown again.");
                }
            }
        }
        TokenCommand::List => {
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
        }
        TokenCommand::Revoke(args) => {
            api.delete_token(args.id).await?;
            if json {
                println!("{}", serde_json::json!({"deleted": true, "id": args.id}));
            } else {
                ui::success(&format!("Token {} revoked", args.id));
            }
        }
    }

    Ok(())
}

// --- Sync commands (unchanged) ---

fn cmd_start() -> Result<()> {
    if let Some(pid) = daemon::is_running()? {
        ui::warn(&format!("Already running (PID {})", pid));
        return Ok(());
    }

    config::Config::load().context("fix config before starting daemon")?;

    let log_dir = daemon::log_dir()?;
    std::fs::create_dir_all(&log_dir)?;

    let log_file = daemon::log_path()?;
    let stdout = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
        .with_context(|| format!("cannot open log file: {}", log_file.display()))?;
    let stderr = stdout
        .try_clone()
        .context("failed to clone log file handle")?;

    let pid_file = daemon::pid_path()?;

    ui::step("Starting daemon");

    let daemonize = daemonize::Daemonize::new()
        .pid_file(&pid_file)
        .chown_pid_file(true)
        .stdout(stdout)
        .stderr(stderr);

    daemonize.start().context("failed to daemonize")?;

    daemon::init_daemon_logging();

    let rt = tokio::runtime::Runtime::new()
        .context("failed to create async runtime")?;

    rt.block_on(run_daemon())
}

fn cmd_stop() -> Result<()> {
    match daemon::is_running()? {
        Some(pid) => {
            let kill_result = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            if kill_result != 0 {
                let _ = std::fs::remove_file(daemon::pid_path()?);
                ui::warn("Process already gone, cleaned up PID file");
                return Ok(());
            }

            for _ in 0..50 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
                if !alive {
                    let _ = daemon::clear_runtime_files();
                    ui::success(&format!("Stopped (was PID {})", pid));
                    return Ok(());
                }
            }

            unsafe {
                libc::kill(pid as i32, libc::SIGKILL);
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
            let _ = daemon::clear_runtime_files();
            ui::success(&format!("Killed (PID {})", pid));
            Ok(())
        }
        None => {
            ui::warn("Not running");
            Ok(())
        }
    }
}

fn cmd_restart() -> Result<()> {
    cmd_stop()?;
    cmd_start()
}

fn cmd_logs(follow: bool) -> Result<()> {
    let log_file = daemon::log_path()?;
    if !log_file.exists() {
        ui::step("No logs yet");
        return Ok(());
    }

    let mut args = vec![];
    if follow {
        args.extend(["-f", "-n", "50"]);
    } else {
        args.extend(["-n", "50"]);
    }
    let path_str = log_file.to_string_lossy().to_string();
    args.push(&path_str);

    let status = std::process::Command::new("tail")
        .args(&args)
        .status()
        .context("failed to run tail")?;

    if !status.success() {
        bail!("tail exited with error");
    }
    Ok(())
}

async fn sync_loop(engine: &sync::SyncEngine) -> Result<()> {
    let sync_dir = engine.sync_dir().to_path_buf();
    let poll_interval = engine.config().poll_interval;

    let watcher = sync::watcher::FsWatcher::new(&sync_dir, engine.ignore_rules())?;

    let mut poll_timer =
        tokio::time::interval(tokio::time::Duration::from_secs(poll_interval));
    poll_timer.tick().await;

    let mut sigterm =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut sigint =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;

    loop {
        if let Some(paths) = watcher.try_recv() {
            tokio::select! {
                biased;
                _ = sigterm.recv() => {
                    info!("shutting down (SIGTERM)");
                    break;
                }
                _ = sigint.recv() => {
                    info!("shutting down (SIGINT)");
                    break;
                }
                result = engine.process_local_changes(paths) => {
                    if let Err(e) = result {
                        error!("local sync error: {}", e);
                    }
                }
            }
        }

        tokio::select! {
            biased;
            _ = sigterm.recv() => {
                info!("shutting down (SIGTERM)");
                break;
            }
            _ = sigint.recv() => {
                info!("shutting down (SIGINT)");
                break;
            }
            _ = poll_timer.tick() => {
                match engine.full_sync().await {
                    Ok(report) => {
                        if report.total_changes() > 0 {
                            info!("sync ({} changes)", report.total_changes());
                        }
                        if report.blocked_deletes > 0 {
                            warn!(
                                "{} deletion(s) held back by the safety guard — run `nuage sync --dry-run` to inspect",
                                report.blocked_deletes
                            );
                        }
                    }
                    Err(e) => error!("remote sync error: {}", e),
                }
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {}
        }
    }

    Ok(())
}

async fn run_daemon() -> Result<()> {
    let engine = build_engine()?;
    engine.preflight()?;

    let _ = daemon::write_meta();
    if config::Config::ensure_secure_permissions().unwrap_or(false) {
        warn!("tightened permissions on ~/.nuage.yml — it held an API token readable by other users");
    }

    info!("daemon started, PID {}", std::process::id());

    let report = engine.full_sync().await?;
    let file_count = engine.state().file_count().unwrap_or(0);
    info!("watching {} ({} files synced)", engine.sync_dir().display(), file_count);

    if report.conflicts > 0 {
        info!("{} conflicts resolved", report.conflicts);
    }
    if report.blocked_deletes > 0 {
        warn!(
            "{} deletion(s) held back by the safety guard — run `nuage sync --dry-run` to inspect",
            report.blocked_deletes
        );
    }

    sync_loop(&engine).await?;

    let _ = daemon::clear_runtime_files();
    info!("daemon stopped");
    Ok(())
}

async fn cmd_watch() -> Result<()> {
    let engine = build_engine()?;
    engine.preflight()?;

    ui::step("Starting initial sync");
    let report = engine.full_sync().await?;

    let file_count = engine.state().file_count().unwrap_or(0);
    println!(
        "[nuage] watching {} (synced {} files)",
        engine.sync_dir().display(),
        file_count
    );

    report_warnings(&report);

    sync_loop(&engine).await?;

    println!("\n[nuage] stopped");
    Ok(())
}

async fn cmd_sync(args: &SyncArgs) -> Result<()> {
    let options = sync::SyncOptions {
        dry_run: args.dry_run,
        allow_bulk_delete: args.allow_bulk_delete,
    };
    let engine = build_engine()?.with_options(options);
    engine.preflight()?;

    if args.retry_failed {
        let cleared = engine.state().clear_all_quarantine()?;
        if cleared > 0 {
            ui::step(&format!("Cleared {} quarantined file(s)", cleared));
        }
    }

    if args.repair_state {
        repair_state(&engine, args.dry_run)?;
    }

    if args.dry_run {
        ui::step("Dry run — nothing will be modified");
    } else {
        ui::step("Syncing");
    }

    let report = engine.full_sync().await?;

    if args.dry_run {
        if report.planned.is_empty() {
            ui::success("Already in sync — no changes planned");
        } else {
            for line in &report.planned {
                println!("  {}", line);
            }
            ui::success(&format!("{} change(s) planned", report.planned.len()));
        }
    } else {
        ui::success(&format!("Sync complete ({} changes)", report.total_changes()));
    }

    report_warnings(&report);
    Ok(())
}

/// Drops tracking records that point at local files which no longer exist, leaving the
/// server untouched, then forgets the cursor so the next pass rebuilds tracking from the
/// server's own view. This recovers from state that drifted out of agreement with the
/// filesystem — for example records written at the sync root because a file's parent
/// folder could not be resolved at the time.
fn repair_state(engine: &sync::SyncEngine, dry_run: bool) -> Result<()> {
    let sync_dir = engine.sync_dir();
    let state = engine.state();

    let stale_files: Vec<String> = state
        .all_files()?
        .into_iter()
        .map(|r| r.local_path)
        .filter(|p| !sync_dir.join(p).exists())
        .collect();

    let stale_folders: Vec<String> = state
        .all_folders()?
        .into_iter()
        .map(|r| r.local_path)
        .filter(|p| !sync_dir.join(p).is_dir())
        .collect();

    if stale_files.is_empty() && stale_folders.is_empty() {
        ui::step("State is consistent with the filesystem — nothing to repair");
        return Ok(());
    }

    if dry_run {
        ui::warn(&format!(
            "would drop {} stale file record(s) and {} stale folder record(s); the server would not be touched",
            stale_files.len(),
            stale_folders.len()
        ));
        return Ok(());
    }

    for path in &stale_files {
        state.remove_file(path)?;
    }
    for path in &stale_folders {
        state.remove_folder(path)?;
    }
    state.clear_cursor()?;

    ui::success(&format!(
        "Dropped {} stale file record(s) and {} stale folder record(s) — the server was not modified",
        stale_files.len(),
        stale_folders.len()
    ));
    Ok(())
}

fn report_warnings(report: &sync::SyncReport) {
    if report.conflicts > 0 {
        ui::warn(&format!(
            "{} conflict(s) — both versions kept, local copy renamed",
            report.conflicts
        ));
    }
    if report.blocked_deletes > 0 {
        ui::warn(&format!(
            "{} deletion(s) held back by the safety guard — see `nuage sync --dry-run`",
            report.blocked_deletes
        ));
    }
    if report.skipped > 0 {
        ui::warn(&format!(
            "{} quarantined file(s) skipped — retry with `nuage sync --retry-failed`",
            report.skipped
        ));
    }
    if report.errors > 0 {
        ui::warn(&format!("{} item(s) failed this pass", report.errors));
    }
}

async fn cmd_status() -> Result<()> {
    match daemon::is_running()? {
        Some(pid) => {
            println!("Daemon: running (PID {})", pid);
            if let Ok(Some(meta)) = daemon::read_meta() {
                println!("Started: {}", meta.started_at);
                println!("Binary: {}", meta.exe);
            }
        }
        None => println!("Daemon: stopped"),
    }

    let config = config::Config::load()?;
    let sync_dir = config.sync_dir_expanded()?;

    if !sync_dir.join(".nuage").join("state.db").exists() {
        println!("Server: {}", config.server_url);
        println!("Sync dir: {}", sync_dir.display());
        println!("Last sync: never");
        println!("Files: 0");
        println!("Folders: 0");
        return Ok(());
    }

    let state = sync::state::SyncState::new(&sync_dir)?;
    let cursor = state.get_cursor()?.unwrap_or_else(|| "never".to_string());
    let file_count = state.file_count()?;
    let folder_count = state.folder_count()?;

    println!("Server: {}", config.server_url);
    println!("Sync dir: {}", sync_dir.display());
    println!("Last sync: {}", cursor);
    println!("Files: {}", file_count);
    println!("Folders: {}", folder_count);

    if !config.selective_sync.is_empty() {
        println!("Selective sync: {}", config.selective_sync.join(", "));
    }

    let quarantined = state.list_quarantined()?;
    if !quarantined.is_empty() {
        println!("\nQuarantined ({}):", quarantined.len());
        for record in &quarantined {
            println!(
                "  file {} — {} failure(s): {}",
                record.facile_id, record.attempts, record.reason
            );
        }
        println!("\nRetry with `nuage sync --retry-failed`.");
    }

    Ok(())
}

async fn cmd_login() -> Result<()> {
    println!("nuage -- interactive setup\n");

    let server_url = prompt("Server URL")?;
    if server_url.is_empty() {
        bail!("server URL cannot be empty");
    }

    print!("API token: ");
    io::stdout().flush()?;
    let token = rpassword::read_password().context("failed to read token")?;
    let token = token.trim().to_string();
    if token.is_empty() {
        bail!("token cannot be empty");
    }

    let default_dir = "~/Nuage".to_string();
    let sync_dir_input = prompt_with_default("Sync directory", &default_dir)?;
    let sync_dir = if sync_dir_input.is_empty() {
        default_dir
    } else {
        sync_dir_input
    };

    let config = config::Config {
        server_url: server_url.clone(),
        token: token.clone(),
        sync_dir: sync_dir.clone(),
        poll_interval: 30,
        ignore_patterns: vec![
            ".DS_Store".to_string(),
            "*.tmp".to_string(),
            ".nuage/".to_string(),
            "Thumbs.db".to_string(),
            ".git/".to_string(),
        ],
        selective_sync: vec![],
    };

    println!("\nTesting connection...");
    let client = api::ApiClient::new(&server_url, &token)?;
    client.test_connection().await?;
    println!("connected successfully");

    config.save()?;
    println!("config saved to ~/.nuage.yml");

    let expanded = shellexpand::tilde(&sync_dir);
    let sync_path = std::path::PathBuf::from(expanded.as_ref());
    std::fs::create_dir_all(&sync_path)
        .with_context(|| format!("cannot create sync directory: {}", sync_path.display()))?;
    println!("sync directory ready: {}", sync_path.display());

    println!("\nRun `nuage start` to start syncing in the background.");
    println!("Run `nuage watch` for foreground mode.");
    Ok(())
}

/// Upgrades in place. The daemon is stopped first: replacing the executable of a
/// running process leaves it holding a half-written image, and a daemon left running
/// through an upgrade would keep serving the old code anyway.
async fn cmd_upgrade() -> Result<()> {
    let was_running = daemon::is_running()?.is_some();
    if was_running {
        ui::step("Stopping daemon for upgrade");
        cmd_stop()?;
    }

    ui::step("Upgrading nuage");
    let status = std::process::Command::new("cargo")
        .args([
            "install",
            "--git",
            "https://github.com/FacileStudio/nuage-cli.git",
            "--force",
        ])
        .status()?;

    if !status.success() {
        if was_running {
            ui::warn("Upgrade failed — restarting the previous daemon");
            let _ = cmd_start();
        }
        bail!("upgrade failed");
    }

    if was_running {
        ui::step("Restarting daemon");
        cmd_start()?;
    }

    ui::success("Upgraded to the latest version");
    Ok(())
}

fn prompt(label: &str) -> Result<String> {
    print!("{}: ", label);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

fn prompt_with_default(label: &str, default: &str) -> Result<String> {
    print!("{} [{}]: ", label, default);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}
