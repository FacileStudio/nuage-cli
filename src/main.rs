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

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};

use commands::{
    daemon_cmds::{self, cmd_logs, cmd_restart, cmd_start, cmd_stop, LogsArgs},
    keys::{self, KeysCommand},
    ls::{self, LsArgs},
    search::{self, SearchArgs},
    shares::{self, ShareArgs, UnshareArgs},
    space,
    spaces::{self, SpacesCommand},
    status,
    sync_cmds::{self, SyncArgs},
    tokens::{self, TokenCommand},
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
    /// Manage spaces
    #[command(subcommand)]
    Spaces(SpacesCommand),
    /// Upgrade nuage-cli
    Upgrade,
    /// List files and folders at a remote path
    Ls(LsArgs),
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

/// Commands that never consult a space and cannot go through the async
/// dispatch, because they either fork the daemon or read its pid file.
fn run(cli: Cli) -> Result<()> {
    match cli.command {
        None => {
            Cli::command().print_help()?;
            println!();
            Ok(())
        }
        Some(Command::Start) => cmd_start(),
        Some(Command::Stop) => cmd_stop(),
        Some(Command::Restart) => cmd_restart(),
        Some(Command::Logs(args)) => cmd_logs(args.follow),
        Some(other) => run_async(other, cli.json),
    }
}

fn run_async(command: Command, json: bool) -> Result<()> {
    daemon::init_terminal_logging();

    let rt = tokio::runtime::Runtime::new().context("failed to create async runtime")?;

    rt.block_on(async {
        space::set_override(space::resolve_env_space().await?);
        dispatch(command, json).await
    })
}

async fn dispatch(command: Command, json: bool) -> Result<()> {
    match command {
        Command::Watch => sync_cmds::cmd_watch().await,
        Command::Sync(args) => sync_cmds::cmd_sync(&args).await,
        Command::Status => status::cmd_status().await,
        Command::Login(args) => login::run(args.server, args.token).await,
        Command::Logout => login::logout(),
        Command::Upgrade => daemon_cmds::cmd_upgrade().await,
        Command::Ls(args) => ls::cmd_ls(&args, json).await,
        Command::Share(args) => shares::cmd_share(&args, json).await,
        Command::Unshare(args) => shares::cmd_unshare(&args, json).await,
        Command::Shares => shares::cmd_shares(json).await,
        Command::Search(args) => search::cmd_search(&args, json).await,
        Command::Spaces(sub) => spaces::cmd_spaces(sub, json).await,
        Command::Token(sub) => tokens::cmd_token(sub, json).await,
        Command::Keys(sub) => keys::cmd_keys(sub, json).await,
        Command::Start | Command::Stop | Command::Restart | Command::Logs(_) => {
            unreachable!()
        }
    }
}
