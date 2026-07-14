use tauri::State;

use crate::db::{history, style_examples, Db};
use crate::{AppError, AppState};

/// Wire type for [`style_examples_active_count`]: the pinned-example count
/// for the frontmost app, plus its display name so the Settings -> About
/// indicator can render "N style examples active for {app}" without a second
/// round trip to resolve the app name.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleExamplesActiveCount {
    pub count: i64,
    pub app_name: Option<String>,
}

/// Read-only indicator for Settings -> About: how many pinned style examples
/// are active for the frontmost app. Resolves "current app" the same way the
/// dictation pipeline does (`os_integration::frontmost_app()`) rather than
/// trusting a bundle id from the frontend, which has no other way to know it.
#[tauri::command]
pub fn style_examples_active_count(
    state: State<'_, AppState>,
) -> Result<StyleExamplesActiveCount, AppError> {
    let Some(app) = os_integration::frontmost_app() else {
        return Ok(StyleExamplesActiveCount {
            count: 0,
            app_name: None,
        });
    };
    let count = match &app.bundle_id {
        Some(bundle) => style_examples::count_pinned_for_bundle(state.db(), bundle)
            .map_err(|e| AppError::Internal(e.to_string()))?,
        None => 0,
    };
    Ok(StyleExamplesActiveCount {
        count,
        app_name: app.name,
    })
}

/// Persists a dictation's raw/cleaned pair as a manually pinned style
/// example, scoped to the dictation's own `target_bundle`. This is the sole
/// MVP capture path for `style_examples` — nothing implicit.
///
/// The frontend already hides the pin action when `style_learning_enabled` is
/// off, but a Tauri command is an IPC surface reachable independently of the
/// UI that gates it, so the setting is re-checked here too.
#[tauri::command]
pub fn style_example_pin(state: State<'_, AppState>, dictation_id: i64) -> Result<(), AppError> {
    let style_learning_enabled = state.inner.lock().unwrap().settings.style_learning_enabled;
    reject_if_style_learning_disabled(style_learning_enabled)?;
    pin_dictation_as_style_example(state.db(), dictation_id)
}

/// Reject the pin command while style learning is disabled. Split into a
/// pure function (mirroring `hotkey::reject_if_recording`) so the gating
/// logic is unit-testable without a live `AppState`.
fn reject_if_style_learning_disabled(style_learning_enabled: bool) -> Result<(), AppError> {
    if !style_learning_enabled {
        return Err(AppError::Internal("style learning is disabled".into()));
    }
    Ok(())
}

fn pin_dictation_as_style_example(db: &Db, dictation_id: i64) -> Result<(), AppError> {
    let dictation = history::get(db, dictation_id)
        .map_err(|e| AppError::Internal(e.to_string()))?
        .ok_or_else(|| AppError::Internal(format!("dictation {dictation_id} not found")))?;

    let target_bundle = dictation
        .target_bundle
        .ok_or_else(|| AppError::Internal("dictation has no target app to pin against".into()))?;

    style_examples::insert(
        db,
        &style_examples::NewStyleExample {
            dictation_id: Some(dictation.id),
            context_key: dictation.context_key,
            target_bundle: Some(target_bundle),
            raw_text: dictation.raw_text,
            final_text: dictation.cleaned_text,
            edit_distance_ratio: None,
            source: "manual_pin".into(),
            pinned: true,
            use_count: 0,
            last_used_at: None,
            created_at: now_secs(),
        },
    )
    .map(|_| ())
    .map_err(|e| AppError::Internal(e.to_string()))
}

