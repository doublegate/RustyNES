//! Optional `SQLite` backing for the script `userdata.*` KV store (v1.8.9).
//!
//! Behind the off-by-default `script-sqlite` feature. The live store stays the
//! in-memory map the Lua API reads / writes each frame; this persists a snapshot
//! to a real on-disk database so a script's `userdata` survives across runs and is
//! inspectable with external `SQLite` tooling. The default / shipped build never
//! compiles this module, so it stays dependency-free and byte-identical.
//!
//! The interface deliberately mirrors
//! [`ScriptEngine::userdata_snapshot`](crate::ScriptEngine::userdata_snapshot) /
//! [`userdata_restore`](crate::ScriptEngine::userdata_restore) — a flat
//! `Vec<(String, String)>` — so the bridge is a one-liner each way.

use rusqlite::{Connection, params};

/// A `SQLite`-backed key/value table for script userdata
/// (`key TEXT PRIMARY KEY -> value TEXT`).
pub struct SqliteKv {
    conn: Connection,
}

impl SqliteKv {
    /// Open (creating if absent) the userdata database at `path`, ensuring the
    /// `userdata` table exists.
    ///
    /// # Errors
    /// Propagates any `SQLite` open / schema error.
    pub fn open<P: AsRef<std::path::Path>>(path: P) -> rusqlite::Result<Self> {
        Self::init(Connection::open(path)?)
    }

    /// Open an in-memory database (tests / ephemeral use).
    ///
    /// # Errors
    /// Propagates any `SQLite` error.
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS userdata (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
            [],
        )?;
        Ok(Self { conn })
    }

    /// Replace the entire store with `pairs` (the `userdata_snapshot` shape) in one
    /// transaction. A duplicate key keeps the last value seen.
    ///
    /// # Errors
    /// Propagates any `SQLite` error; the transaction rolls back on failure.
    pub fn save_pairs(&mut self, pairs: &[(String, String)]) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM userdata", [])?;
        {
            let mut stmt =
                tx.prepare("INSERT OR REPLACE INTO userdata (key, value) VALUES (?1, ?2)")?;
            for (k, v) in pairs {
                stmt.execute(params![k, v])?;
            }
        }
        tx.commit()
    }

    /// Load all pairs (the `userdata_restore` shape), ordered by key for a stable,
    /// deterministic result.
    ///
    /// # Errors
    /// Propagates any `SQLite` error.
    pub fn load_pairs(&self) -> rusqlite::Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM userdata ORDER BY key")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(k: &str, v: &str) -> (String, String) {
        (k.to_owned(), v.to_owned())
    }

    #[test]
    fn round_trips_pairs_key_ordered() {
        let mut kv = SqliteKv::open_in_memory().unwrap();
        kv.save_pairs(&[pair("b", "2"), pair("a", "1")]).unwrap();
        assert_eq!(
            kv.load_pairs().unwrap(),
            vec![pair("a", "1"), pair("b", "2")]
        );
    }

    #[test]
    fn save_replaces_previous_contents() {
        let mut kv = SqliteKv::open_in_memory().unwrap();
        kv.save_pairs(&[pair("x", "old")]).unwrap();
        kv.save_pairs(&[pair("y", "new")]).unwrap();
        assert_eq!(kv.load_pairs().unwrap(), vec![pair("y", "new")]);
    }

    #[test]
    fn empty_save_clears_the_store() {
        let mut kv = SqliteKv::open_in_memory().unwrap();
        kv.save_pairs(&[pair("k", "v")]).unwrap();
        kv.save_pairs(&[]).unwrap();
        assert!(kv.load_pairs().unwrap().is_empty());
    }
}
