//! Host build script.
//!
//! In addition to the usual `tauri_build::build()`, this re-propagates the
//! runtime dylib search paths (rpaths) for the two inference engines we now
//! link as ggml **dynamic backends**:
//!
//!   * `parakeet-cpp` (STT) — `libparakeet.dylib` + its `libggml*.dylib`.
//!   * `llama-cpp-2`  (LLM) — `libllama.dylib` + its `libggml*.dylib`.
//!
//! Two problems to solve so the final HOST binary runs both engines:
//!
//! 1. **rpath propagation.** Cargo does NOT forward a *-sys crate's
//!    `cargo:rustc-link-arg` to the final binary's link step, so the rpaths the
//!    sys crates emit for their OWN link units never reach the host binary.
//!    Without them the host links fine but cannot resolve
//!    `@rpath/libllama.dylib` / `@rpath/libparakeet.dylib` at launch. We re-emit
//!    the rpaths here, reading the dirs from the sys crates' `links` metadata
//!    (`DEP_PARAKEET_*` / `DEP_LLAMA_*`).
//!
//!    Cargo only exposes a `links` crate's `DEP_<NAME>_<KEY>` metadata to the
//!    build scripts of crates that depend on it as a *normal* (`[dependencies]`)
//!    dependency. The host reaches the sys crates only transitively, so
//!    `Cargo.toml` declares `parakeet-cpp-sys` and `llama-cpp-sys-2` as direct
//!    normal deps purely to receive their metadata here (features matched so
//!    cargo unifies and does not rebuild the native libs twice).
//!
//! 2. **dual-ggml install-name collision.** Both engines ship a
//!    `libggml-base.0.dylib` / `libggml.0.dylib` with the SAME `@rpath/...`
//!    install name but DIFFERENT, ABI-incompatible ggml versions (llama's ggml
//!    0.9.x vs parakeet's 0.13.x). Under two-level namespacing, dyld dedups by
//!    install name, so only ONE `@rpath/libggml-base.0.dylib` is ever loaded —
//!    and BOTH engines (plus their dlopen'd backend modules) bind to it. The
//!    loser then jumps through a wrong/absent vtable slot and SIGSEGVs on first
//!    model load.
//!
//!    The host binary itself links llama's ggml DIRECTLY (it depends on
//!    `llama-cpp-sys-2`), so its own `@rpath/libggml-base.0.dylib` load command
//!    is llama's version — i.e. the bare name MUST stay llama's. We therefore
//!    disambiguate the OTHER engine: stage parakeet's `libparakeet.dylib` +
//!    `libggml*` dylibs + backend modules into a private dir under `OUT_DIR`
//!    with a `lirevo_pk_` install-name prefix on the ggml leaves (via
//!    `install_name_tool`), and point the host's parakeet rpath +
//!    `LIREVO_PARAKEET_BACKENDS_DIR` at that staged dir. llama keeps the plain
//!    `@rpath/libggml-base.0.dylib`; parakeet uses the renamed leaves; dyld
//!    loads both, and each engine binds to its own ggml.
//!
//!    NOTE (Phase 3 bundling): the shipped `.app` must apply the SAME
//!    disambiguation when the dylibs are relocated into the bundle. This build
//!    step is the dev/CI equivalent that makes the host binary actually run.
//!
//! We also surface the loadable-backend-modules directories to runtime Rust
//! (`LIREVO_PARAKEET_BACKENDS_DIR` / `LIREVO_LLAMA_BACKENDS_DIR`) so
//! `engine::backend::BackendManager` can point each engine's
//! `ggml_backend_load_all_from_path` at the right place before the first model
//! load — enabling runtime Metal/CPU selection.

use std::path::{Path, PathBuf};

fn main() {
    tauri_build::build();

    emit_inference_rpaths();
}

