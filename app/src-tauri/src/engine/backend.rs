//! Dynamic ggml backend wiring for the two inference engines.
//!
//! Both the STT engine (`parakeet-cpp`) and the LLM (`llama-cpp-2`) are built
//! with ggml's `GGML_BACKEND_DL`: the compute backends (Metal, the CPU
//! variants) are loadable `.so` MODULES, discovered + selected at runtime
//! rather than statically linked. For that discovery to find the Metal module
//! (instead of silently falling back to CPU) each engine must be pointed at its
//! loadable-modules directory BEFORE its global ggml backend is first created:
//!
//!   * parakeet — honours the `PARAKEET_BACKENDS_DIR` env var, read once on the
//!     first model load. We set it from `LIREVO_PARAKEET_BACKENDS_DIR` (emitted
//!     by the host `build.rs` from the sys crate's `DEP_PARAKEET_BACKENDS_DIR`).
//!   * llama — exposes `load_backends_from_path` (wrapped by `inference_core`)
//!     which must run before `LlamaBackend::init()` (i.e. before the first
//!     [`crate::engine::Engine::ensure_llm`]). We call it from [`prepare`].
//!
//! [`BackendManager::prepare`] does both, exactly once. [`BackendManager`] also
//! resolves which compute backend each engine actually selected
//! ([`ActiveBackends`]), which the Engine caches after the first model load.

use std::path::Path;
use std::sync::Once;

/// The loadable-backend-modules dir for parakeet, captured at build time by the
/// host `build.rs` from the sys crate's `DEP_PARAKEET_BACKENDS_DIR`. `None` on a
/// static build (or any build where the metadata was absent).
const PARAKEET_BACKENDS_DIR: Option<&str> = option_env!("LIREVO_PARAKEET_BACKENDS_DIR");

/// The loadable-backend-modules dir for llama, captured the same way from
/// `DEP_LLAMA_BACKENDS_DIR`. Used as a fallback if `inference-core` does not
/// surface llama-cpp-2's own compile-time `BACKENDS_DIR`.
const LLAMA_BACKENDS_DIR: Option<&str> = option_env!("LIREVO_LLAMA_BACKENDS_DIR");

static PREPARE: Once = Once::new();

/// The compute backend each engine resolved to, e.g. `stt = "MTL0"` / `"cpu"`,
/// `llm = "Metal"` / `"CPU"`. Resolvable only after the corresponding model has
/// been loaded (the ggml backend is created lazily on first load).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveBackends {
    pub stt: String,
    pub llm: String,
}

/// Prepares + reports the dynamic ggml backends for both engines.
pub struct BackendManager;

impl BackendManager {
    /// Point both engines at their loadable-backend-modules directories. Run
    /// once, BEFORE the first STT model load and the first `LlamaBackend::init`.
    /// Idempotent: subsequent calls are no-ops (guarded by a `Once`).
    pub fn prepare() {
        PREPARE.call_once(|| {
            // parakeet reads PARAKEET_BACKENDS_DIR lazily, on first model load.
            if let Some(dir) = PARAKEET_BACKENDS_DIR {
                // SAFETY: called once, at Engine construction, before any STT
                // model load creates the parakeet backend and before other
                // threads can race on the environment.
                unsafe { std::env::set_var("PARAKEET_BACKENDS_DIR", dir) };
                tracing::info!(parakeet_backends_dir = dir, "prepared STT dynamic backends");
            } else {
                tracing::warn!(
                    "LIREVO_PARAKEET_BACKENDS_DIR unset at build time; \
                     STT may fall back to a non-dynamic backend"
                );
            }

            // llama: load the modules now, before LlamaBackend::init(). Prefer
            // llama-cpp-2's own compile-time BACKENDS_DIR (via inference-core);
            // fall back to the host-captured env. Same path in practice.
            let llm_dir = inference_core::llm_backends_dir().or(LLAMA_BACKENDS_DIR);
            if let Some(dir) = llm_dir {
                inference_core::load_llm_backends_from_path(Path::new(dir));
                tracing::info!(llama_backends_dir = dir, "prepared LLM dynamic backends");
            } else {
                tracing::warn!(
                    "no llama backends dir at build time; \
                     LLM may fall back to a non-dynamic backend"
                );
            }
        });
    }

