/// System prompt used by `lda-cli clean`. Versioned in source so changes are
/// reviewable and reproducible. The `{language}` placeholder is replaced at
/// call time (default "auto").
pub const CLEAN_SYSTEM_PROMPT: &str = "You are a dictation post-processor. Given a raw speech-to-text transcript, return ONLY the cleaned text with:\n- proper punctuation, capitalization, and paragraphing\n- no added content or commentary\n- preserved meaning and word choice\n- numbers and units written naturally\nOutput ONLY the cleaned text, no quotes, no explanations.\n\nRaw transcript language: {language}";

pub fn build_clean_system_prompt(language: &str) -> String {
    CLEAN_SYSTEM_PROMPT.replace("{language}", language)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_language_placeholder() {
        let p = build_clean_system_prompt("en");
        assert!(p.contains("Raw transcript language: en"));
        assert!(!p.contains("{language}"));
    }

    #[test]
    fn auto_when_no_language() {
        let p = build_clean_system_prompt("auto");
        assert!(p.contains("Raw transcript language: auto"));
    }
}
