use anyhow::Result;
use tracing::{info, warn};

use crate::commands::{supervisor, targets};
use crate::config;
use crate::daemon;

pub async fn run_daemon() -> Result<()> {
    let config = config::Config::load()?;
    let resolved = targets::resolve_targets(&config).await?;
    resolved.warn_unknown();

    let mut engines = Vec::with_capacity(resolved.known.len());
    for target in resolved.known {
        engines.push(targets::build_engine(&config, target)?);
    }

    let _ = daemon::write_meta();
    if config::Config::ensure_secure_permissions().unwrap_or(false) {
        warn!(
            "tightened permissions on ~/.nuage.yml — it held an API token readable by other users"
        );
    }

    info!("daemon started, PID {}", std::process::id());

    supervisor::run(engines).await?;

    let _ = daemon::clear_runtime_files();
    info!("daemon stopped");
    Ok(())
}
