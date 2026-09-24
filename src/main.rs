mod api;
mod commands;
mod config;
mod daemon;
mod handoff;
mod hash;
mod ignore;
mod login;
mod sync;
mod ui;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

use commands::{
    daemon_cmds::{self, cmd_logs, cmd_restart, cmd_start, cmd_stop, LogsArgs},
    download::{self, DownloadArgs},
    files::{self, MkdirArgs, MvArgs, RmArgs},
    keys::{self, KeysCommand},
    ls::{self, LsArgs},
    search::{self, SearchArgs},
    shares::{self, ShareArgs, UnshareArgs},
    space,
    spaces::{self, SpacesCommand},
    status,
    sync_cmds::{self, SyncArgs},
    tokens::{self, TokenCommand},
    upload::{self, UploadArgs},
    LoginArgs,
};

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

    #[arg(
        long,
        global = true,
        value_name = "NAME_OR_ID",
        help = "Act on this space instead of the selected one"
    )]
    space: Option<String>,

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
    /// Sign in to a Nuage server
    Login(LoginArgs),
    /// Sign out, clearing the stored token
    Logout,
    /// List spaces, or select the one this machine works in
    #[command(subcommand)]
    Spaces(SpacesCommand),
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
    /// Manage API keys
    #[command(subcommand)]
    Keys(KeysCommand),
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

/// Commands that never consult the selected space.
///
/// Refused rather than ignored: `nuage --space Shared sync` reads as scoping
/// the sync and cannot, and a flag a command accepts and discards is worse than
/// one it rejects.
fn ignores_space(command: &Option<Command>) -> bool {
    matches!(
        command,
        None | Some(
            Command::Start
                | Command::Stop
                | Command::Restart
                | Command::Logs(_)
                | Command::Sync(_)
                | Command::Watch
                | Command::Login(_)
                | Command::Logout
                | Command::Upgrade
        )
    )
}

fn run(cli: Cli) -> Result<()> {
    let space_flag = cli.space.clone();

    if space_flag.is_some() && ignores_space(&cli.command) {
        bail!(
            "`--space` does not apply to this command. It scopes the file, share and search commands; the sync daemon syncs every space into one directory."
        );
    }

    match cli.command {
        Some(Command::Start) => cmd_start(),
        Some(Command::Stop) => cmd_stop(),
        Some(Command::Restart) => cmd_restart(),
        Some(Command::Logs(args)) => cmd_logs(args.follow),
        other => run_async(other, cli.json, space_flag),
    }
}

fn run_async(command: Option<Command>, json: bool, space_flag: Option<String>) -> Result<()> {
    daemon::init_terminal_logging();

    let rt = tokio::runtime::Runtime::new().context("failed to create async runtime")?;

    rt.block_on(async {
        space::SPACE_OVERRIDE
            .set(space::resolve_space_flag(space_flag.as_deref()).await?)
            .ok();

        dispatch(command, json).await
    })
}

async fn dispatch(command: Option<Command>, json: bool) -> Result<()> {
    match command {
        None | Some(Command::Watch) => sync_cmds::cmd_watch().await,
        Some(Command::Sync(args)) => sync_cmds::cmd_sync(&args).await,
        Some(Command::Status) => status::cmd_status().await,
        Some(Command::Login(args)) => login::run(args.server, args.token).await,
        Some(Command::Logout) => login::logout(),
        Some(Command::Upgrade) => daemon_cmds::cmd_upgrade().await,
        Some(Command::Ls(args)) => ls::cmd_ls(&args, json).await,
        Some(Command::Upload(args)) => upload::cmd_upload(&args, json).await,
        Some(Command::Download(args)) => download::cmd_download(&args, json).await,
        Some(Command::Mkdir(args)) => files::cmd_mkdir(&args, json).await,
        Some(Command::Mv(args)) => files::cmd_mv(&args, json).await,
        Some(Command::Rm(args)) => files::cmd_rm(&args, json).await,
        Some(Command::Share(args)) => shares::cmd_share(&args, json).await,
        Some(Command::Unshare(args)) => shares::cmd_unshare(&args, json).await,
        Some(Command::Shares) => shares::cmd_shares(json).await,
        Some(Command::Search(args)) => search::cmd_search(&args, json).await,
        Some(Command::Spaces(sub)) => spaces::cmd_spaces(sub, json).await,
        Some(Command::Token(sub)) => tokens::cmd_token(sub, json).await,
        Some(Command::Keys(sub)) => keys::cmd_keys(sub, json).await,
        Some(Command::Start | Command::Stop | Command::Restart | Command::Logs(_)) => {
            unreachable!()
        }
    }
}