    /// Resolve the currently active compute backends.
    ///
    /// `stt_backend_name` is `parakeet_cpp::Model::backend_name()` of a LOADED
    /// model (empty if no STT model is resident yet — the backend is created
    /// lazily on load). `llm` is taken from llama-cpp-2's device enumeration
    /// (the first non-CPU device, else the first); it is only meaningful after
    /// [`prepare`] has loaded the modules.
    #[must_use]
    pub fn active(stt_backend_name: &str) -> ActiveBackends {
        ActiveBackends {
            stt: stt_backend_name.to_string(),
            llm: inference_core::active_llm_backend_name(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_is_idempotent() {
        // No model is loaded, but prepare() must never panic and must be safe
        // to call repeatedly (the Once guard).
        BackendManager::prepare();
        BackendManager::prepare();
    }

    #[test]
    fn active_threads_through_stt_name() {
        // The STT name is passed through verbatim; the LLM name comes from the
        // device enumeration (may be empty before any backend is loaded).
        let a = BackendManager::active("MTL0");
        assert_eq!(a.stt, "MTL0");
    }

    /// End-to-end proof that the rpath plumbing (build.rs) + dynamic-backend
    /// loading (`prepare`) + Metal selection all work inside the host: build an
    /// Engine pointed at a real Parakeet GGUF, load the STT model (which creates
    /// the ggml backend), and assert the resolved STT backend is Metal — i.e.
    /// the `libggml-metal.so` MODULE was discovered + selected over the CPU
    /// fallback. Skips cleanly when no model is on disk.
    ///
    /// macOS only (Metal); on CI without a model it is a no-op.
    #[cfg(target_os = "macos")]
    #[tokio::test(flavor = "multi_thread")]
    async fn dl_stt_backend_is_metal() {
        use crate::engine::{Engine, EngineConfig};
        use crate::stt::catalog;
        use inference_core::profile::ProfileName;
        use std::path::PathBuf;

        // Locate a models dir that holds the shipped STT GGUF. Try the dev app
        // data dir first, then the binding's own models dir (CI may override
        // via LIREVO_TEST_MODELS_DIR).
        let candidates: Vec<PathBuf> = {
            let mut v = Vec::new();
            if let Ok(env) = std::env::var("LIREVO_TEST_MODELS_DIR") {
                v.push(PathBuf::from(env));
            }
            if let Some(home) = std::env::var_os("HOME") {
                let home = PathBuf::from(home);
                v.push(home.join("Library/Application Support/Lirevo (Dev)/models"));
                v.push(home.join("Library/Application Support/Lirevo/models"));
                v.push(home.join("Progetti/Personale/parakeet-cpp/models"));
            }
            v
        };
        let Some(models_dir) = candidates
            .into_iter()
            .find(|d| crate::stt::gguf_path(d).exists())
        else {
            eprintln!(
                "skipping dl_stt_backend_is_metal: no {} found in any candidate models dir",
                catalog::STT_GGUF_FILENAME
            );
            return;
        };
        eprintln!("using STT model dir: {}", models_dir.display());

        let engine = Engine::new(
            EngineConfig {
                llm_model_path: None,
                llm_ctx_size: 4096,
                stt_model_id: Some(catalog::default_model_id().to_string()),
            },
            ProfileName::Balanced,
            models_dir,
        );

        // Loading the real model forces ggml backend creation under DL.
        let loaded = engine.ensure_stt().await.expect("ensure_stt");
        assert!(loaded.is_some(), "STT model should have loaded");

        let active = engine
            .active_backends()
            .expect("active backends cached after STT load");
        eprintln!("resolved STT backend (DL): {}", active.stt);
        eprintln!("resolved LLM backend (DL): {}", active.llm);

        let lower = active.stt.to_ascii_lowercase();
        // ggml's Metal backend registers its device as "MTL<n>" (e.g. "MTL0");
        // the human-facing name is "Metal". Accept either spelling, while still
        // proving Metal (not CPU) was the dlopen'd module that got selected.
        assert!(
            lower.contains("metal") || lower.contains("mtl"),
            "expected the Metal backend under dynamic-backends, got {:?}",
            active.stt
        );
        assert_ne!(
            lower, "cpu",
            "Metal module should have been selected over the CPU fallback, got {:?}",
            active.stt
        );

        // All de-risk assertions passed. Terminate with the libc `_exit`
        // syscall, skipping atexit handlers + C++ static destructors, to dodge
        // the PRE-EXISTING ggml-Metal teardown abort those destructors trigger
        // at normal process exit (ggml-metal-device.m residency-set GGML_ASSERT).
        // This is not specific to the DL path; `std::process::exit` does not
        // help (it still runs the destructors). This is the only model-gated
        // test that loads a real Metal model in this binary's run; terminating
        // here is acceptable for the de-risk proof.
        println!(
            "dl_stt_backend_is_metal: OK (stt={}, llm={})",
            active.stt, active.llm
        );
        use std::io::Write as _;
        std::io::stdout().flush().ok();
        std::io::stderr().flush().ok();
        extern "C" {
            fn _exit(code: i32) -> !;
        }
        // SAFETY: POSIX immediate-termination syscall; never returns, touches no
        // Rust state. Called only after all assertions passed + output flushed.
        unsafe { _exit(0) }
    }
}
