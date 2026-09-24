use anyhow::{Context, Result};
use rusqlite::params;

use super::SyncState;

#[cfg(test)]
mod tests;

/// A remote file or folder is identified by its `facile_id`, but the tables are
/// keyed by `local_path`. These are the two operations that keep one object from
/// owning two rows: dropping the rows a moved object left behind, and shifting a
/// whole subtree of rows onto the path it now occupies.
///
/// Without them a re-parent or rename on the server wrote a second row for the
/// same `facile_id`, and every lookup by id answered with whichever row SQLite
/// happened to return first — usually the stale one. Files then resolved to a
/// directory the server no longer used, and the real one stayed empty.
impl SyncState {
    pub fn drop_folder_duplicates(&self, facile_id: &str, keep: &str) -> Result<Vec<String>> {
        self.drop_duplicates("folders", facile_id, keep)
    }

    pub fn drop_file_duplicates(&self, facile_id: &str, keep: &str) -> Result<Vec<String>> {
        self.drop_duplicates("files", facile_id, keep)
    }

    pub fn reparent_folders(&self, from: &str, to: &str) -> Result<()> {
        self.reparent("folders", from, to)
    }

    pub fn reparent_files(&self, from: &str, to: &str) -> Result<()> {
        self.reparent("files", from, to)
    }

    fn drop_duplicates(&self, table: &str, facile_id: &str, keep: &str) -> Result<Vec<String>> {
        let dropped = self.other_paths(table, facile_id, keep)?;
        if dropped.is_empty() {
            return Ok(dropped);
        }

        self.db
            .execute(
                &format!("DELETE FROM {table} WHERE facile_id = ?1 AND local_path <> ?2"),
                params![facile_id, keep],
            )
            .context("failed to drop duplicate rows")?;

        Ok(dropped)
    }

    fn other_paths(&self, table: &str, facile_id: &str, keep: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .db
            .prepare(&format!(
                "SELECT local_path FROM {table} WHERE facile_id = ?1 AND local_path <> ?2"
            ))
            .context("failed to prepare duplicate lookup")?;

        let rows = stmt
            .query_map(params![facile_id, keep], |row| row.get::<_, String>(0))
            .context("failed to list duplicate rows")?;

        let mut paths = Vec::new();
        for row in rows {
            paths.push(row.context("failed to read a duplicate row")?);
        }
        Ok(paths)
    }

    /// Moves `from` and everything under it onto `to`.
    ///
    /// The destination is cleared first: the subtree arriving there is the
    /// authoritative record of those paths, so any row left sitting at or under
    /// `to` is a stale duplicate and goes. That keeps the update below free of
    /// `local_path` uniqueness collisions.
    fn reparent(&self, table: &str, from: &str, to: &str) -> Result<()> {
        self.db
            .execute(
                &format!(
                    "DELETE FROM {table} WHERE {} AND NOT {}",
                    at_or_under("?2"),
                    at_or_under("?1"),
                ),
                params![from, to],
            )
            .context("failed to clear the destination of a moved subtree")?;

        self.db
            .execute(
                &format!(
                    "UPDATE {table} SET local_path = ?2 || substr(local_path, length(?1) + 1) \
                     WHERE {}",
                    at_or_under("?1"),
                ),
                params![from, to],
            )
            .context("failed to shift a moved subtree")?;

        Ok(())
    }
}

/// Matches `local_path` against a bound parameter, exactly or as an ancestor
/// directory of it. `substr` rather than `LIKE` so a path holding `%` or `_` is
/// matched literally.
fn at_or_under(param: &str) -> String {
    format!(
        "(local_path = {param} OR substr(local_path, 1, length({param}) + 1) = {param} || '/')"
    )
}
