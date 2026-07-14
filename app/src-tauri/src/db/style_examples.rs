use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::Db;

/// Insert payload (every column except `id`).
#[derive(Debug, Clone)]
pub struct NewStyleExample {
    pub dictation_id: Option<i64>,
    pub context_key: Option<String>,
    pub target_bundle: Option<String>,
    pub raw_text: String,
    pub final_text: String,
    pub edit_distance_ratio: Option<f64>,
    pub source: String,
    pub pinned: bool,
    pub use_count: i64,
    pub last_used_at: Option<i64>,
    pub created_at: i64,
}

/// Full row for a saved style example.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleExample {
    pub id: i64,
    pub dictation_id: Option<i64>,
    pub context_key: Option<String>,
    pub target_bundle: Option<String>,
    pub raw_text: String,
    pub final_text: String,
    pub edit_distance_ratio: Option<f64>,
    pub source: String,
    pub pinned: bool,
    pub use_count: i64,
    pub last_used_at: Option<i64>,
    pub created_at: i64,
}

fn row_to_style_example(r: &rusqlite::Row) -> rusqlite::Result<StyleExample> {
    Ok(StyleExample {
        id: r.get(0)?,
        dictation_id: r.get(1)?,
        context_key: r.get(2)?,
        target_bundle: r.get(3)?,
        raw_text: r.get(4)?,
        final_text: r.get(5)?,
        edit_distance_ratio: r.get(6)?,
        source: r.get(7)?,
        pinned: r.get::<_, i64>(8)? != 0,
        use_count: r.get(9)?,
        last_used_at: r.get(10)?,
        created_at: r.get(11)?,
    })
}

const SELECT_COLUMNS: &str = "id, dictation_id, context_key, target_bundle, raw_text, final_text, \
     edit_distance_ratio, source, pinned, use_count, last_used_at, created_at";

pub fn insert(db: &Db, e: &NewStyleExample) -> rusqlite::Result<i64> {
    db.with_conn(|c| {
        c.execute(
            "INSERT INTO style_examples (dictation_id, context_key, target_bundle, raw_text,
                final_text, edit_distance_ratio, source, pinned, use_count, last_used_at, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                e.dictation_id,
                e.context_key,
                e.target_bundle,
                e.raw_text,
                e.final_text,
                e.edit_distance_ratio,
                e.source,
                i64::from(e.pinned),
                e.use_count,
                e.last_used_at,
                e.created_at,
            ],
        )?;
        Ok(c.last_insert_rowid())
    })
}

#[allow(dead_code)] // consumer lands with the Settings -> Writing Style page (#88)
pub fn get(db: &Db, id: i64) -> rusqlite::Result<Option<StyleExample>> {
    db.with_conn(|c| {
        c.query_row(
            &format!("SELECT {SELECT_COLUMNS} FROM style_examples WHERE id = ?1"),
            params![id],
            row_to_style_example,
        )
        .optional()
    })
}

#[allow(dead_code)] // consumer lands with the Settings -> Writing Style page (#88)
pub fn delete(db: &Db, id: i64) -> rusqlite::Result<()> {
    db.with_conn(|c| {
        c.execute("DELETE FROM style_examples WHERE id = ?1", params![id])
            .map(|_| ())
    })
}

/// All examples for a given app, newest first.
#[allow(dead_code)] // consumer lands with the Settings -> Writing Style page (#88)
pub fn list_for_bundle(db: &Db, target_bundle: &str) -> rusqlite::Result<Vec<StyleExample>> {
    db.with_conn(|c| {
        let mut stmt = c.prepare(&format!(
            "SELECT {SELECT_COLUMNS} FROM style_examples WHERE target_bundle = ?1 ORDER BY created_at DESC"
        ))?;
        let rows = stmt
            .query_map(params![target_bundle], row_to_style_example)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })
}

/// Ranked top-`k` retrieval for the cleanup prompt: pinned examples first,
/// then by most-recently-used, then by most-used. Scoped to `target_bundle`
/// and, when provided, further scoped to `context_key` (recipient-level).
pub fn top_k(
    db: &Db,
    target_bundle: &str,
    context_key: Option<&str>,
    k: usize,
) -> rusqlite::Result<Vec<StyleExample>> {
    let k = k as i64;
    db.with_conn(|c| {
        const ORDER: &str = "ORDER BY pinned DESC, last_used_at DESC, use_count DESC LIMIT ?";
        if let Some(ctx) = context_key {
            let mut stmt = c.prepare(&format!(
                "SELECT {SELECT_COLUMNS} FROM style_examples \
                 WHERE target_bundle = ? AND context_key = ? {ORDER}"
            ))?;
            let rows = stmt
                .query_map(params![target_bundle, ctx, k], row_to_style_example)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        } else {
            let mut stmt = c.prepare(&format!(
                "SELECT {SELECT_COLUMNS} FROM style_examples WHERE target_bundle = ? {ORDER}"
            ))?;
            let rows = stmt
                .query_map(params![target_bundle, k], row_to_style_example)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        }
    })
}

