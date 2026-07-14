use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

pub mod history;
pub mod style_examples;

/// Ordered, append-only migration list. NEVER edit a released migration; add a
/// new one. `to_latest` applies only the not-yet-applied migrations and bumps
/// `PRAGMA user_version`, so an upgraded app self-heals its schema.
fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("migrations/001_dictations.sql")),
        M::up(include_str!("migrations/002_smart_routing.sql")),
        M::up(include_str!("migrations/003_style_examples.sql")),
        M::up(include_str!("migrations/004_dictations_context_key.sql")),
    ])
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

    /// Open the file DB, falling back to an in-memory DB (logged) if that fails,
    /// so a broken/locked file never prevents the app from starting.
    pub fn open_or_memory(path: &Path) -> Self {
        match Self::open(path) {
            Ok(db) => db,
            Err(e) => {
                tracing::error!(
                    ?e,
                    "failed to open data.db; using in-memory history (not persisted)"
                );
                Self::memory().expect("in-memory DB must open")
            }
        }
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

    #[test]
    fn migration_002_adds_smart_routing_columns() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrations().to_latest(&mut conn).unwrap();
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(dictations)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        for expected in [
            "input_device",
            "smart_routing_enabled",
            "smart_routing_applied",
        ] {
            assert!(
                cols.iter().any(|c| c == expected),
                "missing column {expected}"
            );
        }
    }

    #[test]
    fn migration_003_creates_style_examples_table() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrations().to_latest(&mut conn).unwrap();
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(style_examples)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        for expected in [
            "id",
            "dictation_id",
            "context_key",
            "target_bundle",
            "raw_text",
            "final_text",
            "edit_distance_ratio",
            "source",
            "pinned",
            "use_count",
            "last_used_at",
            "created_at",
        ] {
            assert!(
                cols.iter().any(|c| c == expected),
                "missing column {expected}"
            );
        }
    }

    #[test]
    fn migration_004_adds_dictations_context_key_column() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrations().to_latest(&mut conn).unwrap();
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(dictations)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert!(cols.iter().any(|c| c == "context_key"));
    }

    #[test]
    fn style_example_dictation_id_nulled_on_parent_delete() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrations().to_latest(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO dictations (id, created_at, stt_model, raw_text, stt_ms, cleaned_text, cleanup_status, inject_method, total_ms) \
             VALUES (1, 0, 'stt', 'raw', 0, 'cleaned', 'ok', 'paste', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO style_examples (dictation_id, target_bundle, raw_text, final_text, source, created_at) \
             VALUES (1, 'com.example.app', 'raw', 'final', 'manual_pin', 0)",
            [],
        )
        .unwrap();

        conn.execute("DELETE FROM dictations WHERE id = 1", [])
            .unwrap();

        let dictation_id: Option<i64> = conn
            .query_row(
                "SELECT dictation_id FROM style_examples WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(dictation_id, None);
    }
}
