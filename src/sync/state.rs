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
                );",
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

    pub fn get_file(&self, local_path: &str) -> Result<Option<FileRecord>> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT id, facile_id, name, local_path, hash, size, folder_id, \
                 remote_updated_at, local_modified_at, synced_at \
                 FROM files WHERE local_path = ?1",
            )
            .context("failed to prepare file query")?;

        let result = stmt.query_row(params![local_path], |row| {
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
        });

        match result {
            Ok(rec) => Ok(Some(rec)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context("failed to get file record"),
        }
    }

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
            .prepare(
                "SELECT id, facile_id, name, local_path, parent_id, remote_updated_at, synced_at \
                 FROM folders WHERE local_path = ?1",
            )
            .context("failed to prepare folder query")?;

        let result = stmt.query_row(params![local_path], |row| {
            Ok(FolderRecord {
                id: row.get(0)?,
                facile_id: row.get(1)?,
                name: row.get(2)?,
                local_path: row.get(3)?,
                parent_id: row.get(4)?,
                remote_updated_at: row.get(5)?,
                synced_at: row.get(6)?,
            })
        });

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
            .prepare(
                "SELECT id, facile_id, name, local_path, hash, size, folder_id, \
                 remote_updated_at, local_modified_at, synced_at \
                 FROM files WHERE hash = ?1 LIMIT 1",
            )
            .context("failed to prepare file query by hash")?;

        let result = stmt.query_row(params![hash], |row| {
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
        });

        match result {
            Ok(rec) => Ok(Some(rec)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context("failed to get file by hash"),
        }
    }

    pub fn get_file_by_facile_id(&self, facile_id: &str) -> Result<Option<FileRecord>> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT id, facile_id, name, local_path, hash, size, folder_id, \
                 remote_updated_at, local_modified_at, synced_at \
                 FROM files WHERE facile_id = ?1",
            )
            .context("failed to prepare file query by facile_id")?;

        let result = stmt.query_row(params![facile_id], |row| {
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
        });

        match result {
            Ok(rec) => Ok(Some(rec)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context("failed to get file by facile_id"),
        }
    }

    pub fn get_folder_by_facile_id(&self, facile_id: &str) -> Result<Option<FolderRecord>> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT id, facile_id, name, local_path, parent_id, remote_updated_at, synced_at \
                 FROM folders WHERE facile_id = ?1",
            )
            .context("failed to prepare folder query by facile_id")?;

        let result = stmt.query_row(params![facile_id], |row| {
            Ok(FolderRecord {
                id: row.get(0)?,
                facile_id: row.get(1)?,
                name: row.get(2)?,
                local_path: row.get(3)?,
                parent_id: row.get(4)?,
                remote_updated_at: row.get(5)?,
                synced_at: row.get(6)?,
            })
        });

        match result {
            Ok(rec) => Ok(Some(rec)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context("failed to get folder by facile_id"),
        }
    }
}
