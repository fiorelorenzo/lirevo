use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

pub mod history;

/// Ordered, append-only migration list. NEVER edit a released migration; add a
/// new one. `to_latest` applies only the not-yet-applied migrations and bumps
/// `PRAGMA user_version`, so an upgraded app self-heals its schema.
fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(include_str!("migrations/001_dictations.sql"))])
}

/// The app's generic local database. One connection behind a mutex (SQLite
/// serializes writes; a desktop app needs no pool). Consumers live in submodules
/// (`db::history`, future features).
pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    /// Open (creating if absent) and migrate the DB at `path`.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let mut conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrations()
            .to_latest(&mut conn)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// In-memory DB (migrated). Used by the startup fallback and tests.
    pub fn memory() -> rusqlite::Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        migrations()
            .to_latest(&mut conn)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub(crate) fn with_conn<T>(
        &self,
        f: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> rusqlite::Result<T> {
        let guard = self.conn.lock().expect("db mutex poisoned");
        f(&guard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_validate() {
        assert!(migrations().validate().is_ok());
    }

    #[test]
    fn fresh_db_is_at_latest_and_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrations().to_latest(&mut conn).unwrap();
        let v1: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert!(v1 >= 1);
        migrations().to_latest(&mut conn).unwrap();
        let v2: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v1, v2);
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='dictations'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn partial_version_applies_only_missing() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 0).unwrap();
        migrations().to_latest(&mut conn).unwrap();
        let v: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert!(v >= 1);
    }
}
