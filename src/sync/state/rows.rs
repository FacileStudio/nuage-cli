use super::{FileRecord, FolderRecord};

pub(super) const FILE_COLUMNS: &str = "id, facile_id, name, local_path, hash, size, folder_id, \
     remote_updated_at, local_modified_at, synced_at";

pub(super) const FOLDER_COLUMNS: &str =
    "id, facile_id, name, local_path, parent_id, remote_updated_at, synced_at";

pub(super) fn map_file_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FileRecord> {
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

pub(super) fn map_folder_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FolderRecord> {
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
