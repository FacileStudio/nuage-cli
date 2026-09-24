use anyhow::{Context, Result};

use super::SyncState;

const SCHEMA_SQL: &str = "CREATE TABLE IF NOT EXISTS files (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    facile_id TEXT NOT NULL,
                    name TEXT NOT NULL,
                    local_path TEXT NOT NULL UNIQUE,
                    hash TEXT,
                    size INTEGER,
                    folder_id INTEGER,
                    remote_updated_at TEXT,
                    local_modified_at INTEGER,
                    synced_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS folders (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    facile_id TEXT NOT NULL,
                    name TEXT NOT NULL,
                    local_path TEXT NOT NULL UNIQUE,
                    parent_id INTEGER,
                    remote_updated_at TEXT,
                    synced_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS sync_cursor (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS sync_quarantine (
                    facile_id TEXT PRIMARY KEY,
                    reason TEXT NOT NULL,
                    attempts INTEGER NOT NULL,
                    first_failed_at TEXT NOT NULL,
                    last_failed_at TEXT NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_files_facile_id ON files(facile_id);
                CREATE INDEX IF NOT EXISTS idx_files_hash ON files(hash);
                CREATE INDEX IF NOT EXISTS idx_folders_facile_id ON folders(facile_id);";

impl SyncState {
    pub(super) fn migrate(&self) -> Result<()> {
        self.db
            .execute_batch(SCHEMA_SQL)
            .context("failed to run database migrations")?;
        Ok(())
    }
}
