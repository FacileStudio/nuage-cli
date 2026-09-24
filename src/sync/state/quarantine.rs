use anyhow::{Context, Result};
use rusqlite::params;

use super::{QuarantineRecord, SyncState};

/// Number of consecutive failures after which a record is considered poisoned.
pub const QUARANTINE_THRESHOLD: u32 = 3;

impl SyncState {
    /// Records a sync failure for a record and returns the new attempt count.
    pub fn record_failure(&self, facile_id: &str, reason: &str, now: &str) -> Result<u32> {
        self.db
            .execute(
                "INSERT INTO sync_quarantine \
                 (facile_id, reason, attempts, first_failed_at, last_failed_at) \
                 VALUES (?1, ?2, 1, ?3, ?3) \
                 ON CONFLICT(facile_id) DO UPDATE SET \
                 reason=?2, attempts=attempts+1, last_failed_at=?3",
                params![facile_id, reason, now],
            )
            .context("failed to record sync failure")?;

        let attempts: i64 = self
            .db
            .query_row(
                "SELECT attempts FROM sync_quarantine WHERE facile_id = ?1",
                params![facile_id],
                |row| row.get(0),
            )
            .context("failed to read failure attempt count")?;

        Ok(attempts.max(0) as u32)
    }

    /// Returns true when the record has failed at least `QUARANTINE_THRESHOLD` times.
    pub fn is_quarantined(&self, facile_id: &str) -> Result<bool> {
        let result = self.db.query_row(
            "SELECT attempts FROM sync_quarantine WHERE facile_id = ?1",
            params![facile_id],
            |row| row.get::<_, i64>(0),
        );

        match result {
            Ok(attempts) => Ok(attempts.max(0) as u32 >= QUARANTINE_THRESHOLD),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
            Err(e) => Err(e).context("failed to check quarantine status"),
        }
    }

    /// Clears any recorded failures for a record after a successful sync.
    pub fn clear_failure(&self, facile_id: &str) -> Result<()> {
        self.db
            .execute(
                "DELETE FROM sync_quarantine WHERE facile_id = ?1",
                params![facile_id],
            )
            .context("failed to clear sync failure")?;
        Ok(())
    }

    /// Returns every record currently at or above the quarantine threshold.
    pub fn list_quarantined(&self) -> Result<Vec<QuarantineRecord>> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT facile_id, reason, attempts, first_failed_at, last_failed_at \
                 FROM sync_quarantine WHERE attempts >= ?1 ORDER BY last_failed_at DESC",
            )
            .context("failed to prepare quarantine query")?;

        let rows = stmt
            .query_map(params![QUARANTINE_THRESHOLD], |row| {
                Ok(QuarantineRecord {
                    facile_id: row.get(0)?,
                    reason: row.get(1)?,
                    attempts: row.get::<_, i64>(2)?.max(0) as u32,
                    first_failed_at: row.get(3)?,
                    last_failed_at: row.get(4)?,
                })
            })
            .context("failed to query quarantined records")?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.context("failed to read quarantine record")?);
        }
        Ok(records)
    }

    /// Empties the quarantine table and returns how many rows were removed.
    pub fn clear_all_quarantine(&self) -> Result<usize> {
        self.db
            .execute("DELETE FROM sync_quarantine", [])
            .context("failed to clear quarantine table")
    }
}
