use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::Db;

/// Cleanup outcome for one dictation.
pub const CLEANUP_APPLIED: &str = "applied";
pub const CLEANUP_SKIPPED: &str = "skipped";
pub const CLEANUP_FAILED: &str = "failed";

/// Terminal pipeline failures at a stage earlier than cleanup. There is no
/// dedicated "stage" column: `cleanup_status` already doubles as the row's
/// overall outcome, so these reuse it rather than add a migration for a
/// second status column.
pub const STT_FAILED: &str = "stt_failed";
pub const INJECT_FAILED: &str = "inject_failed";

/// Insert payload (every column except `id`).
#[derive(Debug, Clone)]
pub struct NewDictation {
    pub created_at: i64,
    pub language: Option<String>,
    pub stt_model: String,
    pub audio_ms: Option<i64>,
    pub raw_text: String,
    pub stt_ms: i64,
    pub llm_model: Option<String>,
    pub cleaned_text: String,
    pub clean_ms: Option<i64>,
    pub cleanup_status: String,
    pub cleanup_error: Option<String>,
    pub inject_method: String,
    pub inject_ms: Option<i64>,
    pub total_ms: i64,
    pub target_app: Option<String>,
    pub target_bundle: Option<String>,
    /// Input device actually used for this dictation.
    pub input_device: Option<String>,
    /// Whether the smart-mic-routing setting was on for this dictation.
    pub smart_routing_enabled: bool,
    /// Whether smart routing actually rerouted to the built-in mic.
    pub smart_routing_applied: bool,
    /// Recipient-level context key (`os_integration::recipient_context_key`),
    /// resolved once alongside `target_bundle` right after STT. `None` when
    /// the target app isn't in the recipient allowlist, no window title was
    /// available, or the dictation predates migration 004.
    pub context_key: Option<String>,
}

/// Lightweight row for the list (no full transcripts).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationSummary {
    pub id: i64,
    pub created_at: i64,
    pub preview: String,
    pub stt_model: String,
    pub llm_model: Option<String>,
    pub target_app: Option<String>,
    pub total_ms: i64,
    pub cleanup_status: String,
}

/// Full row for the detail view.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dictation {
    pub id: i64,
    pub created_at: i64,
    pub language: Option<String>,
    pub stt_model: String,
    pub audio_ms: Option<i64>,
    pub raw_text: String,
    pub stt_ms: i64,
    pub llm_model: Option<String>,
    pub cleaned_text: String,
    pub clean_ms: Option<i64>,
    pub cleanup_status: String,
    pub cleanup_error: Option<String>,
    pub inject_method: String,
    pub inject_ms: Option<i64>,
    pub total_ms: i64,
    pub target_app: Option<String>,
    pub target_bundle: Option<String>,
    /// Input device actually used. `None` for rows written before migration 002.
    pub input_device: Option<String>,
    /// Whether smart mic routing was enabled. `None` for pre-002 rows.
    pub smart_routing_enabled: Option<bool>,
    /// Whether smart routing rerouted to the built-in mic. `None` for pre-002 rows.
    pub smart_routing_applied: Option<bool>,
    /// Recipient-level context key. `None` for pre-004 rows, or when no
    /// recipient could be resolved for this dictation.
    pub context_key: Option<String>,
}

const PREVIEW_CHARS: usize = 120;

pub(crate) fn preview_of(text: &str) -> String {
    let mut s: String = text.chars().take(PREVIEW_CHARS).collect();
    if text.chars().count() > PREVIEW_CHARS {
        s.push('…');
    }
    s
}

pub fn insert(db: &Db, e: &NewDictation) -> rusqlite::Result<i64> {
    db.with_conn(|c| {
        c.execute(
            "INSERT INTO dictations (created_at, language, stt_model, audio_ms, raw_text, stt_ms,
                llm_model, cleaned_text, clean_ms, cleanup_status, cleanup_error,
                inject_method, inject_ms, total_ms, target_app, target_bundle,
                input_device, smart_routing_enabled, smart_routing_applied, context_key)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
            params![
                e.created_at,
                e.language,
                e.stt_model,
                e.audio_ms,
                e.raw_text,
                e.stt_ms,
                e.llm_model,
                e.cleaned_text,
                e.clean_ms,
                e.cleanup_status,
                e.cleanup_error,
                e.inject_method,
                e.inject_ms,
                e.total_ms,
                e.target_app,
                e.target_bundle,
                e.input_device,
                i64::from(e.smart_routing_enabled),
                i64::from(e.smart_routing_applied),
                e.context_key,
            ],
        )?;
        Ok(c.last_insert_rowid())
    })
}