fn emit_inference_rpaths() {
    let is_unix = cfg!(any(target_os = "macos", target_os = "linux"));

    // --- llama-cpp-2 (LLM): plain rpaths ------------------------------------
    // The host links llama's ggml directly, so the bare `@rpath/libggml*.dylib`
    // names must resolve to llama's ggml — keep them un-renamed.
    let llama_root = std::env::var("DEP_LLAMA_ROOT").ok().map(PathBuf::from);
    let llama_backends = std::env::var("DEP_LLAMA_BACKENDS_DIR")
        .ok()
        .map(PathBuf::from);
    if let Some(root) = &llama_root {
        if is_unix {
            let lib_dir = root.join("lib");
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());
        }
    }
    if let Some(backends_dir) = &llama_backends {
        if is_unix {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", backends_dir.display());
        }
        println!(
            "cargo:rustc-env=LIREVO_LLAMA_BACKENDS_DIR={}",
            backends_dir.display()
        );
    }

    // --- parakeet-cpp (STT): renamed ggml to dodge the llama collision ------
    let pk_rpath = std::env::var("DEP_PARAKEET_RPATH").ok();
    let pk_backends = std::env::var("DEP_PARAKEET_BACKENDS_DIR")
        .ok()
        .map(PathBuf::from);

    if cfg!(target_os = "macos") {
        // Stage parakeet's dylibs + modules with renamed ggml leaves.
        let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
        match stage_parakeet_engine(
            pk_rpath.as_deref(),
            pk_backends.as_deref(),
            &out_dir.join("parakeet_engine"),
        ) {
            Ok((rpath_dirs, backends_dir)) => {
                for dir in &rpath_dirs {
                    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.display());
                }
                println!(
                    "cargo:rustc-env=LIREVO_PARAKEET_BACKENDS_DIR={}",
                    backends_dir.display()
                );
            }
            Err(e) => {
                println!("cargo:warning=lirevo: parakeet ggml staging failed: {e}");
                fallback_parakeet_rpaths(pk_rpath.as_deref(), pk_backends.as_deref());
            }
        }
    } else {
        // Non-macOS: plain rpaths (the SONAME-collision handling is deferred to
        // the v2 bundling work; a single ggml is present on these targets today).
        fallback_parakeet_rpaths(pk_rpath.as_deref(), pk_backends.as_deref());
    }
}

/// Emit plain (un-renamed) parakeet rpaths + backends-dir env. Used on non-macOS
/// and as a macOS fallback if staging fails.
fn fallback_parakeet_rpaths(rpath: Option<&str>, backends: Option<&Path>) {
    let is_unix = cfg!(any(target_os = "macos", target_os = "linux"));
    if let Some(rpath) = rpath {
        if is_unix {
            for dir in rpath.split(';').filter(|d| !d.is_empty()) {
                println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
            }
        }
    }
    if let Some(backends_dir) = backends {
        println!(
            "cargo:rustc-env=LIREVO_PARAKEET_BACKENDS_DIR={}",
            backends_dir.display()
        );
    }
}

