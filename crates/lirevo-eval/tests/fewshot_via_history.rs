//! Experiment: does carrying few-shot examples in `ChatRequest::history` (as
//! real user/assistant turns) avoid the contamination measured in issue
//! #144, where splicing examples as prose into the system prompt
//! (`lirevo_prompts::build_clean_system_prompt_with_examples`) made a 1B
//! model echo a pinned example's `Final` text instead of processing the
//! transcript?
//!
//! Three arms, run for the casual cases only — #144's contamination was
//! casual-only, formal did not regress:
//!   A `baseline` — system = `build_clean_system_prompt(lang)`,        history = []
//!   B `spliced`  — system = `build_clean_system_prompt_with_examples`, history = []
//!                  (byte-for-byte the shipped v0.9 behaviour)
//!   C `history`  — system = `build_clean_system_prompt(lang)`,        history =
//!                  the examples as alternating `User(raw)` / `Assistant(final)`
//!                  turns (the hypothesis under test)
//!
//! This is an experiment, not a regression gate: it asserts only that each
//! arm produced non-empty output, never on the chrF or contamination
//! outcome — read the printed table (`--nocapture`) to see the answer.
//!
//! Skipped unless `LIREVO_EVAL_GGUF_PATH` points at a real GGUF file on
//! disk; see `gguf_smoke.rs`, which this test's backend-loading mirrors.

use std::path::{Path, PathBuf};
use std::sync::Once;

use inference_core::{ChatMessage, ChatRequest, ChatRole, LlamaBackend};
use lirevo_eval::corpus::{load_jsonl, TestCase};
use lirevo_eval::scoring::chrf::chrf;
use lirevo_prompts::{build_clean_system_prompt, build_clean_system_prompt_with_examples};

/// Mirrors `EXAMPLES["casual"]` in `scripts/derive-style-spike-v5.py` exactly
/// — kept byte-identical so this comparison lines up with issue #144's
/// numbers rather than measuring a different set of examples.
fn casual_examples(lang: &str) -> Vec<(String, String)> {
    let pairs: &[(&str, &str)] = match lang {
        "it" => &[
            (
                "scusa il ritardo sto arrivando tra dieci minuti",
                "scusa il ritardo, arrivo tra 10 min",
            ),
            (
                "ok perfetto allora ci sentiamo domani mattina",
                "ok perfetto, ci sentiamo domani mattina",
            ),
            (
                "no guarda oggi non ce la faccio proprio mi dispiace",
                "no guarda oggi non ce la faccio, mi dispiace",
            ),
        ],
        "en" => &[
            (
                "sorry i'm late i'll be there in ten minutes",
                "sorry i'm late, be there in 10",
            ),
            (
                "ok great so we'll talk tomorrow morning then",
                "ok great, talk tomorrow morning",
            ),
            (
                "no honestly i can't make it today i'm sorry",
                "no honestly i can't make it today, sorry",
            ),
        ],
        other => panic!("no casual examples mirrored for language {other}"),
    };
    pairs
        .iter()
        .map(|(raw, final_text)| ((*raw).to_string(), (*final_text).to_string()))
        .collect()
}

static LOAD_LLM_BACKENDS: Once = Once::new();

/// Mirrors the private `ensure_llm_backends_loaded` in
/// `src/backend/gguf.rs` — dlopen's the ggml compute backend modules before
/// the first `LlamaBackend::load`. Duplicated (rather than routed through
/// `GgufBackend`) because `GgufBackend::generate` hardcodes `history: Vec::new()`,
/// which would defeat arm C.
fn ensure_llm_backends_loaded() {
    LOAD_LLM_BACKENDS.call_once(|| {
        if let Some(dir) = inference_core::llm_backends_dir() {
            inference_core::load_llm_backends_from_path(Path::new(dir));
        } else {
            eprintln!(
                "no llama backends dir at build time; LLM may fall back to a non-dynamic backend"
            );
        }
    });
}