/// Current wall-clock time in epoch seconds, matching `style_examples`'
/// caller-owns-the-clock convention (see `db::style_examples::touch_use`).
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_dictation(target_bundle: Option<&str>) -> history::NewDictation {
        history::NewDictation {
            created_at: 1000,
            language: Some("en".into()),
            stt_model: "parakeet-tdt-0.6b-v3".into(),
            audio_ms: Some(3000),
            raw_text: "raw transcript".into(),
            stt_ms: 100,
            llm_model: Some("gemma-3-1b".into()),
            cleaned_text: "Cleaned transcript.".into(),
            clean_ms: Some(200),
            cleanup_status: history::CLEANUP_APPLIED.into(),
            cleanup_error: None,
            inject_method: "pasteboard".into(),
            inject_ms: Some(50),
            total_ms: 350,
            target_app: target_bundle.map(|_| "Mail".into()),
            target_bundle: target_bundle.map(Into::into),
            input_device: None,
            smart_routing_enabled: false,
            smart_routing_applied: false,
            context_key: None,
        }
    }

    #[test]
    fn pins_a_dictation_with_a_target_bundle() {
        let db = Db::memory().unwrap();
        let dictation_id = history::insert(&db, &sample_dictation(Some("com.apple.mail"))).unwrap();

        pin_dictation_as_style_example(&db, dictation_id).unwrap();

        let examples = style_examples::list_for_bundle(&db, "com.apple.mail").unwrap();
        assert_eq!(examples.len(), 1);
        assert_eq!(examples[0].source, "manual_pin");
        assert!(examples[0].pinned);
        assert_eq!(examples[0].dictation_id, Some(dictation_id));
        assert_eq!(examples[0].raw_text, "raw transcript");
        assert_eq!(examples[0].final_text, "Cleaned transcript.");
    }

    /// The fix at the heart of #137: pinning a dictation that was captured
    /// with a recipient-level `context_key` (see `hotkey.rs::run_pipeline`)
    /// must carry that key onto the resulting `style_examples` row, instead
    /// of hardcoding `context_key: None` as before.
    #[test]
    fn pin_carries_dictation_context_key_through() {
        let db = Db::memory().unwrap();
        let mut dictation = sample_dictation(Some("com.apple.MobileSMS"));
        dictation.context_key = Some("recipient-a".into());
        let dictation_id = history::insert(&db, &dictation).unwrap();

        pin_dictation_as_style_example(&db, dictation_id).unwrap();

        let examples = style_examples::list_for_bundle(&db, "com.apple.MobileSMS").unwrap();
        assert_eq!(examples.len(), 1);
        assert_eq!(examples[0].context_key.as_deref(), Some("recipient-a"));
    }

    /// Proves the recipient tier — dead code before this issue, since every
    /// pinned row had `context_key IS NULL` — is now actually reachable
    /// end-to-end: pin a dictation with no recipient context, then pin a
    /// second one scoped to a recipient, and confirm `top_k` prefers the
    /// recipient-scoped pin for that recipient over the app-level one.
    #[test]
    fn pinned_recipient_example_outranks_app_level_pin_in_top_k() {
        let db = Db::memory().unwrap();

        let mut app_level = sample_dictation(Some("com.apple.MobileSMS"));
        app_level.raw_text = "app-level raw".into();
        app_level.cleaned_text = "app-level final".into();
        let app_level_id = history::insert(&db, &app_level).unwrap();
        pin_dictation_as_style_example(&db, app_level_id).unwrap();

        let mut recipient = sample_dictation(Some("com.apple.MobileSMS"));
        recipient.context_key = Some("recipient-a".into());
        recipient.raw_text = "recipient raw".into();
        recipient.cleaned_text = "recipient final".into();
        let recipient_id = history::insert(&db, &recipient).unwrap();
        pin_dictation_as_style_example(&db, recipient_id).unwrap();

        let rows =
            style_examples::top_k(&db, "com.apple.MobileSMS", Some("recipient-a"), 1).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].final_text, "recipient final",
            "recipient-scoped pin wins over the app-level one for its own recipient"
        );
        assert_eq!(rows[0].context_key.as_deref(), Some("recipient-a"));
    }

    #[test]
    fn errors_when_dictation_has_no_target_bundle() {
        let db = Db::memory().unwrap();
        let dictation_id = history::insert(&db, &sample_dictation(None)).unwrap();

        let err = pin_dictation_as_style_example(&db, dictation_id).unwrap_err();
        assert!(matches!(err, AppError::Internal(_)));
    }

    #[test]
    fn errors_when_dictation_does_not_exist() {
        let db = Db::memory().unwrap();
        let err = pin_dictation_as_style_example(&db, 999).unwrap_err();
        assert!(matches!(err, AppError::Internal(_)));
    }

    #[test]
    fn rejects_pin_when_style_learning_disabled() {
        assert!(matches!(
            reject_if_style_learning_disabled(false),
            Err(AppError::Internal(_))
        ));
    }

    #[test]
    fn allows_pin_when_style_learning_enabled() {
        assert!(reject_if_style_learning_disabled(true).is_ok());
    }
}
