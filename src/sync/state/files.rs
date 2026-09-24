use anyhow::{Context, Result};
use rusqlite::params;

use super::rows::{map_file_row, FILE_COLUMNS};
use super::{FileRecord, SyncState, UpsertFile};

impl SyncState {
    pub fn get_file(&self, local_path: &str) -> Result<Option<FileRecord>> {
        let mut stmt = self
            .db
            .prepare(&format!(
                "SELECT {FILE_COLUMNS} FROM files WHERE local_path = ?1"
            ))
            .context("failed to prepare file query")?;

        let result = stmt.query_row(params![local_path], map_file_row);

        match result {
            Ok(rec) => Ok(Some(rec)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context("failed to get file record"),
        }
    }

    /// Records a file at `record.local_path` and returns the paths it was tracked
    /// at before, if any.
    ///
    /// A remote file owns exactly one row: recording it somewhere else moves its
    /// identity there rather than adding a second row beside the first.
    pub fn upsert_file(&self, record: &UpsertFile) -> Result<Vec<String>> {
        let replaced = self.drop_file_duplicates(&record.facile_id, &record.local_path)?;

        self.db
            .execute(
                "INSERT INTO files (facile_id, name, local_path, hash, size, folder_id, \
                 remote_updated_at, local_modified_at, synced_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
                 ON CONFLICT(local_path) DO UPDATE SET \
                 facile_id=?1, name=?2, hash=?4, size=?5, folder_id=?6, \
                 remote_updated_at=?7, local_modified_at=?8, synced_at=?9",
                params![
                    record.facile_id,
                    record.name,
                    record.local_path,
                    record.hash,
                    record.size,
                    record.folder_id,
                    record.remote_updated_at,
                    record.local_modified_at,
                    record.synced_at,
                ],
            )
            .context("failed to upsert file record")?;

        Ok(replaced)
    }

    pub fn remove_file(&self, local_path: &str) -> Result<()> {
        self.db
            .execute(
                "DELETE FROM files WHERE local_path = ?1",
                params![local_path],
            )
            .context("failed to remove file record")?;
        Ok(())
    }

    pub fn file_count(&self) -> Result<i64> {
        self.db
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
            .context("failed to count files")
    }

    pub fn get_file_by_hash(&self, hash: &str) -> Result<Option<FileRecord>> {
        let mut stmt = self
            .db
            .prepare(&format!(
                "SELECT {FILE_COLUMNS} FROM files WHERE hash = ?1 LIMIT 1"
            ))
            .context("failed to prepare file query by hash")?;

        let result = stmt.query_row(params![hash], map_file_row);

        match result {
            Ok(rec) => Ok(Some(rec)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context("failed to get file by hash"),
        }
    }

    /// The oldest row for a file id, which is the path the file was first written
    /// to. The `ORDER BY` makes that deterministic rather than whichever row the
    /// query planner reached first. See [`SyncState::get_folder_by_facile_id`].
    pub fn get_file_by_facile_id(&self, facile_id: &str) -> Result<Option<FileRecord>> {
        let mut stmt = self
            .db
            .prepare(&format!(
                "SELECT {FILE_COLUMNS} FROM files WHERE facile_id = ?1 ORDER BY id LIMIT 1"
            ))
            .context("failed to prepare file query by facile_id")?;

        let result = stmt.query_row(params![facile_id], map_file_row);

        match result {
            Ok(rec) => Ok(Some(rec)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context("failed to get file by facile_id"),
        }
    }

    /// Returns every tracked file record, for the startup reconcile pass.
    pub fn all_files(&self) -> Result<Vec<FileRecord>> {
        let mut stmt = self
            .db
            .prepare(&format!("SELECT {FILE_COLUMNS} FROM files"))
            .context("failed to prepare all files query")?;

        let rows = stmt
            .query_map([], map_file_row)
            .context("failed to query all file records")?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.context("failed to read file record")?);
        }
        Ok(records)
    }
}
