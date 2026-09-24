mod cursor;
mod files;
mod folders;
mod quarantine;
mod rows;
mod schema;

#[cfg(test)]
mod tests;

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

pub use quarantine::QUARANTINE_THRESHOLD;

#[allow(dead_code)]
pub struct FileRecord {
    pub id: i64,
    pub facile_id: String,
    pub name: String,
    pub local_path: String,
    pub hash: Option<String>,
    pub size: Option<i64>,
    pub folder_id: Option<i64>,
    pub remote_updated_at: Option<String>,
    pub local_modified_at: Option<i64>,
    pub synced_at: String,
}

#[allow(dead_code)]
pub struct FolderRecord {
    pub id: i64,
    pub facile_id: String,
    pub name: String,
    pub local_path: String,
    pub parent_id: Option<i64>,
    pub remote_updated_at: Option<String>,
    pub synced_at: String,
}

/// A record of a file that repeatedly failed to sync.
#[allow(dead_code)]
pub struct QuarantineRecord {
    pub facile_id: String,
    pub reason: String,
    pub attempts: u32,
    pub first_failed_at: String,
    pub last_failed_at: String,
}

/// The columns of one row of the `files` table, as written by
/// [`SyncState::upsert_file`].
#[derive(Default)]
pub struct UpsertFile {
    pub facile_id: String,
    pub name: String,
    pub local_path: String,
    pub hash: Option<String>,
    pub size: Option<i64>,
    pub folder_id: Option<i64>,
    pub remote_updated_at: Option<String>,
    pub local_modified_at: Option<i64>,
    pub synced_at: String,
}

/// The columns of one row of the `folders` table, as written by
/// [`SyncState::upsert_folder`].
#[derive(Default)]
pub struct UpsertFolder {
    pub facile_id: String,
    pub name: String,
    pub local_path: String,
    pub parent_id: Option<i64>,
    pub remote_updated_at: Option<String>,
    pub synced_at: String,
}

pub struct SyncState {
    db: Connection,
}

impl SyncState {
    pub fn new(sync_dir: &Path) -> Result<Self> {
        let db_dir = sync_dir.join(".nuage");
        std::fs::create_dir_all(&db_dir)
            .with_context(|| format!("cannot create state directory: {}", db_dir.display()))?;

        let db_path = db_dir.join("state.db");
        let db = Connection::open(&db_path)
            .with_context(|| format!("cannot open state database: {}", db_path.display()))?;

        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .context("failed to set database pragmas")?;

        let state = Self { db };
        state.migrate()?;
        Ok(state)
    }

    #[cfg(test)]
    fn in_memory() -> Result<Self> {
        let db = Connection::open_in_memory().context("cannot open in-memory state database")?;
        let state = Self { db };
        state.migrate()?;
        Ok(state)
    }
}