pub fn list(db: &Db, limit: u32, offset: u32) -> rusqlite::Result<Vec<DictationSummary>> {
    db.with_conn(|c| {
        let mut stmt = c.prepare(
            "SELECT id, created_at, cleaned_text, stt_model, llm_model, target_app, total_ms, cleanup_status
             FROM dictations ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt
            .query_map(params![limit, offset], |r| {
                let cleaned: String = r.get(2)?;
                Ok(DictationSummary {
                    id: r.get(0)?,
                    created_at: r.get(1)?,
                    preview: preview_of(&cleaned),
                    stt_model: r.get(3)?,
                    llm_model: r.get(4)?,
                    target_app: r.get(5)?,
                    total_ms: r.get(6)?,
                    cleanup_status: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })
}

pub fn get(db: &Db, id: i64) -> rusqlite::Result<Option<Dictation>> {
    db.with_conn(|c| {
        c.query_row(
            "SELECT id, created_at, language, stt_model, audio_ms, raw_text, stt_ms, llm_model,
                cleaned_text, clean_ms, cleanup_status, cleanup_error, inject_method, inject_ms,
                total_ms, target_app, target_bundle, input_device, smart_routing_enabled,
                smart_routing_applied, context_key FROM dictations WHERE id = ?1",
            params![id],
            |r| {
                Ok(Dictation {
                    id: r.get(0)?,
                    created_at: r.get(1)?,
                    language: r.get(2)?,
                    stt_model: r.get(3)?,
                    audio_ms: r.get(4)?,
                    raw_text: r.get(5)?,
                    stt_ms: r.get(6)?,
                    llm_model: r.get(7)?,
                    cleaned_text: r.get(8)?,
                    clean_ms: r.get(9)?,
                    cleanup_status: r.get(10)?,
                    cleanup_error: r.get(11)?,
                    inject_method: r.get(12)?,
                    inject_ms: r.get(13)?,
                    total_ms: r.get(14)?,
                    target_app: r.get(15)?,
                    target_bundle: r.get(16)?,
                    input_device: r.get(17)?,
                    smart_routing_enabled: r.get::<_, Option<i64>>(18)?.map(|v| v != 0),
                    smart_routing_applied: r.get::<_, Option<i64>>(19)?.map(|v| v != 0),
                    context_key: r.get(20)?,
                })
            },
        )
        .optional()
    })
}

pub fn delete(db: &Db, id: i64) -> rusqlite::Result<()> {
    db.with_conn(|c| {
        c.execute("DELETE FROM dictations WHERE id = ?1", params![id])
            .map(|_| ())
    })
}

pub fn clear(db: &Db) -> rusqlite::Result<()> {
    db.with_conn(|c| c.execute("DELETE FROM dictations", []).map(|_| ()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(created_at: i64, cleaned: &str, stt_only: bool) -> NewDictation {
        NewDictation {
            created_at,
            language: Some("it".into()),
            stt_model: "parakeet-tdt-0.6b-v3".into(),
            audio_ms: Some(3000),
            raw_text: "raw".into(),
            stt_ms: 171,
            llm_model: if stt_only {
                None
            } else {
                Some("gemma-3-1b".into())
            },
            cleaned_text: cleaned.into(),
            clean_ms: if stt_only { None } else { Some(652) },
            cleanup_status: if stt_only {
                CLEANUP_SKIPPED.into()
            } else {
                CLEANUP_APPLIED.into()
            },
            cleanup_error: None,
            inject_method: "pasteboard".into(),
            inject_ms: Some(163),
            total_ms: 986,
            target_app: Some("Mail".into()),
            target_bundle: Some("com.apple.mail".into()),
            input_device: Some("MacBook Pro Microphone".into()),
            smart_routing_enabled: true,
            smart_routing_applied: stt_only,
            context_key: None,
        }
    }

    #[test]
    fn insert_list_get_roundtrip() {
        let db = Db::memory().unwrap();
        let id1 = insert(&db, &sample(1000, "first", false)).unwrap();
        let id2 = insert(&db, &sample(2000, &"x".repeat(200), true)).unwrap();

        let list = list(&db, 10, 0).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, id2); // newest first
        assert!(list[0].preview.ends_with('…'));
        assert_eq!(list[0].llm_model, None);
        assert_eq!(list[1].id, id1);

        let full = get(&db, id1).unwrap().unwrap();
        assert_eq!(full.cleaned_text, "first");
        assert_eq!(full.cleanup_status, CLEANUP_APPLIED);
        assert_eq!(full.input_device.as_deref(), Some("MacBook Pro Microphone"));
        assert_eq!(full.smart_routing_enabled, Some(true));
        // id1 was the non-stt-only sample, so routing was not applied.
        assert_eq!(full.smart_routing_applied, Some(false));
        let stt_only = get(&db, id2).unwrap().unwrap();
        assert_eq!(stt_only.llm_model, None);
        assert_eq!(stt_only.clean_ms, None);
        assert_eq!(stt_only.smart_routing_applied, Some(true));
    }

    /// `context_key` round-trips through insert/get both when a recipient was
    /// resolved (allowlisted app with a window title) and when it wasn't
    /// (`None` — non-allowlisted app, no window title, or pre-004 row).
    #[test]
    fn context_key_roundtrips_with_and_without_a_value() {
        let db = Db::memory().unwrap();

        let mut with_context = sample(1, "hey there", false);
        with_context.target_bundle = Some("com.apple.MobileSMS".into());
        with_context.context_key = Some("abcdef0123456789".into());
        let with_context_id = insert(&db, &with_context).unwrap();

        let without_context = sample(2, "unrelated app", false);
        let without_context_id = insert(&db, &without_context).unwrap();

        let got_with = get(&db, with_context_id).unwrap().unwrap();
        assert_eq!(got_with.context_key.as_deref(), Some("abcdef0123456789"));

        let got_without = get(&db, without_context_id).unwrap().unwrap();
        assert_eq!(got_without.context_key, None);
    }

    #[test]
    fn delete_and_clear() {
        let db = Db::memory().unwrap();
        let id = insert(&db, &sample(1, "a", false)).unwrap();
        insert(&db, &sample(2, "b", false)).unwrap();
        delete(&db, id).unwrap();
        assert_eq!(list(&db, 10, 0).unwrap().len(), 1);
        clear(&db).unwrap();
        assert_eq!(list(&db, 10, 0).unwrap().len(), 0);
        assert!(get(&db, id).unwrap().is_none());
    }

    #[test]
    fn list_paginates() {
        let db = Db::memory().unwrap();
        for i in 0..5 {
            insert(&db, &sample(i, "x", false)).unwrap();
        }
        let page = list(&db, 2, 2).unwrap();
        assert_eq!(page.len(), 2);
    }

    /// STT/inject failure rows have no cleaned transcript yet (NOT NULL
    /// columns fall back to empty strings) but must still round-trip and
    /// surface the failed stage via `cleanup_status`.
    #[test]
    fn insert_get_stage_failure_rows() {
        let db = Db::memory().unwrap();

        let stt_failed = NewDictation {
            created_at: 1,
            language: Some("en".into()),
            stt_model: "parakeet-tdt-0.6b-v3".into(),
            audio_ms: Some(1500),
            raw_text: String::new(),
            stt_ms: 42,
            llm_model: None,
            cleaned_text: String::new(),
            clean_ms: None,
            cleanup_status: STT_FAILED.into(),
            cleanup_error: Some("STT model not loaded".into()),
            inject_method: "none".into(),
            inject_ms: None,
            total_ms: 42,
            target_app: None,
            target_bundle: None,
            input_device: None,
            smart_routing_enabled: false,
            smart_routing_applied: false,
            context_key: None,
        };
        let stt_id = insert(&db, &stt_failed).unwrap();
        let got = get(&db, stt_id).unwrap().unwrap();
        assert_eq!(got.cleanup_status, STT_FAILED);
        assert_eq!(got.cleanup_error.as_deref(), Some("STT model not loaded"));
        assert_eq!(got.raw_text, "");
        assert_eq!(got.inject_method, "none");

        let mut inject_failed = sample(2, "cleaned text", false);
        inject_failed.cleanup_status = INJECT_FAILED.into();
        inject_failed.cleanup_error = Some("inject failed: no focused element".into());
        inject_failed.inject_method = "failed".into();
        inject_failed.inject_ms = None;
        let inject_id = insert(&db, &inject_failed).unwrap();
        let got = get(&db, inject_id).unwrap().unwrap();
        assert_eq!(got.cleanup_status, INJECT_FAILED);
        assert_eq!(got.cleaned_text, "cleaned text");
        assert_eq!(got.inject_ms, None);

        // Both stage-failure rows still show up in the plain list query.
        let list = list(&db, 10, 0).unwrap();
        assert_eq!(list.len(), 2);
    }
}
