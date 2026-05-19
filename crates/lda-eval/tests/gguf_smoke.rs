//! Smoke test for `GgufBackend`.
//!
//! Skipped unless the env var `LDA_EVAL_GGUF_PATH` points at a real GGUF
//! file on disk. This lets `cargo test` succeed in environments without
//! a multi-GB model.

use lda_eval::backend::{build_from_spec, GenerateReq};

#[tokio::test(flavor = "multi_thread")]
async fn gguf_round_trip_returns_text() {
    let Ok(path) = std::env::var("LDA_EVAL_GGUF_PATH") else {
        eprintln!("skip: LDA_EVAL_GGUF_PATH not set");
        return;
    };
    let spec = format!("gguf:smoke@{path}");
    let backend = build_from_spec(&spec).await.expect("build");
    let out = backend
        .generate(GenerateReq {
            system_prompt: "You are a helpful assistant.".into(),
            transcript: "Say the single word OK.".into(),
            max_tokens: 16,
            temperature: 0.0,
        })
        .await
        .expect("generate");
    assert!(!out.text.is_empty(), "expected non-empty output");
    assert!(out.latency_ms > 0);
}
