//! `lda-eval gen-corpus` — oracle-driven corpus expansion.
//!
//! Groups seed cases by `(profile, language)`. For each cell short of
//! `target_per_cell`, prompts the oracle with the existing seeds as
//! few-shot examples and asks for `needed` additional JSONL cases.
//! Parses the oracle's output line-by-line, rewriting ids deterministically.
//! The combined seeds + generated output is written to `--out` for the user
//! to review manually before committing.

use std::collections::HashMap;
use std::fmt::Write as _;

use anyhow::{Context, Result};

use crate::backend::{build_from_spec, GenerateReq};
use crate::cli::GenCorpusArgs;
use crate::corpus::{load_jsonl, TestCase};
use crate::profiles::{load_toml, validate};

pub async fn run(args: GenCorpusArgs) -> Result<()> {
    let seeds = load_jsonl(&args.seeds).context("load seeds")?;
    let profiles = load_toml(&args.profiles).context("load profiles")?;
    validate(&seeds, &profiles).context("cross-ref")?;

    let oracle = build_from_spec(&args.oracle)
        .await
        .context("build oracle")?;
    let target = args.target_per_cell as usize;

    // Group seeds by (profile, language).
    let mut by_cell: HashMap<(String, String), Vec<&TestCase>> = HashMap::new();
    for s in &seeds {
        by_cell
            .entry((s.profile.clone(), s.language.clone()))
            .or_default()
            .push(s);
    }

    // Deterministic cell ordering so re-runs are reproducible.
    let mut cells: Vec<&(String, String)> = by_cell.keys().collect();
    cells.sort();

    let mut generated: Vec<TestCase> = Vec::new();
    for cell_key in cells {
        let (profile_id, lang) = cell_key;
        let cell_seeds = by_cell.get(cell_key).expect("just iterated");
        let have = cell_seeds.len();
        if have >= target {
            continue;
        }
        let needed = target - have;
        let prompt = build_oracle_prompt(profile_id, lang, cell_seeds, needed);
        let out = oracle
            .generate(GenerateReq {
                system_prompt: ORACLE_SYSTEM_PROMPT.into(),
                transcript: prompt,
                max_tokens: 2048,
                temperature: 0.7,
            })
            .await
            .with_context(|| format!("oracle generate for cell ({profile_id}, {lang})"))?;
        let new_cases = parse_oracle_jsonl(&out.text, profile_id, lang, have + 1);
        generated.extend(new_cases);
    }

    // Emit seeds + generated, sorted by id.
    let mut all = seeds.clone();
    all.extend(generated);
    all.sort_by(|a, b| a.id.cmp(&b.id));

    let mut buf = String::new();
    for c in &all {
        writeln!(buf, "{}", serde_json::to_string(c)?)
            .map_err(|e| anyhow::anyhow!("write line: {e}"))?;
    }
    std::fs::write(&args.out, buf).context("write output jsonl")?;
    eprintln!(
        "wrote {} (review manually before merging into v1.jsonl)",
        args.out.display(),
    );
    Ok(())
}

const ORACLE_SYSTEM_PROMPT: &str =
    "You generate JSONL test cases for a dictation refiner evaluation corpus. \
Follow the user's instructions exactly. Output ONLY valid JSONL — one object per line, \
no markdown fences, no commentary.";

fn build_oracle_prompt(profile: &str, lang: &str, seeds: &[&TestCase], needed: usize) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "Profile: {profile}");
    let _ = writeln!(s, "Language: {lang}");
    let _ = writeln!(s, "Generate {needed} additional JSONL test cases. Schema:");
    let _ = writeln!(
        s,
        "{{\"id\":\"<lang>-<profile>-NNN\",\"language\":\"<lang>\",\"profile\":\"<profile>\",\"transcript\":\"<raw STT, lowercase, sparse punctuation, may contain disfluences>\",\"expected\":\"<gold refined output>\",\"tags\":[],\"notes\":\"\"}}"
    );
    let _ = writeln!(s);
    let _ = writeln!(s, "Reference seeds for this cell:");
    for x in seeds {
        if let Ok(line) = serde_json::to_string(x) {
            let _ = writeln!(s, "{line}");
        }
    }
    let _ = writeln!(s);
    let _ = writeln!(
        s,
        "Vary the topics. Keep adversarial difficulty: multiple disfluences, numerals, \
code-switching, ambiguous tone. Produce ONLY the new {needed} lines."
    );
    s
}

fn parse_oracle_jsonl(text: &str, profile: &str, lang: &str, start_idx: usize) -> Vec<TestCase> {
    let mut out = Vec::new();
    let mut offset: usize = 0;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("```") {
            continue;
        }
        match serde_json::from_str::<TestCase>(line) {
            Ok(mut c) => {
                let n = start_idx + offset;
                c.id = format!("{lang}-{profile}-{n:03}");
                c.language = lang.to_string();
                c.profile = profile.to_string();
                out.push(c);
            }
            Err(e) => tracing::warn!(?e, line, "skip non-JSONL line from oracle"),
        }
        offset += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{build_oracle_prompt, parse_oracle_jsonl};
    use crate::corpus::TestCase;

    fn seed(id: &str) -> TestCase {
        TestCase {
            id: id.into(),
            language: "it".into(),
            profile: "plain".into(),
            transcript: "ciao".into(),
            expected: "Ciao.".into(),
            tags: vec![],
            notes: String::new(),
        }
    }

    #[test]
    fn build_oracle_prompt_includes_profile_lang_seeds_and_count() {
        let seeds = vec![seed("it-plain-001"), seed("it-plain-002")];
        let refs: Vec<&TestCase> = seeds.iter().collect();
        let p = build_oracle_prompt("plain", "it", &refs, 3);
        assert!(p.contains("Profile: plain"));
        assert!(p.contains("Language: it"));
        assert!(p.contains("Generate 3 additional"));
        assert!(p.contains("it-plain-001"));
    }

    #[test]
    fn parse_oracle_jsonl_rewrites_ids_and_skips_garbage() {
        let raw = r#"```jsonl
{"id":"x","language":"???","profile":"???","transcript":"a","expected":"A","tags":[],"notes":""}
not json at all
{"id":"y","language":"it","profile":"plain","transcript":"b","expected":"B","tags":[],"notes":""}
```"#;
        let parsed = parse_oracle_jsonl(raw, "plain", "it", 7);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].id, "it-plain-007");
        assert_eq!(parsed[1].id, "it-plain-009");
        assert_eq!(parsed[0].language, "it");
        assert_eq!(parsed[0].profile, "plain");
    }
}
