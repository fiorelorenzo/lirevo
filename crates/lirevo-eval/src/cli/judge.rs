//! `lirevo-eval judge` — re-score an existing report with an LLM-as-judge backend.
//!
//! Loads the JSON sidecar produced by `lirevo-eval run`, sends a strict-JSON
//! eliciting prompt per cell to the judge backend, and writes the augmented
//! report with `judge_fidelity` / `judge_style` / `judge_rationale` filled in.
//! Cells where the judge returns non-JSON are logged and left with `None`s.

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::backend::{build_from_spec, GenerateReq};
use crate::cli::JudgeArgs;
use crate::report::{json::load as load_report, write_pair, BackendDescriptor};

const SYS: &str =
    "You are a strict evaluator. For each (transcript, expected, candidate) triple, score \
the candidate on FIDELITY (1-5: preserves semantic content of transcript) and \
STYLE_MATCH (1-5: matches the style demonstrated by expected). Return ONLY a JSON \
object with keys `fidelity`, `style`, `rationale`. No markdown, no preamble.";

#[derive(Debug, Deserialize)]
struct JudgePayload {
    fidelity: u8,
    style: u8,
    rationale: String,
}

pub async fn run(args: JudgeArgs) -> Result<()> {
    let mut report = load_report(&args.report).context("load report")?;
    let judge = build_from_spec(&args.judge).await.context("build judge")?;

    for outcome in &mut report.outcomes {
        let prompt = format!(
            "Transcript:\n{t}\n\nExpected (target style):\n{e}\n\nCandidate:\n{c}\n\n\
Return strict JSON only: {{\"fidelity\": N, \"style\": N, \"rationale\": \"...\"}}",
            t = outcome.case.transcript,
            e = outcome.case.expected,
            c = outcome.candidate,
        );
        let out = judge
            .generate(GenerateReq {
                system_prompt: SYS.into(),
                transcript: prompt,
                max_tokens: 256,
                temperature: 0.0,
            })
            .await
            .with_context(|| format!("judge generate for case {}", outcome.case.id))?;

        let trimmed = out.text.trim();
        match serde_json::from_str::<JudgePayload>(trimmed) {
            Ok(p) => {
                outcome.score.judge_fidelity = Some(p.fidelity);
                outcome.score.judge_style = Some(p.style);
                outcome.score.judge_rationale = Some(p.rationale);
            }
            Err(e) => {
                tracing::warn!(
                    case = %outcome.case.id,
                    raw = %trimmed,
                    ?e,
                    "judge returned non-JSON; recorded as null"
                );
            }
        }
    }

    // Best-effort descriptor for the judge — split on `:` to extract kind/id.
    let (kind, id) = args
        .judge
        .split_once(':')
        .map_or(("unknown", args.judge.as_str()), |(k, rest)| {
            (k, rest.split('@').next().unwrap_or(rest))
        });
    report.judge = Some(BackendDescriptor {
        spec: args.judge.clone(),
        id: id.to_string(),
        kind: kind.to_string(),
    });

    write_pair(&report, &args.out).context("write augmented report")?;
    println!("wrote {}", args.out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::JudgePayload;

    #[test]
    fn parses_strict_json_payload() {
        let raw =
            r#"{"fidelity": 4, "style": 5, "rationale": "preserves meaning, tone slightly off"}"#;
        let p: JudgePayload = serde_json::from_str(raw).unwrap();
        assert_eq!(p.fidelity, 4);
        assert_eq!(p.style, 5);
        assert!(p.rationale.contains("tone"));
    }

    #[test]
    fn rejects_non_json() {
        let raw = "Yeah looks fine to me.";
        assert!(serde_json::from_str::<JudgePayload>(raw).is_err());
    }
}
