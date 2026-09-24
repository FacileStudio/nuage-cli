use anyhow::Result;
use clap::{Args, Subcommand};

use crate::api::ApiClient;
use crate::commands::space::{is_personal, resolve_space_name, selected_space, PERSONAL};
use crate::config;
use crate::ui;

#[derive(Subcommand)]
pub enum SpacesCommand {
    /// List spaces available to this account
    List,
    /// Select the space every command works in
    Use(SpacesUseArgs),
}

#[derive(Args)]
pub struct SpacesUseArgs {
    /// Space name or id, or `personal` for your own files
    #[arg(value_name = "NAME_OR_ID", required_unless_present = "none")]
    pub name: Option<String>,

    /// Same as `personal`
    #[arg(long)]
    pub none: bool,
}

pub async fn cmd_spaces(sub: SpacesCommand, json: bool) -> Result<()> {
    match sub {
        SpacesCommand::List => cmd_spaces_list(json).await,
        SpacesCommand::Use(args) => cmd_spaces_use(&args, json).await,
    }
}

fn print_space_row(id: &str, name: &str, selected: bool, detail: &str) {
    let marker = if selected { "*" } else { " " };
    println!("{} {:<4} {:<24} {}", marker, id, name, ui::dim(detail));
}

/// The personal tree is listed alongside the real spaces even though the
/// server does not return it, because a target you cannot see is a target
/// you cannot switch back to.
pub async fn cmd_spaces_list(json: bool) -> Result<()> {
    let config = config::Config::load()?;
    let api = ApiClient::new(&config.server_url, &config.token, None)?;
    let spaces = api.list_spaces().await?;
    let selected = selected_space(&config);

    if json {
        println!(
            "{}",
            serde_json::json!({ "selected": selected, "spaces": spaces })
        );
        return Ok(());
    }

    print_space_row("-", PERSONAL, selected.is_none(), "your own files");

    for space in &spaces {
        print_space_row(
            &space.id.to_string(),
            &space.name,
            Some(space.id) == selected,
            &space.role,
        );
    }

    Ok(())
}

fn report_space_choice(chosen: Option<i64>) {
    match chosen {
        Some(id) => {
            ui::success(&format!("Now working in space {id}"));
            ui::hint(&format!(
                "`nuage spaces use {PERSONAL}` goes back to your own files"
            ));
        }
        None => ui::success("Now working in your personal space"),
    }
}

/// Read-modify-write against the file rather than the validated config, so a
/// `NUAGE_TOKEN` or `NUAGE_SERVER_URL` set for this one run is not baked into
/// `~/.nuage.yml` as if it had been typed there.
pub async fn cmd_spaces_use(args: &SpacesUseArgs, json: bool) -> Result<()> {
    let raw = args.name.as_deref().map(str::trim).unwrap_or(PERSONAL);
    let wants_personal = args.none || is_personal(raw);

    let chosen = if wants_personal {
        None
    } else {
        let loaded = config::Config::load()?;
        let api = ApiClient::new(&loaded.server_url, &loaded.token, None)?;
        Some(match raw.parse::<i64>() {
            Ok(id) => id,
            Err(_) => resolve_space_name(&api, raw).await?,
        })
    };

    let mut config = config::Config::load_or_default()?;
    config.space = chosen;
    config.save()?;

    if json {
        println!("{}", serde_json::json!({ "selected": chosen }));
        return Ok(());
    }

    report_space_choice(chosen);
    Ok(())
}