/// Mirrors `GgufBackend::load` (`src/backend/gguf.rs`): same env-configurable
/// ctx size, same `n_threads = 0` (backend auto-detects).
fn load_backend(path: PathBuf) -> LlamaBackend {
    ensure_llm_backends_loaded();
    let ctx_size: u32 = std::env::var("LIREVO_EVAL_CTX_SIZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4096);
    LlamaBackend::load(path, ctx_size, 0).expect("load gguf model")
}

/// App-parity request: temperature 0.2, max_tokens 2048, stop = [], and
/// everything else from `ChatRequest::default()` (top_p 0.9, top_k 40) —
/// exactly `app/src-tauri/src/hotkey.rs`'s cleanup-stage call.
fn app_parity_request(system: String, history: Vec<ChatMessage>, user: String) -> ChatRequest {
    ChatRequest {
        system: Some(system),
        user,
        history,
        temperature: 0.2,
        max_tokens: 2048,
        stop: vec![],
        ..ChatRequest::default()
    }
}

/// The hypothesis under test: examples as alternating real chat turns
/// instead of prose in the system prompt.
fn history_from_examples(examples: &[(String, String)]) -> Vec<ChatMessage> {
    examples
        .iter()
        .flat_map(|(raw, final_text)| {
            [
                ChatMessage {
                    role: ChatRole::User,
                    content: raw.clone(),
                },
                ChatMessage {
                    role: ChatRole::Assistant,
                    content: final_text.clone(),
                },
            ]
        })
        .collect()
}

/// First ~25 normalized (trimmed + lowercased) chars of `text` — a
/// partial-copy fingerprint short enough that a truncated echo still matches.
fn contamination_fingerprint(text: &str) -> String {
    let normalized = text.trim().to_lowercase();
    match normalized.char_indices().nth(25) {
        Some((byte_idx, _)) => normalized[..byte_idx].to_string(),
        None => normalized,
    }
}

/// Returns the contaminating example's `final` text if `candidate` contains
/// any example's ~25-char fingerprint (a full copy contains its own
/// fingerprint too, so one check covers both full and partial copies).
fn contaminating_example<'a>(candidate: &str, examples: &'a [(String, String)]) -> Option<&'a str> {
    let candidate_norm = candidate.trim().to_lowercase();
    examples.iter().find_map(|(_, final_text)| {
        let fingerprint = contamination_fingerprint(final_text);
        (!fingerprint.is_empty() && candidate_norm.contains(&fingerprint))
            .then_some(final_text.as_str())
    })
}

struct ArmResult {
    arm: &'static str,
    chrf: f64,
    contaminated: Option<String>,
    candidate: String,
}

#[test]
fn fewshot_via_history_experiment() {
    let Ok(model_path) = std::env::var("LIREVO_EVAL_GGUF_PATH") else {
        eprintln!("skip: LIREVO_EVAL_GGUF_PATH not set");
        return;
    };

    let corpus_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/corpus/style-spike-v5.jsonl");
    let cases: Vec<TestCase> = load_jsonl(&corpus_path)
        .expect("load style-spike-v5 corpus")
        .into_iter()
        .filter(|c| c.id.contains("chat_casual") && c.id.ends_with("-baseline"))
        .collect();
    assert_eq!(
        cases.len(),
        4,
        "expected exactly 4 casual baseline cases (it x2, en x2)"
    );

    let backend = load_backend(PathBuf::from(model_path));

    println!(
        "\nfewshot-via-history experiment — casual cases, app-parity sampler \
         (temp=0.2, top_p=0.9, top_k=40, max_tokens=2048)\n"
    );

    let mut all_results: Vec<(TestCase, Vec<ArmResult>)> = Vec::new();

    for case in cases {
        let examples = casual_examples(&case.language);

        let arms: [(&'static str, ChatRequest); 3] = [
            (
                "A baseline",
                app_parity_request(
                    build_clean_system_prompt(&case.language),
                    vec![],
                    case.transcript.clone(),
                ),
            ),
            (
                "B spliced",
                app_parity_request(
                    build_clean_system_prompt_with_examples(&case.language, &examples),
                    vec![],
                    case.transcript.clone(),
                ),
            ),
            (
                "C history",
                app_parity_request(
                    build_clean_system_prompt(&case.language),
                    history_from_examples(&examples),
                    case.transcript.clone(),
                ),
            ),
        ];

        let mut results = Vec::new();
        for (arm_name, req) in arms {
            let resp = backend
                .chat_sync(req)
                .unwrap_or_else(|e| panic!("case {}, arm {arm_name}: {e}", case.id));
            assert!(
                !resp.text.trim().is_empty(),
                "case {}, arm {arm_name}: model produced empty output",
                case.id
            );

            let score = chrf(&resp.text, &case.expected, 6, 2.0);
            let contaminated =
                contaminating_example(&resp.text, &examples).map(ToString::to_string);
            results.push(ArmResult {
                arm: arm_name,
                chrf: score,
                contaminated,
                candidate: resp.text,
            });
        }

        println!("case {}  [{}]", case.id, case.language);
        println!("  transcript: {}", case.transcript);
        println!("  expected:   {}", case.expected);
        for r in &results {
            println!(
                "  {:<10} chrF={:.4}  contaminated={}  candidate={:?}",
                r.arm,
                r.chrf,
                if r.contaminated.is_some() {
                    "yes"
                } else {
                    "no "
                },
                r.candidate,
            );
            if let Some(hit) = &r.contaminated {
                println!("             ^ matched example final: {hit:?}");
            }
        }
        println!();

        all_results.push((case, results));
    }

    println!("{:-<90}", "");
    println!(
        "{:<26} {:<12} {:>8}  {:<12}",
        "case", "arm", "chrF", "contaminated"
    );
    println!("{:-<90}", "");
    for (case, results) in &all_results {
        for r in results {
            println!(
                "{:<26} {:<12} {:>8.4}  {:<12}",
                case.id,
                r.arm,
                r.chrf,
                if r.contaminated.is_some() {
                    "yes"
                } else {
                    "no"
                },
            );
        }
    }
    println!("{:-<90}", "");
}
