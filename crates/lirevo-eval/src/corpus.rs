//! Corpus loader: JSONL of `(transcript, profile, language, expected)` cases.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

const SUPPORTED_LANGS: &[&str] = &["en", "it", "fr", "de", "es"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCase {
    pub id: String,
    pub language: String,
    pub profile: String,
    pub transcript: String,
    pub expected: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Error)]
pub enum CorpusError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("jsonl parse error on line {line}: {source}")]
    Json {
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid language `{0}` (supported: en, it, fr, de, es)")]
    Language(String),
    #[error("duplicate test case id `{0}`")]
    DuplicateId(String),
}

#[must_use = "loaded corpus should be used"]
pub fn load_jsonl(path: &Path) -> Result<Vec<TestCase>, CorpusError> {
    let raw = std::fs::read_to_string(path)?;
    parse_jsonl(&raw)
}

#[must_use = "parsed corpus should be used"]
pub fn parse_jsonl(raw: &str) -> Result<Vec<TestCase>, CorpusError> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (i, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let case: TestCase = serde_json::from_str(line).map_err(|e| CorpusError::Json {
            line: i + 1,
            source: e,
        })?;
        if !SUPPORTED_LANGS.contains(&case.language.as_str()) {
            return Err(CorpusError::Language(case.language));
        }
        if !seen.insert(case.id.clone()) {
            return Err(CorpusError::DuplicateId(case.id));
        }
        out.push(case);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{parse_jsonl, CorpusError};

    #[test]
    fn parses_two_jsonl_lines() {
        let raw = r#"{"id":"it-mail_formal-001","language":"it","profile":"mail_formal","transcript":"ciao","expected":"Buongiorno","tags":[],"notes":""}
{"id":"en-chat_casual-001","language":"en","profile":"chat_casual","transcript":"hey","expected":"hey","tags":["greeting"],"notes":""}"#;
        let cases = parse_jsonl(raw).unwrap();
        assert_eq!(cases.len(), 2);
        assert_eq!(cases[0].language, "it");
        assert_eq!(cases[1].profile, "chat_casual");
        assert_eq!(cases[1].tags, vec!["greeting".to_string()]);
    }

    #[test]
    fn rejects_unknown_language() {
        let raw = r#"{"id":"x","language":"xx","profile":"plain","transcript":"a","expected":"b","tags":[],"notes":""}"#;
        let err = parse_jsonl(raw).unwrap_err();
        assert!(matches!(err, CorpusError::Language(_)), "got: {err}");
    }
}
