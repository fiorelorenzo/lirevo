//! Profile loader: TOML map of profile-id → metadata + per-language prompts.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::corpus::TestCase;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Assertion {
    RegexMustMatch { pattern: String },
    RegexMustNotMatch { pattern: String },
    MaxLengthChars { value: usize },
    MinLengthChars { value: usize },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Profile {
    #[serde(default)]
    pub post_assertions: Vec<Assertion>,
    pub system_prompts: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct ProfilesFile {
    profile: HashMap<String, Profile>,
}

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml parse error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("test case `{case_id}` references unknown profile `{profile}`")]
    UnknownProfile { case_id: String, profile: String },
    #[error("profile `{profile}` is missing system_prompts.{language} for test case `{case_id}`")]
    MissingPrompt {
        profile: String,
        language: String,
        case_id: String,
    },
}

#[must_use = "loaded profiles should be used"]
pub fn load_toml(path: &Path) -> Result<HashMap<String, Profile>, ProfileError> {
    let raw = std::fs::read_to_string(path)?;
    parse_toml(&raw)
}

#[must_use = "parsed profiles should be used"]
pub fn parse_toml(raw: &str) -> Result<HashMap<String, Profile>, ProfileError> {
    let f: ProfilesFile = toml::from_str(raw)?;
    Ok(f.profile)
}

pub fn validate<S: std::hash::BuildHasher>(
    cases: &[TestCase],
    profiles: &HashMap<String, Profile, S>,
) -> Result<(), ProfileError> {
    for c in cases {
        let p = profiles
            .get(&c.profile)
            .ok_or_else(|| ProfileError::UnknownProfile {
                case_id: c.id.clone(),
                profile: c.profile.clone(),
            })?;
        if !p.system_prompts.contains_key(&c.language) {
            return Err(ProfileError::MissingPrompt {
                profile: c.profile.clone(),
                language: c.language.clone(),
                case_id: c.id.clone(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_toml, validate};
    use crate::corpus::TestCase;

    #[test]
    fn parses_profile_with_per_lang_prompts() {
        let raw = r#"
[profile.plain]
post_assertions = []

[profile.plain.system_prompts]
it = "Sei un refiner..."
en = "You are a refiner..."
fr = "Tu es un refiner..."
de = "Du bist ein Refiner..."
es = "Eres un refinador..."
"#;
        let profiles = parse_toml(raw).unwrap();
        let p = profiles.get("plain").expect("plain present");
        assert_eq!(p.system_prompts.get("it").unwrap(), "Sei un refiner...");
        assert_eq!(p.system_prompts.len(), 5);
    }

    #[test]
    fn cross_ref_rejects_missing_profile() {
        let raw = r#"
[profile.plain]
post_assertions = []
[profile.plain.system_prompts]
it = "x"
en = "x"
fr = "x"
de = "x"
es = "x"
"#;
        let profiles = parse_toml(raw).unwrap();
        let case = TestCase {
            id: "x".into(),
            language: "it".into(),
            profile: "missing_profile".into(),
            transcript: String::new(),
            expected: String::new(),
            tags: vec![],
            notes: String::new(),
        };
        let err = validate(&[case], &profiles).unwrap_err();
        assert!(err.to_string().contains("missing_profile"), "got: {err}");
    }

    #[test]
    fn cross_ref_rejects_missing_per_lang_prompt() {
        let raw = r#"
[profile.plain]
post_assertions = []
[profile.plain.system_prompts]
en = "only english"
"#;
        let profiles = parse_toml(raw).unwrap();
        let case = TestCase {
            id: "x".into(),
            language: "it".into(),
            profile: "plain".into(),
            transcript: String::new(),
            expected: String::new(),
            tags: vec![],
            notes: String::new(),
        };
        let err = validate(&[case], &profiles).unwrap_err();
        assert!(err.to_string().contains("system_prompts.it"), "got: {err}");
    }
}