/// Records that an example was actually used in a cleanup prompt: bumps
/// `use_count` and refreshes `last_used_at` so future `top_k` ranking reflects
/// real usage. `now` is the caller-supplied current timestamp (Unix seconds),
/// matching the rest of this module's caller-owns-the-clock convention.
pub fn touch_use(db: &Db, id: i64, now: i64) -> rusqlite::Result<()> {
    db.with_conn(|c| {
        c.execute(
            "UPDATE style_examples SET use_count = use_count + 1, last_used_at = ?2 WHERE id = ?1",
            params![id, now],
        )
        .map(|_| ())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(target_bundle: &str, created_at: i64) -> NewStyleExample {
        NewStyleExample {
            dictation_id: None,
            context_key: None,
            target_bundle: Some(target_bundle.into()),
            raw_text: "raw".into(),
            final_text: "final".into(),
            edit_distance_ratio: Some(0.2),
            source: "manual_pin".into(),
            pinned: false,
            use_count: 0,
            last_used_at: None,
            created_at,
        }
    }

    #[test]
    fn insert_get_delete_roundtrip() {
        let db = Db::memory().unwrap();
        let id = insert(&db, &sample("com.apple.mail", 1000)).unwrap();

        let got = get(&db, id).unwrap().unwrap();
        assert_eq!(got.target_bundle.as_deref(), Some("com.apple.mail"));
        assert_eq!(got.raw_text, "raw");
        assert_eq!(got.final_text, "final");
        assert!(!got.pinned);
        assert_eq!(got.use_count, 0);
        assert_eq!(got.last_used_at, None);

        delete(&db, id).unwrap();
        assert!(get(&db, id).unwrap().is_none());
    }

    #[test]
    fn list_for_bundle_scopes_and_orders_newest_first() {
        let db = Db::memory().unwrap();
        insert(&db, &sample("com.apple.mail", 1)).unwrap();
        let id2 = insert(&db, &sample("com.apple.mail", 2)).unwrap();
        insert(&db, &sample("com.other.app", 3)).unwrap();

        let rows = list_for_bundle(&db, "com.apple.mail").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, id2);
    }

    #[test]
    fn touch_use_bumps_count_and_last_used_at() {
        let db = Db::memory().unwrap();
        let id = insert(&db, &sample("com.apple.mail", 1)).unwrap();

        touch_use(&db, id, 500).unwrap();
        let got = get(&db, id).unwrap().unwrap();
        assert_eq!(got.use_count, 1);
        assert_eq!(got.last_used_at, Some(500));

        touch_use(&db, id, 900).unwrap();
        let got = get(&db, id).unwrap().unwrap();
        assert_eq!(got.use_count, 2);
        assert_eq!(got.last_used_at, Some(900));
    }

    /// `top_k` orders pinned-first, then most-recently-used, then most-used,
    /// and respects both the `k` cap and `target_bundle` scoping.
    #[test]
    fn top_k_orders_pinned_first_then_recency_then_use_count() {
        let db = Db::memory().unwrap();

        let mut unpinned_old = sample("com.apple.mail", 1);
        unpinned_old.last_used_at = Some(100);
        unpinned_old.use_count = 5;
        let unpinned_old_id = insert(&db, &unpinned_old).unwrap();

        let mut unpinned_recent = sample("com.apple.mail", 2);
        unpinned_recent.last_used_at = Some(300);
        unpinned_recent.use_count = 1;
        let unpinned_recent_id = insert(&db, &unpinned_recent).unwrap();

        let mut pinned = sample("com.apple.mail", 3);
        pinned.pinned = true;
        pinned.last_used_at = Some(50);
        pinned.use_count = 0;
        let pinned_id = insert(&db, &pinned).unwrap();

        // Different app entirely — must never surface in this bundle's results.
        insert(&db, &sample("com.other.app", 4)).unwrap();

        let rows = top_k(&db, "com.apple.mail", None, 10).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(
            rows[0].id, pinned_id,
            "pinned sorts first regardless of recency/use"
        );
        assert_eq!(rows[1].id, unpinned_recent_id, "then most-recently-used");
        assert_eq!(rows[2].id, unpinned_old_id);

        let capped = top_k(&db, "com.apple.mail", None, 2).unwrap();
        assert_eq!(capped.len(), 2, "k caps the result count");
        assert_eq!(capped[0].id, pinned_id);
        assert_eq!(capped[1].id, unpinned_recent_id);
    }

    #[test]
    fn top_k_scopes_by_context_key_when_present() {
        let db = Db::memory().unwrap();

        let mut work = sample("com.apple.mail", 1);
        work.context_key = Some("boss@example.com".into());
        let work_id = insert(&db, &work).unwrap();

        let mut personal = sample("com.apple.mail", 2);
        personal.context_key = Some("friend@example.com".into());
        insert(&db, &personal).unwrap();

        let mut no_context = sample("com.apple.mail", 3);
        no_context.context_key = None;
        insert(&db, &no_context).unwrap();

        let rows = top_k(&db, "com.apple.mail", Some("boss@example.com"), 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, work_id);

        // No context_key filter falls back to bundle-wide scoping.
        let all = top_k(&db, "com.apple.mail", None, 10).unwrap();
        assert_eq!(all.len(), 3);
    }
}
