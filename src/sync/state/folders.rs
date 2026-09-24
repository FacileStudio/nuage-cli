use anyhow::{Context, Result};
use rusqlite::params;

use super::rows::{map_folder_row, FOLDER_COLUMNS};
use super::{FolderRecord, SyncState, UpsertFolder};

impl SyncState {
    pub fn all_folders(&self) -> Result<Vec<FolderRecord>> {
        let mut stmt = self
            .db
            .prepare(&format!(
                "SELECT {FOLDER_COLUMNS} FROM folders \
                 ORDER BY (LENGTH(local_path) - LENGTH(REPLACE(local_path, '/', ''))) DESC, \
                 local_path DESC"
            ))
            .context("failed to prepare folder listing")?;

        let rows = stmt
            .query_map([], map_folder_row)
            .context("failed to list folders")?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.context("failed to read folder record")?);
        }
        Ok(out)
    }

    pub fn get_folder(&self, local_path: &str) -> Result<Option<FolderRecord>> {
        let mut stmt = self
            .db
            .prepare(&format!(
                "SELECT {FOLDER_COLUMNS} FROM folders WHERE local_path = ?1"
            ))
            .context("failed to prepare folder query")?;

        let result = stmt.query_row(params![local_path], map_folder_row);

        match result {
            Ok(rec) => Ok(Some(rec)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context("failed to get folder record"),
        }
    }

    /// Records a folder at `record.local_path` and returns the paths it was
    /// tracked at before, if any.
    ///
    /// A remote folder owns exactly one row: recording it somewhere else moves
    /// its identity there rather than adding a second row beside the first.
    pub fn upsert_folder(&self, record: &UpsertFolder) -> Result<Vec<String>> {
        let replaced =
            self.drop_folder_duplicates(&record.facile_id, &record.local_path)?;

        self.db
            .execute(
                "INSERT INTO folders (facile_id, name, local_path, parent_id, remote_updated_at, synced_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
                 ON CONFLICT(local_path) DO UPDATE SET \
                 facile_id=?1, name=?2, parent_id=?4, remote_updated_at=?5, synced_at=?6",
                params![
                    record.facile_id,
                    record.name,
                    record.local_path,
                    record.parent_id,
                    record.remote_updated_at,
                    record.synced_at,
                ],
            )
            .context("failed to upsert folder record")?;

        Ok(replaced)
    }

    pub fn remove_folder(&self, local_path: &str) -> Result<()> {
        self.db
            .execute(
                "DELETE FROM folders WHERE local_path = ?1",
                params![local_path],
            )
            .context("failed to remove folder record")?;
        Ok(())
    }

    pub fn folder_count(&self) -> Result<i64> {
        self.db
            .query_row("SELECT COUNT(*) FROM folders", [], |row| row.get(0))
            .context("failed to count folders")
    }

    /// The oldest row for a folder id, which is the path the folder was first
    /// materialised at and therefore where its content lives.
    ///
    /// The `ORDER BY` is what makes that deterministic. Unordered, a duplicate
    /// row for the same id made this a coin flip, and the loser of that flip was
    /// the path every file under the folder was then written to.
    pub fn get_folder_by_facile_id(&self, facile_id: &str) -> Result<Option<FolderRecord>> {
        let mut stmt = self
            .db
            .prepare(&format!(
                "SELECT {FOLDER_COLUMNS} FROM folders WHERE facile_id = ?1 ORDER BY id LIMIT 1"
            ))
            .context("failed to prepare folder query by facile_id")?;

        let result = stmt.query_row(params![facile_id], map_folder_row);

        match result {
            Ok(rec) => Ok(Some(rec)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context("failed to get folder by facile_id"),
        }
    }
}
