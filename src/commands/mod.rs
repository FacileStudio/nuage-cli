pub mod daemon_cmds;
pub mod daemon_run;
pub mod keys;
pub mod paths;
pub mod progress;
pub mod repair;
pub mod search;
pub mod shares;
pub mod space;
pub mod spaces;
pub mod status;
pub mod supervisor;
pub mod sync_cmds;
pub mod targets;
pub mod tokens;
pub mod upgrade;

use clap::Args;

#[derive(Args)]
pub struct LoginArgs {
    #[arg(long, help = "Server URL, with or without the /api suffix")]
    pub server: Option<String>,
    #[arg(
        long,
        help = "Paste an API token instead of signing in through the browser"
    )]
    pub token: bool,
}
