//! End-to-end smoke for `lda-eval run`. Skipped without `LDA_EVAL_GGUF_PATH`.

use std::process::Command;

#[test]
fn run_produces_markdown_and_json() {
    let Ok(model) = std::env::var("LDA_EVAL_GGUF_PATH") else {
        eprintln!("skip: LDA_EVAL_GGUF_PATH not set");
        return;
    };
    let manifest = env!("CARGO_MANIFEST_DIR");
    let tmp = tempfile::tempdir().unwrap();
    let md = tmp.path().join("smoke.md");
    let status = Command::new(env!("CARGO_BIN_EXE_lda-eval"))
        .args([
            "run",
            "--corpus",
            &format!("{manifest}/data/corpus/v1-seed.jsonl"),
            "--profiles",
            &format!("{manifest}/data/profiles/v1.toml"),
            "--backends",
            &format!("gguf:smoke@{model}"),
            "--out",
            md.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(md.exists());
    assert!(md.with_extension("json").exists());
    let body = std::fs::read_to_string(&md).unwrap();
    assert!(body.contains("# Refiner bake-off"));
    assert!(body.contains("gguf:smoke"));
}