/// Stage parakeet's dylibs/modules into `dst`, renaming the `libggml*` dylibs
/// with a `lirevo_pk_` install-name prefix so they no longer collide with
/// llama's identically-named ggml, and rewriting every cross-reference
/// (`libparakeet` -> ggml, ggml -> ggml-base, backend modules -> ggml-base).
/// Re-runs each build (cheap file copies) so it stays correct if the sys crate
/// rebuilds.
///
/// `pk_rpath` is the `;`-joined DEP_PARAKEET_RPATH (dirs holding parakeet's
/// dylibs, incl. `libparakeet.dylib`); `pk_backends` is the loadable-modules
/// dir. Returns `(rpath_dirs_to_emit, staged_backends_dir)`.
fn stage_parakeet_engine(
    pk_rpath: Option<&str>,
    pk_backends: Option<&Path>,
    dst: &Path,
) -> std::io::Result<(Vec<PathBuf>, PathBuf)> {
    use std::process::Command;

    const PREFIX: &str = "lirevo_pk_";

    let dst_lib = dst.join("lib");
    std::fs::create_dir_all(&dst_lib)?;

    let rename = |name: &str| -> String {
        if name.starts_with("libggml") && name.ends_with(".dylib") {
            format!("{PREFIX}{name}")
        } else {
            name.to_string()
        }
    };

    // Collect the source dirs: every DEP_PARAKEET_RPATH dir (dylibs +
    // libparakeet) plus the backends dir (the loadable `.so` modules). We copy
    // ALL of them into a single flat staged `lib/` so one rpath covers it.
    let mut src_dirs: Vec<PathBuf> = Vec::new();
    if let Some(rpath) = pk_rpath {
        for d in rpath.split(';').filter(|s| !s.is_empty()) {
            src_dirs.push(PathBuf::from(d));
        }
    }
    if let Some(b) = pk_backends {
        src_dirs.push(b.to_path_buf());
    }

    // Copy dylibs/so, resolving symlinks, renaming ggml dylibs. Dedup by output
    // name (the rpath dirs overlap with build/ subtrees).
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for src in &src_dirs {
        let Ok(rd) = std::fs::read_dir(src) else {
            continue;
        };
        for entry in rd.flatten() {
            let ft = entry.file_type()?;
            if !ft.is_file() && !ft.is_symlink() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if !(name.ends_with(".dylib") || name.ends_with(".so")) {
                continue;
            }
            let out_name = rename(&name);
            if !seen.insert(out_name.clone()) {
                continue;
            }
            let real = std::fs::canonicalize(entry.path())?;
            std::fs::copy(&real, dst_lib.join(&out_name))?;
        }
    }

    let ggml_old_to_new = |dep: &str| -> Option<String> {
        let leaf = dep.rsplit('/').next().unwrap_or(dep);
        if leaf.starts_with("libggml") && leaf.ends_with(".dylib") {
            Some(format!("@rpath/{PREFIX}{leaf}"))
        } else {
            None
        }
    };

    let fix = |path: &Path| -> std::io::Result<()> {
        let _ = Command::new("chmod").arg("u+w").arg(path).status();
        let fname = path.file_name().unwrap().to_string_lossy().to_string();

        if fname.starts_with(PREFIX) {
            let id_out = Command::new("otool")
                .args(["-D", path.to_str().unwrap()])
                .output()?;
            let id = String::from_utf8_lossy(&id_out.stdout);
            if let Some(cur_id) = id.lines().nth(1).map(str::trim).filter(|s| !s.is_empty()) {
                if let Some(new_id) = ggml_old_to_new(cur_id) {
                    Command::new("install_name_tool")
                        .args(["-id", &new_id, path.to_str().unwrap()])
                        .status()?;
                }
            }
        }

        let l_out = Command::new("otool")
            .args(["-L", path.to_str().unwrap()])
            .output()?;
        let l = String::from_utf8_lossy(&l_out.stdout);
        for line in l.lines() {
            let dep = line.split_whitespace().next().unwrap_or("");
            if dep.starts_with("@rpath/libggml") && dep.ends_with(".dylib") {
                if let Some(new_dep) = ggml_old_to_new(dep) {
                    if new_dep != dep {
                        Command::new("install_name_tool")
                            .args(["-change", dep, &new_dep, path.to_str().unwrap()])
                            .status()?;
                    }
                }
            }
        }
        Ok(())
    };

    let staged: Vec<PathBuf> = std::fs::read_dir(&dst_lib)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "dylib" || e == "so"))
        .collect();
    for p in &staged {
        fix(p)?;
    }
    // Re-sign ad-hoc (install_name_tool invalidates signatures).
    for p in &staged {
        let _ = Command::new("codesign")
            .args(["-f", "-s", "-", p.to_str().unwrap()])
            .status();
    }

    for src in &src_dirs {
        println!("cargo:rerun-if-changed={}", src.display());
    }

    // One rpath dir (the staged `lib/`) holds libparakeet, its ggml, AND the
    // backend modules — so it doubles as the backends dir.
    Ok((vec![dst_lib.clone()], dst_lib))
}
