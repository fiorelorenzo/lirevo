//! Runs deterministic post-assertions against a candidate output.

use regex::Regex;

use crate::profiles::Assertion;

#[derive(Debug, Clone)]
pub struct AssertionResult {
    pub assertion: Assertion,
    pub passed: bool,
}

#[must_use]
pub fn run_all(candidate: &str, assertions: &[Assertion]) -> Vec<AssertionResult> {
    assertions
        .iter()
        .map(|a| AssertionResult {
            assertion: a.clone(),
            passed: run_one(candidate, a),
        })
        .collect()
}

pub(crate) fn run_one(s: &str, a: &Assertion) -> bool {
    match a {
        Assertion::RegexMustMatch { pattern } => Regex::new(pattern).is_ok_and(|r| r.is_match(s)),
        Assertion::RegexMustNotMatch { pattern } => {
            // Parse failure → treat as "no match", i.e. assertion passes.
            !Regex::new(pattern).is_ok_and(|r| r.is_match(s))
        }
        Assertion::MaxLengthChars { value } => s.chars().count() <= *value,
        Assertion::MinLengthChars { value } => s.chars().count() >= *value,
    }
}

#[cfg(test)]
mod tests {
    use super::run_one;
    use crate::profiles::Assertion;

    #[test]
    fn regex_must_match_passes() {
        let a = Assertion::RegexMustMatch {
            pattern: "^Buongiorno".into(),
        };
        assert!(run_one("Buongiorno, le scrivo", &a));
    }

    #[test]
    fn regex_must_not_match_passes_when_absent() {
        let a = Assertion::RegexMustNotMatch {
            pattern: "(?i)cordiali saluti".into(),
        };
        assert!(run_one("hey tutto bene?", &a));
    }

    #[test]
    fn max_length_fails_when_too_long() {
        let a = Assertion::MaxLengthChars { value: 5 };
        assert!(!run_one("hello world", &a));
    }
}
