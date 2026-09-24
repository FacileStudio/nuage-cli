use anyhow::{bail, Result};
use clap::Args;

use crate::commands::{repair, supervisor, targets};
use crate::config::{self, Config};
use crate::sync::{self, SyncTarget};
use crate::ui;

#[derive(Args)]
pub struct SyncArgs {
    #[arg(long, help = "Show what would change without applying anything")]
    pub dry_run: bool,
    #[arg(
        long,
        help = "Allow propagating an unusually large batch of local deletions"
    )]
    pub allow_bulk_delete: bool,
    #[arg(long, help = "Clear quarantined files and retry them")]
    pub retry_failed: bool,
    #[arg(
        long,
        help = "Drop tracking records whose local file is gone, without deleting anything on the server, then re-enumerate"
    )]
    pub repair_state: bool,
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

fn report_sync(name: &str, report: &sync::SyncReport, dry_run: bool, named: bool) {
    let prefix = if named {
        format!("{name}: ")
    } else {
        String::new()
    };

    if !dry_run {
        ui::success(&format!(
            "{prefix}sync complete ({} changes)",
            report.total_changes()
        ));
        return;
    }

    if report.planned.is_empty() {
        ui::success(&format!("{prefix}already in sync — no changes planned"));
        return;
    }

    for line in &report.planned {
        println!("  {prefix}{line}");
    }
    ui::success(&format!(
        "{prefix}{} change(s) planned",
        report.planned.len()
    ));
}

fn clear_quarantine(engine: &sync::SyncEngine) -> Result<()> {
    let cleared = engine.state().clear_all_quarantine()?;
    if cleared > 0 {
        ui::step(&format!("Cleared {} quarantined file(s)", cleared));
    }
    Ok(())
}

async fn sync_target(config: &Config, target: SyncTarget, args: &SyncArgs) -> Result<sync::SyncReport> {
    let options = sync::SyncOptions {
        dry_run: args.dry_run,
        allow_bulk_delete: args.allow_bulk_delete,
    };
    let engine = targets::build_engine(config, target)?.with_options(options);
    engine.preflight()?;

    if args.retry_failed {
        clear_quarantine(&engine)?;
    }
    if args.repair_state {
        repair::repair_state(&engine, args.dry_run)?;
    }

    engine.full_sync().await
}

async fn sync_all(
    config: &Config,
    targets: Vec<SyncTarget>,
    unknown: Vec<String>,
    args: &SyncArgs,
) -> Result<()> {
    let named = targets.len() > 1;
    let count = targets.len() + unknown.len();
    let mut failures = unknown;

    for target in targets {
        let name = target.name.clone();
        match sync_target(config, target, args).await {
            Ok(report) => {
                report_sync(&name, &report, args.dry_run, named);
                report_warnings(&report);
            }
            Err(e) => {
                ui::warn(&format!("{name}: {e:#}"));
                failures.push(name);
            }
        }
    }

    if failures.is_empty() {
        return Ok(());
    }

    bail!(
        "{} of {count} spaces failed: {}",
        failures.len(),
        failures.join(", ")
    )
}

pub async fn cmd_sync(args: &SyncArgs) -> Result<()> {
    let config = config::Config::load()?;
    let targets = targets::resolve_targets(&config).await?;
    targets.warn_unknown();

    if args.dry_run {
        ui::step("Dry run — nothing will be modified");
    } else {
        ui::step("Syncing");
    }

    sync_all(&config, targets.known, targets.unknown, args).await
}

pub async fn cmd_watch() -> Result<()> {
    let config = config::Config::load()?;
    let targets = targets::resolve_targets(&config).await?;
    targets.warn_unknown();

    let engines = targets
        .known
        .into_iter()
        .map(|target| targets::build_engine(&config, target))
        .collect::<Result<Vec<_>>>()?;

    supervisor::run(engines).await?;

    println!("\n[nuage] stopped");
    Ok(())
}
