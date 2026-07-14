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

/// Per-`(target_bundle, context_key)` row cap, enforced after every insert so
/// a heavily-dictated app/recipient doesn't grow `style_examples` unbounded.
/// Pinned rows are never auto-evicted (see `evict_over_cap`), so a context
/// that is entirely pinned may legitimately stay over cap.
const MAX_ROWS_PER_CONTEXT: i64 = 50;

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
        let id = c.last_insert_rowid();
        evict_over_cap(c, e.target_bundle.as_deref(), e.context_key.as_deref())?;
        Ok(id)
    })
}

/// Enforces [`MAX_ROWS_PER_CONTEXT`] for the `(target_bundle, context_key)`
/// scope of a just-inserted row: when the scope holds more than the cap,
/// deletes the excess non-pinned rows ordered by least-recently-used
/// (`last_used_at ASC`, then `use_count ASC` as a tiebreak for rows that
/// have never been used). Pinned rows are excluded from the eviction
/// candidates entirely. Uses `IS` rather than `=` throughout so `NULL`
/// `context_key`/`target_bundle` values match each other correctly (`=`
/// against `NULL` is never true in SQL).
fn evict_over_cap(
    c: &rusqlite::Connection,
    target_bundle: Option<&str>,
    context_key: Option<&str>,
) -> rusqlite::Result<()> {
    let count: i64 = c.query_row(
        "SELECT COUNT(*) FROM style_examples WHERE target_bundle IS ?1 AND context_key IS ?2",
        params![target_bundle, context_key],
        |r| r.get(0),
    )?;
    let over = count - MAX_ROWS_PER_CONTEXT;
    if over <= 0 {
        return Ok(());
    }
    c.execute(
        "DELETE FROM style_examples WHERE id IN (
            SELECT id FROM style_examples
            WHERE target_bundle IS ?1 AND context_key IS ?2 AND pinned = 0
            ORDER BY last_used_at ASC, use_count ASC
            LIMIT ?3
        )",
        params![target_bundle, context_key, over],
    )?;
    Ok(())
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
/// and, when `context_key` is provided (recipient-level, from
/// `os_integration::recipient_context_key`), resolved in
/// recipient -> app -> global order:
///
/// 1. Rows matching both `target_bundle` and `context_key` (recipient-level).
/// 2. If that yields fewer than `k`, backfilled with app-level rows for the
///    same `target_bundle` that have no `context_key` at all, up to `k`
///    total.
/// 3. If neither tier has rows, an empty `Vec` (no examples spliced into the
///    prompt — the existing zero-regression fallback).
///
/// When `context_key` is `None` (no recipient detected, or the frontmost app
/// isn't in STYLE-14's allowlist), retrieval is unchanged from before this
/// resolution order existed: every row for `target_bundle` regardless of its
/// `context_key`.
pub fn top_k(
    db: &Db,
    target_bundle: &str,
    context_key: Option<&str>,
    k: usize,
) -> rusqlite::Result<Vec<StyleExample>> {
    let k = k as i64;
    db.with_conn(|c| {
        const ORDER: &str = "ORDER BY pinned DESC, last_used_at DESC, use_count DESC LIMIT ?";
        let Some(ctx) = context_key else {
            let mut stmt = c.prepare(&format!(
                "SELECT {SELECT_COLUMNS} FROM style_examples WHERE target_bundle = ? {ORDER}"
            ))?;
            let rows = stmt
                .query_map(params![target_bundle, k], row_to_style_example)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            return Ok(rows);
        };

        let mut recipient_stmt = c.prepare(&format!(
            "SELECT {SELECT_COLUMNS} FROM style_examples \
             WHERE target_bundle = ? AND context_key = ? {ORDER}"
        ))?;
        let mut rows = recipient_stmt
            .query_map(params![target_bundle, ctx, k], row_to_style_example)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let remaining = k - rows.len() as i64;
        if remaining > 0 {
            let mut app_stmt = c.prepare(&format!(
                "SELECT {SELECT_COLUMNS} FROM style_examples \
                 WHERE target_bundle = ? AND context_key IS NULL {ORDER}"
            ))?;
            let backfill = app_stmt
                .query_map(params![target_bundle, remaining], row_to_style_example)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.extend(backfill);
        }
        Ok(rows)
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
        let no_context_id = insert(&db, &no_context).unwrap();

        // A different recipient's rows never leak in, but the app-level
        // (no context_key) row backfills once the recipient tier is
        // exhausted (see `top_k_resolves_recipient_then_app_then_global`
        // for the ordering-sensitive version of this).
        let rows = top_k(&db, "com.apple.mail", Some("boss@example.com"), 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, work_id, "recipient-scoped row ranks first");
        assert_eq!(rows[1].id, no_context_id, "app-level row backfills");

        // No context_key filter falls back to bundle-wide scoping (every row,
        // regardless of its own context_key) — unchanged from before the
        // recipient -> app -> global resolution order existed.
        let all = top_k(&db, "com.apple.mail", None, 10).unwrap();
        assert_eq!(all.len(), 3);
    }

    /// Full recipient -> app -> global resolution order: a recipient with its
    /// own examples gets those first; a recipient with none falls back to the
    /// app-level pool; an app with neither degrades to empty (no examples
    /// spliced into the prompt).
    #[test]
    fn top_k_resolves_recipient_then_app_then_global() {
        let db = Db::memory().unwrap();

        let mut recipient_row = sample("com.apple.MobileSMS", 1);
        recipient_row.context_key = Some("recipient-a".into());
        let recipient_row_id = insert(&db, &recipient_row).unwrap();

        let mut app_row = sample("com.apple.MobileSMS", 2);
        app_row.context_key = None;
        let app_row_id = insert(&db, &app_row).unwrap();

        // Recipient with its own example: recipient tier alone satisfies k=1,
        // so the app-level row must not appear.
        let recipient_hit = top_k(&db, "com.apple.MobileSMS", Some("recipient-a"), 1).unwrap();
        assert_eq!(recipient_hit.len(), 1);
        assert_eq!(recipient_hit[0].id, recipient_row_id);

        // A different recipient with no examples of their own backfills with
        // the app-level row.
        let recipient_miss = top_k(&db, "com.apple.MobileSMS", Some("recipient-b"), 10).unwrap();
        assert_eq!(recipient_miss.len(), 1);
        assert_eq!(recipient_miss[0].id, app_row_id);

        // An app with neither a recipient nor an app-level example degrades
        // to no examples at all.
        let none_at_all = top_k(&db, "com.other.app", Some("recipient-c"), 10).unwrap();
        assert!(none_at_all.is_empty());
    }

    /// Seeding more than `MAX_ROWS_PER_CONTEXT` rows in the same
    /// `(target_bundle, context_key)` scope evicts the least-recently-used
    /// non-pinned row first, and never touches a pinned row even when it is
    /// the least-recently-used.
    #[test]
    fn insert_evicts_least_used_non_pinned_row_over_cap() {
        let db = Db::memory().unwrap();

        let mut pinned_but_oldest = sample("com.apple.mail", 0);
        pinned_but_oldest.pinned = true;
        pinned_but_oldest.last_used_at = Some(1);
        pinned_but_oldest.use_count = 0;
        let pinned_id = insert(&db, &pinned_but_oldest).unwrap();

        let mut least_used = sample("com.apple.mail", 1);
        least_used.last_used_at = Some(10);
        least_used.use_count = 0;
        let least_used_id = insert(&db, &least_used).unwrap();

        // Fill up to (but not over) the cap with more-recently-used rows.
        for i in 0..(MAX_ROWS_PER_CONTEXT - 2) {
            let mut row = sample("com.apple.mail", 2 + i);
            row.last_used_at = Some(1000 + i);
            row.use_count = 1;
            insert(&db, &row).unwrap();
        }

        let before: i64 = db
            .with_conn(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM style_examples WHERE target_bundle = 'com.apple.mail'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(before, MAX_ROWS_PER_CONTEXT);

        // One more insert pushes this scope one row over the cap, triggering
        // eviction of exactly one row: the least-recently-used non-pinned one.
        let mut tipping_row = sample("com.apple.mail", 2 + MAX_ROWS_PER_CONTEXT);
        tipping_row.last_used_at = Some(2000);
        tipping_row.use_count = 1;
        let tipping_id = insert(&db, &tipping_row).unwrap();

        let after: i64 = db
            .with_conn(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM style_examples WHERE target_bundle = 'com.apple.mail'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(after, MAX_ROWS_PER_CONTEXT, "cap is enforced after insert");

        assert!(
            get(&db, least_used_id).unwrap().is_none(),
            "least-recently-used non-pinned row is evicted"
        );
        assert!(
            get(&db, pinned_id).unwrap().is_some(),
            "pinned row survives even though it is the oldest by last_used_at"
        );
        assert!(get(&db, tipping_id).unwrap().is_some());
    }

    /// A different `(target_bundle, context_key)` scope has its own
    /// independent cap: filling one scope to the cap must not evict rows that
    /// belong to a different bundle or a different recipient.
    #[test]
    fn eviction_cap_is_scoped_per_target_bundle_and_context_key() {
        let db = Db::memory().unwrap();

        let mut other_bundle_row = sample("com.other.app", 0);
        other_bundle_row.last_used_at = Some(1);
        other_bundle_row.use_count = 0;
        let other_bundle_id = insert(&db, &other_bundle_row).unwrap();

        let mut other_recipient_row = sample("com.apple.mail", 0);
        other_recipient_row.context_key = Some("someone-else".into());
        other_recipient_row.last_used_at = Some(1);
        other_recipient_row.use_count = 0;
        let other_recipient_id = insert(&db, &other_recipient_row).unwrap();

        for i in 0..=MAX_ROWS_PER_CONTEXT {
            let mut row = sample("com.apple.mail", 1 + i);
            row.last_used_at = Some(100 + i);
            row.use_count = 1;
            insert(&db, &row).unwrap();
        }

        assert!(
            get(&db, other_bundle_id).unwrap().is_some(),
            "a different target_bundle's rows are never evicted by this scope's cap"
        );
        assert!(
            get(&db, other_recipient_id).unwrap().is_some(),
            "a different context_key's rows are never evicted by this scope's cap"
        );
    }
}
