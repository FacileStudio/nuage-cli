use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::Path;

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

pub struct SyncState {
    db: Connection,
}

const FILE_COLUMNS: &str = "id, facile_id, name, local_path, hash, size, folder_id, \
     remote_updated_at, local_modified_at, synced_at";

const FOLDER_COLUMNS: &str =
    "id, facile_id, name, local_path, parent_id, remote_updated_at, synced_at";

fn map_file_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FileRecord> {
    Ok(FileRecord {
        id: row.get(0)?,
        facile_id: row.get(1)?,
        name: row.get(2)?,
        local_path: row.get(3)?,
        hash: row.get(4)?,
        size: row.get(5)?,
        folder_id: row.get(6)?,
        remote_updated_at: row.get(7)?,
        local_modified_at: row.get(8)?,
        synced_at: row.get(9)?,
    })
}

fn map_folder_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FolderRecord> {
    Ok(FolderRecord {
        id: row.get(0)?,
        facile_id: row.get(1)?,
        name: row.get(2)?,
        local_path: row.get(3)?,
        parent_id: row.get(4)?,
        remote_updated_at: row.get(5)?,
        synced_at: row.get(6)?,
    })
}

/// Number of consecutive failures after which a record is considered poisoned.
pub const QUARANTINE_THRESHOLD: u32 = 3;

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

    fn migrate(&self) -> Result<()> {
        self.db
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS files (
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
                CREATE INDEX IF NOT EXISTS idx_folders_facile_id ON folders(facile_id);",
            )
            .context("failed to run database migrations")?;
        Ok(())
    }

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

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_file(
        &self,
        facile_id: &str,
        name: &str,
        local_path: &str,
        hash: Option<&str>,
        size: Option<i64>,
        folder_id: Option<i64>,
        remote_updated_at: Option<&str>,
        local_modified_at: Option<i64>,
        synced_at: &str,
    ) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO files (facile_id, name, local_path, hash, size, folder_id, \
                 remote_updated_at, local_modified_at, synced_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
                 ON CONFLICT(local_path) DO UPDATE SET \
                 facile_id=?1, name=?2, hash=?4, size=?5, folder_id=?6, \
                 remote_updated_at=?7, local_modified_at=?8, synced_at=?9",
                params![
                    facile_id,
                    name,
                    local_path,
                    hash,
                    size,
                    folder_id,
                    remote_updated_at,
                    local_modified_at,
                    synced_at,
                ],
            )
            .context("failed to upsert file record")?;
        Ok(())
    }

    pub fn remove_file(&self, local_path: &str) -> Result<()> {
        self.db
            .execute("DELETE FROM files WHERE local_path = ?1", params![local_path])
            .context("failed to remove file record")?;
        Ok(())
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

    pub fn upsert_folder(
        &self,
        facile_id: &str,
        name: &str,
        local_path: &str,
        parent_id: Option<i64>,
        remote_updated_at: Option<&str>,
        synced_at: &str,
    ) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO folders (facile_id, name, local_path, parent_id, remote_updated_at, synced_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
                 ON CONFLICT(local_path) DO UPDATE SET \
                 facile_id=?1, name=?2, parent_id=?4, remote_updated_at=?5, synced_at=?6",
                params![facile_id, name, local_path, parent_id, remote_updated_at, synced_at],
            )
            .context("failed to upsert folder record")?;
        Ok(())
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

    pub fn file_count(&self) -> Result<i64> {
        self.db
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
            .context("failed to count files")
    }

    pub fn folder_count(&self) -> Result<i64> {
        self.db
            .query_row("SELECT COUNT(*) FROM folders", [], |row| row.get(0))
            .context("failed to count folders")
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

    pub fn get_file_by_facile_id(&self, facile_id: &str) -> Result<Option<FileRecord>> {
        let mut stmt = self
            .db
            .prepare(&format!(
                "SELECT {FILE_COLUMNS} FROM files WHERE facile_id = ?1"
            ))
            .context("failed to prepare file query by facile_id")?;

        let result = stmt.query_row(params![facile_id], map_file_row);

        match result {
            Ok(rec) => Ok(Some(rec)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context("failed to get file by facile_id"),
        }
    }

    pub fn get_folder_by_facile_id(&self, facile_id: &str) -> Result<Option<FolderRecord>> {
        let mut stmt = self
            .db
            .prepare(&format!(
                "SELECT {FOLDER_COLUMNS} FROM folders WHERE facile_id = ?1"
            ))
            .context("failed to prepare folder query by facile_id")?;

        let result = stmt.query_row(params![facile_id], map_folder_row);

        match result {
            Ok(rec) => Ok(Some(rec)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context("failed to get folder by facile_id"),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> SyncState {
        SyncState::in_memory().expect("in-memory state")
    }

    #[test]
    fn migrate_is_idempotent() {
        let state = state();
        state.migrate().expect("second migrate");
        state.migrate().expect("third migrate");
        assert_eq!(state.file_count().expect("file count"), 0);
    }

    #[test]
    fn failures_accumulate_until_threshold() {
        let state = state();
        assert!(!state.is_quarantined("f1").expect("check"));

        assert_eq!(state.record_failure("f1", "boom", "t1").expect("r1"), 1);
        assert!(!state.is_quarantined("f1").expect("check"));

        assert_eq!(state.record_failure("f1", "boom", "t2").expect("r2"), 2);
        assert!(!state.is_quarantined("f1").expect("check"));

        assert_eq!(state.record_failure("f1", "boom", "t3").expect("r3"), 3);
        assert!(state.is_quarantined("f1").expect("check"));

        let quarantined = state.list_quarantined().expect("list");
        assert_eq!(quarantined.len(), 1);
        assert_eq!(quarantined[0].facile_id, "f1");
        assert_eq!(quarantined[0].attempts, 3);
        assert_eq!(quarantined[0].first_failed_at, "t1");
        assert_eq!(quarantined[0].last_failed_at, "t3");

        assert_eq!(state.clear_all_quarantine().expect("clear all"), 1);
        assert!(state.list_quarantined().expect("list").is_empty());
    }

    #[test]
    fn clear_failure_resets_attempts() {
        let state = state();
        assert_eq!(state.record_failure("f1", "boom", "t1").expect("r1"), 1);
        assert_eq!(state.record_failure("f1", "boom", "t2").expect("r2"), 2);

        state.clear_failure("f1").expect("clear");
        assert!(!state.is_quarantined("f1").expect("check"));
        assert_eq!(state.record_failure("f1", "boom", "t3").expect("r3"), 1);
    }

    #[test]
    fn all_files_returns_every_tracked_path() {
        let state = state();
        for path in ["a/one.md", "a/b/two.md", "three.md"] {
            state
                .upsert_file("id", "name", path, None, None, None, None, None, "now")
                .expect("upsert file");
        }

        let mut paths: Vec<String> = state
            .all_files()
            .expect("all files")
            .into_iter()
            .map(|f| f.local_path)
            .collect();
        paths.sort();

        assert_eq!(paths, vec!["a/b/two.md", "a/one.md", "three.md"]);
    }

    #[test]
    fn upsert_file_updates_existing_path() {
        let state = state();
        state
            .upsert_file(
                "id1",
                "one.txt",
                "one.txt",
                Some("h1"),
                Some(1),
                None,
                None,
                None,
                "t1",
            )
            .expect("first upsert");
        state
            .upsert_file(
                "id2",
                "one.txt",
                "one.txt",
                Some("h2"),
                Some(2),
                None,
                None,
                None,
                "t2",
            )
            .expect("second upsert");

        assert_eq!(state.file_count().expect("count"), 1);
        let record = state.get_file("one.txt").expect("get").expect("some");
        assert_eq!(record.facile_id, "id2");
        assert_eq!(record.hash.as_deref(), Some("h2"));
        assert_eq!(record.size, Some(2));
        assert_eq!(record.synced_at, "t2");
    }
}
