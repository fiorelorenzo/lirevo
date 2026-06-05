//! Shared system prompts used by `lirevo-cli clean` and `lirevo-prototype`.

/// Body of the dictation cleanup system prompt. The language instruction is
/// appended per-call by [`build_clean_system_prompt`]. Versioned in source so
/// changes are reviewable and reproducible.
pub const CLEAN_SYSTEM_PROMPT_BODY: &str = "You are a dictation post-processor. Given a raw speech-to-text transcript, return ONLY the cleaned text with:\n- proper punctuation, capitalization, and paragraphing\n- no added content or commentary\n- preserved meaning and word choice\n- numbers and units written naturally\nOutput ONLY the cleaned text, no quotes, no explanations.";

/// Build the cleanup system prompt for a given dictation language.
///
/// `language` is the dictation language setting: a code (`en` / `it` / `fr` /
/// `de` / `es`), `auto`, or empty. The prompt always tells the model to keep
/// the cleaned text in the transcript's own language and never translate it —
/// naming the language explicitly when it is known so the model applies that
/// language's punctuation, capitalization, and spacing conventions, and falling
/// back to a "keep the same language" instruction for `auto` / unknown codes.
#[must_use]
pub fn build_clean_system_prompt(language: &str) -> String {
    let lang_instruction = match language_name(language) {
        Some(name) => format!(
            "The transcript is in {name}. Write the cleaned text in {name} — never translate it into another language — and apply {name} punctuation, capitalization, and spacing conventions."
        ),
        None => "Write the cleaned text in the same language as the transcript — never translate it into another language — and apply that language's punctuation, capitalization, and spacing conventions.".to_string(),
    };
    format!("{CLEAN_SYSTEM_PROMPT_BODY}\n\n{lang_instruction}")
}

/// English name for a dictation language code, or `None` for `auto`, empty, or
/// an unrecognized code (the prompt then falls back to a language-agnostic
/// "keep the same language" instruction).
fn language_name(code: &str) -> Option<&'static str> {
    match code {
        "en" => Some("English"),
        "it" => Some("Italian"),
        "fr" => Some("French"),
        "de" => Some("German"),
        "es" => Some("Spanish"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_language_named_and_enforced() {
        let p = build_clean_system_prompt("it");
        assert!(p.contains("Italian"));
        assert!(p.contains("never translate"));
        assert!(!p.contains("{language}"));
        // The shared body is always present.
        assert!(p.contains("dictation post-processor"));
    }

    #[test]
    fn auto_keeps_same_language_without_naming_one() {
        let p = build_clean_system_prompt("auto");
        assert!(p.contains("same language as the transcript"));
        assert!(p.contains("never translate"));
        // Must not invent a concrete language for auto-detect.
        assert!(!p.contains("English"));
        assert!(!p.contains("Italian"));
    }

    #[test]
    fn unknown_code_falls_back_to_same_language() {
        let p = build_clean_system_prompt("");
        assert!(p.contains("same language as the transcript"));
    }
}
