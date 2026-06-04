use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::Db;

/// Cleanup outcome for one dictation.
pub const CLEANUP_APPLIED: &str = "applied";
pub const CLEANUP_SKIPPED: &str = "skipped";
pub const CLEANUP_FAILED: &str = "failed";

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
}

const PREVIEW_CHARS: usize = 120;

fn preview_of(text: &str) -> String {
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
                inject_method, inject_ms, total_ms, target_app, target_bundle)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
            params![
                e.created_at, e.language, e.stt_model, e.audio_ms, e.raw_text, e.stt_ms,
                e.llm_model, e.cleaned_text, e.clean_ms, e.cleanup_status, e.cleanup_error,
                e.inject_method, e.inject_ms, e.total_ms, e.target_app, e.target_bundle,
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
                total_ms, target_app, target_bundle FROM dictations WHERE id = ?1",
            params![id],
            |r| {
                Ok(Dictation {
                    id: r.get(0)?, created_at: r.get(1)?, language: r.get(2)?, stt_model: r.get(3)?,
                    audio_ms: r.get(4)?, raw_text: r.get(5)?, stt_ms: r.get(6)?, llm_model: r.get(7)?,
                    cleaned_text: r.get(8)?, clean_ms: r.get(9)?, cleanup_status: r.get(10)?,
                    cleanup_error: r.get(11)?, inject_method: r.get(12)?, inject_ms: r.get(13)?,
                    total_ms: r.get(14)?, target_app: r.get(15)?, target_bundle: r.get(16)?,
                })
            },
        )
        .optional()
    })
}

pub fn delete(db: &Db, id: i64) -> rusqlite::Result<()> {
    db.with_conn(|c| c.execute("DELETE FROM dictations WHERE id = ?1", params![id]).map(|_| ()))
}

pub fn clear(db: &Db) -> rusqlite::Result<()> {
    db.with_conn(|c| c.execute("DELETE FROM dictations", []).map(|_| ()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(created_at: i64, cleaned: &str, stt_only: bool) -> NewDictation {
        NewDictation {
            created_at, language: Some("it".into()), stt_model: "parakeet-tdt-0.6b-v3".into(),
            audio_ms: Some(3000), raw_text: "raw".into(), stt_ms: 171,
            llm_model: if stt_only { None } else { Some("gemma-3-1b".into()) },
            cleaned_text: cleaned.into(), clean_ms: if stt_only { None } else { Some(652) },
            cleanup_status: if stt_only { CLEANUP_SKIPPED.into() } else { CLEANUP_APPLIED.into() },
            cleanup_error: None, inject_method: "pasteboard".into(), inject_ms: Some(163),
            total_ms: 986, target_app: Some("Mail".into()), target_bundle: Some("com.apple.mail".into()),
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
        let stt_only = get(&db, id2).unwrap().unwrap();
        assert_eq!(stt_only.llm_model, None);
        assert_eq!(stt_only.clean_ms, None);
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
        for i in 0..5 { insert(&db, &sample(i, "x", false)).unwrap(); }
        let page = list(&db, 2, 2).unwrap();
        assert_eq!(page.len(), 2);
    }
}
