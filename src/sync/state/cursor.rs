use anyhow::{Context, Result};
use rusqlite::params;

use super::SyncState;

impl SyncState {
    pub fn get_cursor(&self) -> Result<Option<String>> {
        let mut stmt = self
            .db
            .prepare("SELECT value FROM sync_cursor WHERE key = 'last_sync'")
            .context("failed to prepare cursor query")?;

        let result = stmt.query_row([], |row| row.get::<_, String>(0));

        match result {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context("failed to get sync cursor"),
        }
    }

    pub fn set_cursor(&self, timestamp: &str) -> Result<()> {
        self.db
            .execute(
                "INSERT OR REPLACE INTO sync_cursor (key, value) VALUES ('last_sync', ?1)",
                params![timestamp],
            )
            .context("failed to set sync cursor")?;
        Ok(())
    }

    /// Forgets the incremental cursor so the next pass re-enumerates the whole server.
    pub fn clear_cursor(&self) -> Result<()> {
        self.db
            .execute("DELETE FROM sync_cursor WHERE key = 'last_sync'", [])
            .context("failed to clear sync cursor")?;
        Ok(())
    }
}
