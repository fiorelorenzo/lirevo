# lirevo-eval — Refiner model evaluation harness

Dev-only Rust binary that benchmarks candidate LLMs for the dictation
refiner stage. Reads a multilingual `(transcript, profile, language) →
expected` corpus, runs each candidate backend over it, and produces a
markdown + JSON report comparing chrF, semantic cosine, deterministic
assertions, cold/warm latency, peak RSS, and (optionally) LLM-as-judge
fidelity/style scores.

This crate is **never** linked into the shipped Tauri app.

## Quick start

```sh
# 1. Run a bake-off across two local GGUF models.
cargo run -p lirevo-eval --release -- run \
  --corpus crates/lirevo-eval/data/corpus/v1-seed.jsonl \
  --profiles crates/lirevo-eval/data/profiles/v1.toml \
  --backends "gguf:qwen3-4b@$MODELS/Qwen3-4B-Instruct-2507-Q4_K_M.gguf,gguf:gemma3-1b@$MODELS/gemma-3-1b-it-Q4_K_M.gguf" \
  --out crates/lirevo-eval/data/reports/$(date +%Y-%m-%d)-bake-off.md

# 2. (Optional) Re-score with Claude as judge.
cargo run -p lirevo-eval --release -- judge \
  --report crates/lirevo-eval/data/reports/$(date +%Y-%m-%d)-bake-off.json \
  --judge claude-p:claude-3-5-sonnet \
  --out crates/lirevo-eval/data/reports/$(date +%Y-%m-%d)-judged.md

# 3. (Optional) Grow the corpus with an oracle.
cargo run -p lirevo-eval --release -- gen-corpus \
  --oracle claude-p:claude-3-5-sonnet \
  --seeds crates/lirevo-eval/data/corpus/v1-seed.jsonl \
  --profiles crates/lirevo-eval/data/profiles/v1.toml \
  --target-per-cell 4 \
  --out crates/lirevo-eval/data/corpus/v1.jsonl
# Then: review the new lines manually before `git add`.

# 4. (Optional) Enable semantic embedding scoring.
# Add --embed to any `run` invocation. On first run, the ONNX MiniLM is
# downloaded into crates/lirevo-eval/.cache/ (gitignored) and its SHA-256 is
# logged. Paste those hashes into data/profiles/v1.toml's
# [scoring.embedding] block to pin them for future runs.
```

`$MODELS` is wherever your local `.gguf` files live —
typically `~/Library/Application Support/ai.lirevo.app/models/`.

## Backend specs

Format: `<kind>:<id>[@<path>]`

- `gguf:<id>@<path>` — loads a GGUF model via `inference-core::LlamaBackend`,
  the same code path used by the shipped app. Example:
  `gguf:qwen3-4b@/Users/me/.../Qwen3-4B-Q4_K_M.gguf`.
- `claude-p:<model>` — shells out to `claude -p` (the Anthropic CLI). The id
  doubles as the `--model` argument. Example: `claude-p:claude-3-5-sonnet`.

`parse_spec` validates strictly: empty kind/id/path are rejected, `@` is
forbidden on `claude-p`, and whitespace is trimmed.

## Adding a backend

Implement `EvalBackend` in `src/backend/<name>.rs`, register a parse-spec
match arm, and wire the factory branch in `build_from_spec`. No other
module needs to change. See `gguf.rs` for the cleanest example.

## Adding a profile

Edit `data/profiles/v1.toml`. A profile must declare a `[profile.<id>]`
table with `post_assertions = [...]` (may be empty) and a
`[profile.<id>.system_prompts]` table containing **all five** language
keys: `en`, `it`, `fr`, `de`, `es`. System prompts are written in their
target language — the model never has to translate the instruction.

Validation: `cargo test -p lirevo-eval --test data_validates`.

## Adding a test case

Append a JSONL line to `data/corpus/v1-seed.jsonl` (or `v1.jsonl` for
oracle-expanded entries) following the schema:

```json
{"id":"<lang>-<profile>-NNN","language":"<lang>","profile":"<profile>","transcript":"...","expected":"...","tags":[],"notes":""}
```

Run `cargo test -p lirevo-eval --test data_validates` to confirm the new
case references a known profile + language. Two cases per
`(profile, language)` is the seed minimum; aim for 4 in V1.

## Output

Each `run` writes:
- `<out>.md` — human-readable summary with worst-cases table.
- `<out>.json` — machine-readable sidecar, consumable by `lirevo-eval judge`.

Both go into `data/reports/` by convention. Reports are text-only and
committed to the repo as a historical audit trail.

## Notes

- `ort` v2 with default features fetches an ONNX Runtime native lib at
  build time. First clean build of `lirevo-eval` needs network access.
- The latency probe currently runs all 5 cells per backend as cold-equivalent
  for GGUF backends — true KV-cache warm reuse is deferred (would require
  exposing state save/load on `inference-core::LlamaBackend`).
- macOS RSS via `mach_task_info`; on Linux/Windows the probe returns `None`.
- The `claude` binary must be in `PATH` for `claude-p:` backends.
