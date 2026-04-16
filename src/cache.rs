//! Proposal cache: SQLite-backed store for completion snippets.
//!
//! Keys are (file_path, byte_range_start, byte_range_end, content_hash) to ensure
//! proposals are tied to specific file regions and invalidated on change.

use std::path::Path;

use anyhow::Context;
use rusqlite::{Connection, params};
use tokio::sync::Mutex;

/// Maximum lines per proposal (enforced at storage time).
pub const MAX_LINES: usize = 20;
/// Maximum bytes per proposal (enforced at storage time).
pub const MAX_BYTES: usize = 2048;

/// A cached proposal for a specific code region.
#[derive(Debug, Clone)]
pub struct Proposal {
    pub id: i64,
    pub file_path: String,
    pub byte_start: usize,
    pub byte_end: usize,
    pub content_hash: String,
    pub snippet: String,
    pub label: String,
    pub created_at: i64,
}

/// The proposal cache database (wrapped in Mutex for thread safety).
pub struct ProposalCache {
    conn: Mutex<Connection>,
}

impl ProposalCache {
    /// Open or create the proposal cache at the given path.
    pub fn open(db_path: &Path) -> anyhow::Result<Self> {
        let mut conn = Connection::open(db_path)
            .with_context(|| format!("failed to open proposal cache at {}", db_path.display()))?;

        // Initialize schema synchronously before wrapping in Mutex
        Self::init_schema(&mut conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Initialize the database schema.
    fn init_schema(conn: &mut Connection) -> anyhow::Result<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS proposals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file_path TEXT NOT NULL,
                byte_start INTEGER NOT NULL,
                byte_end INTEGER NOT NULL,
                content_hash TEXT NOT NULL,
                snippet TEXT NOT NULL,
                label TEXT NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            )",
            [],
        )?;

        // Index for fast lookup by file and byte range
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_proposals_lookup 
             ON proposals(file_path, byte_start, byte_end, content_hash)",
            [],
        )?;

        // Index for cleanup by age
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_proposals_created 
             ON proposals(created_at)",
            [],
        )?;

        Ok(())
    }

    /// Store a proposal, enforcing size caps.
    pub async fn store(
        &self,
        file_path: &str,
        byte_start: usize,
        byte_end: usize,
        content_hash: &str,
        snippet: &str,
        label: &str,
    ) -> anyhow::Result<i64> {
        // Enforce caps
        if snippet.lines().count() > MAX_LINES {
            anyhow::bail!(
                "snippet exceeds max lines ({} > {})",
                snippet.lines().count(),
                MAX_LINES
            );
        }
        if snippet.len() > MAX_BYTES {
            anyhow::bail!(
                "snippet exceeds max bytes ({} > {})",
                snippet.len(),
                MAX_BYTES
            );
        }

        let conn = self.conn.lock().await;
        conn.execute(
            "INSERT INTO proposals (file_path, byte_start, byte_end, content_hash, snippet, label)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT DO UPDATE SET
                snippet = excluded.snippet,
                label = excluded.label,
                created_at = unixepoch()",
            params![
                file_path,
                byte_start as i64,
                byte_end as i64,
                content_hash,
                snippet,
                label
            ],
        )?;

        Ok(conn.last_insert_rowid())
    }

    /// Lookup proposals for a specific file region.
    pub async fn lookup(
        &self,
        file_path: &str,
        byte_start: usize,
        byte_end: usize,
        content_hash: &str,
    ) -> anyhow::Result<Vec<Proposal>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT id, file_path, byte_start, byte_end, content_hash, snippet, label, created_at
             FROM proposals
             WHERE file_path = ?1 
               AND byte_start = ?2 
               AND byte_end = ?3 
               AND content_hash = ?4
             ORDER BY created_at DESC",
        )?;

        let proposals = stmt
            .query_map(
                params![file_path, byte_start as i64, byte_end as i64, content_hash],
                |row| {
                    Ok(Proposal {
                        id: row.get(0)?,
                        file_path: row.get(1)?,
                        byte_start: row.get::<_, i64>(2)? as usize,
                        byte_end: row.get::<_, i64>(3)? as usize,
                        content_hash: row.get(4)?,
                        snippet: row.get(5)?,
                        label: row.get(6)?,
                        created_at: row.get(7)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(proposals)
    }

    /// Lookup proposals that contain a given byte offset (for cursor-based lookup).
    pub async fn lookup_at_offset(
        &self,
        file_path: &str,
        offset: usize,
    ) -> anyhow::Result<Vec<Proposal>> {
        let conn = self.conn.lock().await;
        let mut stmt = conn.prepare(
            "SELECT id, file_path, byte_start, byte_end, content_hash, snippet, label, created_at
             FROM proposals
             WHERE file_path = ?1 
               AND byte_start <= ?2 
               AND byte_end >= ?2
             ORDER BY (byte_end - byte_start) ASC, created_at DESC",
        )?;

        let proposals = stmt
            .query_map(params![file_path, offset as i64], |row| {
                Ok(Proposal {
                    id: row.get(0)?,
                    file_path: row.get(1)?,
                    byte_start: row.get::<_, i64>(2)? as usize,
                    byte_end: row.get::<_, i64>(3)? as usize,
                    content_hash: row.get(4)?,
                    snippet: row.get(5)?,
                    label: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(proposals)
    }

    /// Delete proposals older than the given timestamp.
    pub async fn cleanup_old(&self, older_than: i64) -> anyhow::Result<usize> {
        let conn = self.conn.lock().await;
        let count = conn.execute(
            "DELETE FROM proposals WHERE created_at < ?1",
            params![older_than],
        )?;
        Ok(count)
    }

    /// Invalidate all proposals for a file (when file changes significantly).
    pub async fn invalidate_file(&self, file_path: &str) -> anyhow::Result<usize> {
        let conn = self.conn.lock().await;
        let count = conn.execute(
            "DELETE FROM proposals WHERE file_path = ?1",
            params![file_path],
        )?;
        Ok(count)
    }

    /// Get cache statistics.
    pub async fn stats(&self) -> anyhow::Result<CacheStats> {
        let conn = self.conn.lock().await;
        let total: i64 = conn.query_row("SELECT COUNT(*) FROM proposals", [], |row| row.get(0))?;

        let total_size: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(snippet)), 0) FROM proposals",
            [],
            |row| row.get(0),
        )?;

        // Use query_row with optional to handle NULL when there are no proposals
        let oldest: Option<i64> =
            conn.query_row("SELECT MIN(created_at) FROM proposals", [], |row| {
                let val: Option<i64> = row.get(0)?;
                Ok(val)
            })?;

        let newest: Option<i64> =
            conn.query_row("SELECT MAX(created_at) FROM proposals", [], |row| {
                let val: Option<i64> = row.get(0)?;
                Ok(val)
            })?;

        Ok(CacheStats {
            total_proposals: total as usize,
            total_bytes: total_size as usize,
            oldest_timestamp: oldest,
            newest_timestamp: newest,
        })
    }
}

/// Cache statistics for diagnostics.
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_proposals: usize,
    pub total_bytes: usize,
    pub oldest_timestamp: Option<i64>,
    pub newest_timestamp: Option<i64>,
}

/// Compute a simple content hash for invalidation.
pub fn compute_hash(content: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
