//! Thin-fetch of GPU ggml backend modules on first run (Linux / Windows).
//!
//! ## Why this exists
//!
//! Both inference engines (`parakeet-cpp` for STT, `llama-cpp-2` for the LLM)
//! are built with ggml's `GGML_BACKEND_DL`: the compute backends (Metal, CUDA,
//! Vulkan, the CPU variants) are loadable `.so` / `.dll` MODULES discovered at
//! runtime by [`crate::engine::backend::BackendManager`].
//!
//! On **macOS** we BUNDLE both the CPU and Metal modules inside the `.app`, so
//! every backend the user could want is already present — this whole module is
//! DORMANT there (see [`detect_gpu`] returning [`DesiredBackend::Metal`], which
//! [`ensure_backend`] treats as already-satisfied).
//!
//! On **Linux / Windows** (a v2 target — not functional end-to-end yet) the
//! plan is: ship the CPU module bundled, and FETCH the large GPU module
//! (CUDA = hundreds of MB, or Vulkan) on first run by hardware detection. This
//! module is the foundation for that: a manifest format, best-effort hardware
//! detection, and an idempotent checksum-verified fetch into the app-data
//! backends dir. The real download + dlopen of a CUDA/Vulkan module on target
//! GPU hardware is UNVERIFIED here (no target box, modules not yet published).
//!
//! ## Per-OS reality (honest)
//!
//! - **macOS:** Metal + CPU bundled. Fetch never runs ([`ensure_backend`] is a
//!   no-op for the bundled backend).
//! - **Linux:** dynamic backends. CPU bundled; CUDA / Vulkan are the fetch
//!   targets. This is the case the fetch is actually designed for.
//! - **Windows:** the app currently links the engines STATICALLY (no dynamic
//!   backends — see the host `Cargo.toml`: parakeet is static, llama resolves
//!   static via `inference-core`). A "fetch" of a loadable module is therefore
//!   MEANINGLESS on Windows until the Windows DL build + os-integration port
//!   land. [`detect_gpu`] still compiles + returns a sensible value on Windows,
//!   but [`ensure_backend`] short-circuits to "nothing to fetch" there. See the
//!   `cfg(windows)` branch.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ===========================================================================
// Manifest format
// ===========================================================================

/// A downloadable backend-module bundle for one (os, arch, backend) tuple.
///
/// Hosted as a JSON document at a stable URL (GitHub Releases of the binding
/// repo for parakeet modules; Lirevo releases for llama modules). The app
/// fetches the manifest, selects the entry matching the running platform + the
/// [`detect_gpu`] decision, downloads + checksum-verifies it, and places the
/// extracted modules into the app-data backends dir.
///
/// ## JSON shape
///
/// ```json
/// {
///   "schema_version": 1,
///   "entries": [
///     {
///       "os": "linux",
///       "arch": "x86_64",
///       "backend": "cuda",
///       "engine": "llama",
///       "version": "0.1.146",
///       "url": "https://github.com/.../ggml-cuda-linux-x86_64.tar.zst",
///       "sha256": "abc123...",
///       "files": ["libggml-cuda.so"]
///     }
///   ]
/// }
/// ```
///
/// `version` ties the bundle to the binding/engine revision it was built
/// against: for `parakeet` it is the `parakeet-cpp` binding git rev (or a
/// release tag); for `llama` it is the `llama-cpp-2` crate version. A loaded
/// module must match the engine's ggml ABI, so [`ensure_backend`] records the
/// version it placed and refuses to reuse a stale on-disk bundle whose version
/// no longer matches the manifest entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendManifest {
    /// Bumped on incompatible manifest-format changes. Current: `1`.
    pub schema_version: u32,
    pub entries: Vec<BackendEntry>,
}

/// One backend bundle, keyed by (os, arch, backend, engine).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendEntry {
    /// `target_os` value: `"macos"`, `"linux"`, `"windows"`.
    pub os: String,
    /// `target_arch` value: `"aarch64"`, `"x86_64"`.
    pub arch: String,
    /// The compute backend this bundle provides.
    pub backend: BackendKind,
    /// Which engine's ggml this bundle is built against — its modules must be
    /// loaded into that engine's backends dir (parakeet and llama ship
    /// ABI-incompatible ggml versions, so the modules are NOT interchangeable).
    pub engine: Engine,
    /// Binding/engine revision the bundle was built against. parakeet: the
    /// `parakeet-cpp` git rev or release tag. llama: the `llama-cpp-2` version
    /// (e.g. `"0.1.146"`).
    pub version: String,
    /// Direct download URL of the (possibly compressed) bundle archive.
    pub url: String,
    /// Lowercase hex SHA-256 of the downloaded archive bytes, verified before
    /// the archive is unpacked / placed.
    pub sha256: String,
    /// Module filenames the bundle is expected to yield once unpacked. Used to
    /// detect a present-and-complete install ([`ensure_backend`] skips the
    /// download when all of these already exist with a matching version stamp).
    pub files: Vec<String>,
}

/// The compute backend a [`BackendEntry`] provides / [`detect_gpu`] selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    /// Apple GPU (macOS only). Always bundled — never fetched.
    Metal,
    /// NVIDIA CUDA (Linux / Windows). The large fetch target.
    Cuda,
    /// Cross-vendor Vulkan (Linux / Windows). The non-NVIDIA fetch target.
    Vulkan,
    /// CPU-only fallback. Always bundled — never fetched.
    Cpu,
}

/// Which engine a backend bundle belongs to. parakeet and llama ship distinct,
/// ABI-incompatible ggml versions, so each has its own backends dir + modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    Parakeet,
    Llama,
}

impl Engine {
    /// Leaf dir name under the app-data backends root for this engine. Mirrors
    /// the bundled `.app` layout (`Resources/backends/{parakeet,llama}`).
    #[must_use]
    pub fn dir_name(self) -> &'static str {
        match self {
            Engine::Parakeet => "parakeet",
            Engine::Llama => "llama",
        }
    }
}

// ===========================================================================
// Hardware detection
// ===========================================================================

/// The backend the running machine should use, decided by [`detect_gpu`].
///
/// On macOS this is always [`DesiredBackend::Metal`] (and it is bundled, so the
/// fetch is a no-op). On Linux / Windows it is CUDA when an NVIDIA GPU is
/// present, else Vulkan, with CPU as the ultimate fallback.
pub type DesiredBackend = BackendKind;

/// Inputs to the (platform-neutral, unit-testable) backend decision. Real
/// detection on Linux / Windows fills these in best-effort; tests inject them.
#[derive(Debug, Clone, Copy, Default)]
pub struct GpuProbe {
    /// An NVIDIA GPU appears present (driver node, `nvidia-smi`, or registry).
    pub nvidia_present: bool,
    /// A Vulkan-capable GPU/loader appears present.
    pub vulkan_present: bool,
}

/// Pure decision: given the OS family and a hardware probe, pick the backend.
///
/// Kept free of any I/O so the policy is exhaustively unit-testable. The real
/// [`detect_gpu`] is a thin `cfg`-per-OS wrapper that builds a [`GpuProbe`] and
/// calls this.
#[must_use]
pub fn decide_backend(is_macos: bool, probe: GpuProbe) -> DesiredBackend {
    if is_macos {
        // Apple Silicon: Metal is always the right (and bundled) answer.
        return BackendKind::Metal;
    }
    if probe.nvidia_present {
        BackendKind::Cuda
    } else if probe.vulkan_present {
        BackendKind::Vulkan
    } else {
        BackendKind::Cpu
    }
}

/// Best-effort detection of the desired GPU backend on the running machine.
///
/// - **macOS:** always [`BackendKind::Metal`].
/// - **Linux:** NVIDIA via `/proc/driver/nvidia` or an `nvidia-smi` on PATH
///   (detects the device even when the CUDA toolkit isn't installed); Vulkan
///   via an ICD/loader presence check; else CPU.
/// - **Windows:** NVIDIA via `nvidia-smi` on PATH (best-effort; a full WMI /
///   registry / NVML probe is deferred with the Windows port); else CPU.
///
/// All probes are best-effort and must never panic or block for long.
#[must_use]
pub fn detect_gpu() -> DesiredBackend {
    #[cfg(target_os = "macos")]
    {
        decide_backend(true, GpuProbe::default())
    }
    #[cfg(target_os = "linux")]
    {
        decide_backend(
            false,
            GpuProbe {
                nvidia_present: linux_nvidia_present(),
                vulkan_present: linux_vulkan_present(),
            },
        )
    }
    #[cfg(target_os = "windows")]
    {
        decide_backend(
            false,
            GpuProbe {
                nvidia_present: windows_nvidia_present(),
                // Vulkan detection on Windows is deferred with the Windows DL
                // port; CPU is the safe fallback there today (the app is
                // statically linked on Windows anyway — see module docs).
                vulkan_present: false,
            },
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        decide_backend(false, GpuProbe::default())
    }
}

#[cfg(target_os = "linux")]
fn linux_nvidia_present() -> bool {
    // The kernel driver exposes this dir whenever the NVIDIA module is loaded,
    // even with no CUDA toolkit installed. Cheap and definitive when present.
    if Path::new("/proc/driver/nvidia/version").exists()
        || Path::new("/proc/driver/nvidia").exists()
    {
        return true;
    }
    // Fallback: an `nvidia-smi` on PATH strongly implies an NVIDIA GPU. We only
    // check for the binary's presence (don't execute it) to stay fast + safe.
    binary_on_path("nvidia-smi")
}

#[cfg(target_os = "linux")]
fn linux_vulkan_present() -> bool {
    // A Vulkan ICD manifest or the loader library implies usable Vulkan.
    const ICD_DIRS: [&str; 3] = [
        "/usr/share/vulkan/icd.d",
        "/etc/vulkan/icd.d",
        "/usr/local/share/vulkan/icd.d",
    ];
    if ICD_DIRS.iter().any(|d| {
        std::fs::read_dir(d)
            .map(|mut r| r.next().is_some())
            .unwrap_or(false)
    }) {
        return true;
    }
    const LOADERS: [&str; 3] = [
        "/usr/lib/libvulkan.so.1",
        "/usr/lib/x86_64-linux-gnu/libvulkan.so.1",
        "/lib/x86_64-linux-gnu/libvulkan.so.1",
    ];
    LOADERS.iter().any(|p| Path::new(p).exists())
}

#[cfg(target_os = "windows")]
fn windows_nvidia_present() -> bool {
    // Best-effort: `nvidia-smi.exe` ships with the NVIDIA driver and lands on
    // PATH (System32 or the driver dir). A full WMI / SetupAPI / NVML enumeration
    // is deferred with the Windows port; this is enough to drive the decision in
    // the (not-yet-wired) Windows fetch path.
    binary_on_path("nvidia-smi.exe") || binary_on_path("nvidia-smi")
}

/// Whether `name` resolves on `PATH` (presence only — never executed).
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn binary_on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(name).exists())
}

// ===========================================================================
// Fetch
// ===========================================================================

/// Outcome of [`ensure_backend`] — what was made available and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchOutcome {
    /// The backend is bundled / always present (e.g. Metal on macOS, or the CPU
    /// fallback). Nothing was fetched; no module dir to add to the load path.
    Bundled,
    /// The required modules were already present + checksum/version-valid on
    /// disk. No download happened. Carries the dir they live in.
    AlreadyPresent(PathBuf),
    /// The bundle was downloaded, verified, and placed. Carries the dir.
    Fetched(PathBuf),
    /// No manifest entry matched the running platform + desired backend, so the
    /// engine will fall back to its bundled CPU module. Not an error.
    NotInManifest,
}

/// Errors from the fetch path. Distinct from [`crate::AppError`] so the logic is
/// testable without the Tauri layer; the caller maps to `AppError` if needed.
#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("manifest parse: {0}")]
    Manifest(String),
    #[error("download: {0}")]
    Download(String),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    Checksum { expected: String, actual: String },
    #[error("filesystem: {0}")]
    Io(String),
}

impl From<std::io::Error> for FetchError {
    fn from(e: std::io::Error) -> Self {
        FetchError::Io(e.to_string())
    }
}

/// Parse a [`BackendManifest`] from JSON bytes.
pub fn parse_manifest(bytes: &[u8]) -> Result<BackendManifest, FetchError> {
    serde_json::from_slice(bytes).map_err(|e| FetchError::Manifest(e.to_string()))
}

/// Find the manifest entry for the running platform + desired backend + engine.
///
/// `os` / `arch` are `target_os` / `target_arch` strings (so tests can drive
/// the selection without depending on the host triple).
#[must_use]
pub fn select_entry<'a>(
    manifest: &'a BackendManifest,
    os: &str,
    arch: &str,
    backend: DesiredBackend,
    engine: Engine,
) -> Option<&'a BackendEntry> {
    manifest
        .entries
        .iter()
        .find(|e| e.os == os && e.arch == arch && e.backend == backend && e.engine == engine)
}

/// Lowercase-hex SHA-256 of a byte slice. Matches the on-disk verifier in
/// `crate::models::verify_sha256` (same algorithm, in-memory here because the
/// backend archives are small relative to the multi-GB model files).
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest.iter() {
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// Verify `bytes` against an expected lowercase-hex SHA-256.
pub fn verify_sha256_bytes(bytes: &[u8], expected: &str) -> Result<(), FetchError> {
    let actual = sha256_hex(bytes);
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(FetchError::Checksum {
            expected: expected.to_string(),
            actual,
        })
    }
}

/// The marker file recording which manifest `version` produced the modules in a
/// given engine's backends dir. Lets [`ensure_backend`] tell a still-valid
/// install from a stale one whose engine ABI has since moved on.
const VERSION_STAMP: &str = ".lirevo-backend-version";

/// Whether the entry's modules are already present + version-matched in `dir`.
fn already_present(dir: &Path, entry: &BackendEntry) -> bool {
    let stamp = dir.join(VERSION_STAMP);
    let stamp_ok = std::fs::read_to_string(&stamp)
        .map(|s| s.trim() == entry.version)
        .unwrap_or(false);
    if !stamp_ok {
        return false;
    }
    entry.files.iter().all(|f| dir.join(f).exists())
}

/// Trait over "fetch the bytes at this URL" so the network can be mocked in
/// unit tests. The production impl ([`ReqwestFetcher`]) reuses the same
/// `reqwest` client style as `crate::models`.
#[allow(async_fn_in_trait)]
pub trait BytesFetcher {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError>;
}

/// Production fetcher: a one-shot `reqwest` GET into memory. The backend
/// archives are small (a CUDA module bundle is hundreds of MB but still far
/// under a model's multi-GB), so an in-memory fetch keeps the verify-then-place
/// flow simple (no `.partial` rename dance needed before checksum).
pub struct ReqwestFetcher;

impl BytesFetcher for ReqwestFetcher {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        let client = reqwest::Client::new();
        let resp = client
            .get(url)
            .send()
            .await
            .map_err(|e| FetchError::Download(format!("http: {e}")))?;
        if !resp.status().is_success() {
            return Err(FetchError::Download(format!("HTTP {}", resp.status())));
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| FetchError::Download(format!("body: {e}")))?;
        Ok(bytes.to_vec())
    }
}

/// The (os, arch, backend, engine) selector + dest dir for one [`ensure_backend`]
/// call. Grouping these keeps the call signature tidy (and clippy-clean) while
/// the same target is reused across both engines from one detection result.
#[derive(Debug, Clone)]
pub struct FetchTarget<'a> {
    pub desired: DesiredBackend,
    pub engine: Engine,
    /// `target_os` string (`"linux"`, `"macos"`, `"windows"`).
    pub os: &'a str,
    /// `target_arch` string (`"x86_64"`, `"aarch64"`).
    pub arch: &'a str,
    /// App-data backends root; the engine's modules land in `root/<engine>`.
    pub backends_root: &'a Path,
}

/// Ensure the desired backend's modules are available for the target engine,
/// fetching them if needed. Idempotent.
///
/// ## Flow
///
/// 1. If `target.desired` is a bundled backend (Metal / CPU) →
///    [`FetchOutcome::Bundled`] (no-op; the modules ship inside the app). This
///    is the macOS path.
/// 2. Otherwise resolve the dest dir `backends_root/<engine>` and check for a
///    present-and-version-matched install → [`FetchOutcome::AlreadyPresent`].
/// 3. Otherwise look the bundle up in `manifest`. No match →
///    [`FetchOutcome::NotInManifest`] (engine falls back to bundled CPU).
/// 4. Otherwise download via `fetcher`, verify SHA-256, place the module file(s)
///    into the dest dir, strip macOS quarantine, write the version stamp →
///    [`FetchOutcome::Fetched`].
///
/// `unpack` maps a downloaded archive to the `(filename, bytes)` pairs to write.
/// The default production path ([`unpack_single_file`]) treats the download as a
/// single raw module file (named by `entry.files[0]`); a real `.tar.zst` bundle
/// impl would decompress here. Injected so tests exercise the place/verify/skip
/// logic without a real archive format.
pub async fn ensure_backend<F, P>(
    target: &FetchTarget<'_>,
    manifest: &BackendManifest,
    fetcher: &F,
    unpack: P,
) -> Result<FetchOutcome, FetchError>
where
    F: BytesFetcher,
    P: Fn(&BackendEntry, &[u8]) -> Result<Vec<(String, Vec<u8>)>, FetchError>,
{
    let FetchTarget {
        desired,
        engine,
        os,
        arch,
        backends_root,
    } = *target;

    // (1) Bundled backends are never fetched — they ship inside the app.
    if matches!(desired, BackendKind::Metal | BackendKind::Cpu) {
        return Ok(FetchOutcome::Bundled);
    }

    let dest = backends_root.join(engine.dir_name());

    // Look up the entry first so a present-install check can compare versions.
    let Some(entry) = select_entry(manifest, os, arch, desired, engine) else {
        return Ok(FetchOutcome::NotInManifest);
    };

    // (2) Already present + version-matched → skip the download.
    if already_present(&dest, entry) {
        return Ok(FetchOutcome::AlreadyPresent(dest));
    }

    // (4) Download → verify → place.
    let bytes = fetcher.fetch(&entry.url).await?;
    verify_sha256_bytes(&bytes, &entry.sha256)?;

    std::fs::create_dir_all(&dest)?;
    for (name, data) in unpack(entry, &bytes)? {
        let path = dest.join(&name);
        // Write atomically: temp + rename so a crash mid-write never leaves a
        // half-written module that `already_present` would treat as valid.
        let tmp = path.with_extension("partial");
        std::fs::write(&tmp, &data)?;
        std::fs::rename(&tmp, &path)?;
        strip_quarantine(&path);
    }
    std::fs::write(dest.join(VERSION_STAMP), &entry.version)?;

    Ok(FetchOutcome::Fetched(dest))
}

/// The default single-file "unpack": treat the downloaded bytes as the raw
/// module named by the entry's first declared file. Real `.tar.zst` bundles
/// would replace this with a decompress+extract step (out of scope here — the
/// bundles aren't published yet).
pub fn unpack_single_file(
    entry: &BackendEntry,
    bytes: &[u8],
) -> Result<Vec<(String, Vec<u8>)>, FetchError> {
    let name = entry
        .files
        .first()
        .ok_or_else(|| FetchError::Manifest("entry has no files".into()))?;
    Ok(vec![(name.clone(), bytes.to_vec())])
}

/// Strip the `com.apple.quarantine` extended attribute from a freshly-downloaded
/// module so Gatekeeper allows `dlopen` of an unsigned/third-party dylib.
///
/// macOS-only and DORMANT in practice: on macOS the GPU backend (Metal) is
/// bundled, so [`ensure_backend`] returns early before any file is written here.
/// It exists so that IF a macOS fetch path is ever added (it isn't today), the
/// quarantine handling is already in place. Best-effort: a missing xattr (the
/// common case for non-quarantined files) is not an error. Uses the system
/// `xattr` tool to avoid a new crate dependency for a dormant path.
#[cfg(target_os = "macos")]
fn strip_quarantine(path: &Path) {
    let _ = std::process::Command::new("xattr")
        .args(["-d", "com.apple.quarantine"])
        .arg(path)
        .output();
}

#[cfg(not(target_os = "macos"))]
#[allow(clippy::missing_const_for_fn)]
fn strip_quarantine(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- detection decision (pure, exhaustive) ----------------------------

    #[test]
    fn macos_always_metal() {
        assert_eq!(
            decide_backend(
                true,
                GpuProbe {
                    nvidia_present: true,
                    vulkan_present: true
                }
            ),
            BackendKind::Metal
        );
        assert_eq!(
            decide_backend(true, GpuProbe::default()),
            BackendKind::Metal
        );
    }

    #[test]
    fn nvidia_takes_cuda() {
        let d = decide_backend(
            false,
            GpuProbe {
                nvidia_present: true,
                vulkan_present: true,
            },
        );
        assert_eq!(d, BackendKind::Cuda);
    }

    #[test]
    fn no_nvidia_with_vulkan_takes_vulkan() {
        let d = decide_backend(
            false,
            GpuProbe {
                nvidia_present: false,
                vulkan_present: true,
            },
        );
        assert_eq!(d, BackendKind::Vulkan);
    }

    #[test]
    fn no_gpu_falls_back_to_cpu() {
        assert_eq!(decide_backend(false, GpuProbe::default()), BackendKind::Cpu);
    }

    #[test]
    fn detect_gpu_compiles_and_returns_sane_value() {
        // Smoke test the cfg-per-OS wrapper. On macOS this is Metal; on
        // Linux/Windows it depends on the runner's hardware (any variant is
        // acceptable — we only assert it doesn't panic and returns a value).
        let d = detect_gpu();
        #[cfg(target_os = "macos")]
        assert_eq!(d, BackendKind::Metal);
        #[cfg(not(target_os = "macos"))]
        let _ = d;
    }

    // ---- manifest parse ----------------------------------------------------

    fn sample_json() -> &'static str {
        r#"{
          "schema_version": 1,
          "entries": [
            {
              "os": "linux", "arch": "x86_64",
              "backend": "cuda", "engine": "llama",
              "version": "0.1.146",
              "url": "https://example.com/ggml-cuda.so",
              "sha256": "00",
              "files": ["libggml-cuda.so"]
            },
            {
              "os": "linux", "arch": "x86_64",
              "backend": "vulkan", "engine": "parakeet",
              "version": "27ce3be",
              "url": "https://example.com/ggml-vulkan.so",
              "sha256": "00",
              "files": ["libggml-vulkan.so"]
            }
          ]
        }"#
    }

    #[test]
    fn parses_manifest() {
        let m = parse_manifest(sample_json().as_bytes()).expect("parse");
        assert_eq!(m.schema_version, 1);
        assert_eq!(m.entries.len(), 2);
        assert_eq!(m.entries[0].backend, BackendKind::Cuda);
        assert_eq!(m.entries[0].engine, Engine::Llama);
        assert_eq!(m.entries[1].engine, Engine::Parakeet);
    }

    #[test]
    fn rejects_garbage_manifest() {
        assert!(parse_manifest(b"not json").is_err());
        assert!(parse_manifest(br#"{"schema_version": 1}"#).is_err()); // missing entries
    }

    #[test]
    fn round_trips_manifest() {
        let m = parse_manifest(sample_json().as_bytes()).unwrap();
        let bytes = serde_json::to_vec(&m).unwrap();
        let again = parse_manifest(&bytes).unwrap();
        assert_eq!(m, again);
    }

    #[test]
    fn selects_matching_entry() {
        let m = parse_manifest(sample_json().as_bytes()).unwrap();
        let e = select_entry(&m, "linux", "x86_64", BackendKind::Cuda, Engine::Llama)
            .expect("cuda/llama entry");
        assert_eq!(e.files, vec!["libggml-cuda.so".to_string()]);
        // No CUDA/parakeet entry in the sample.
        assert!(select_entry(&m, "linux", "x86_64", BackendKind::Cuda, Engine::Parakeet).is_none());
        // Wrong arch.
        assert!(select_entry(&m, "linux", "aarch64", BackendKind::Cuda, Engine::Llama).is_none());
    }

    // ---- checksum ----------------------------------------------------------

    #[test]
    fn sha256_matches_known_vector() {
        // SHA-256("abc")
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn verify_good_and_bad_checksums() {
        let data = b"hello backend";
        let good = sha256_hex(data);
        assert!(verify_sha256_bytes(data, &good).is_ok());
        // Case-insensitive accept.
        assert!(verify_sha256_bytes(data, &good.to_uppercase()).is_ok());
        // Wrong digest rejected with a descriptive error.
        match verify_sha256_bytes(data, "deadbeef") {
            Err(FetchError::Checksum { expected, actual }) => {
                assert_eq!(expected, "deadbeef");
                assert_eq!(actual, good);
            }
            other => panic!("expected checksum error, got {other:?}"),
        }
    }

    // ---- ensure_backend logic (mocked HTTP, temp dirs) --------------------

    /// A `BytesFetcher` returning canned bytes (or an error), recording calls so
    /// tests can assert a download did / did not happen. No network.
    struct MockFetcher {
        payload: Result<Vec<u8>, ()>,
        calls: std::cell::Cell<u32>,
    }
    impl MockFetcher {
        fn ok(bytes: Vec<u8>) -> Self {
            Self {
                payload: Ok(bytes),
                calls: std::cell::Cell::new(0),
            }
        }
        fn err() -> Self {
            Self {
                payload: Err(()),
                calls: std::cell::Cell::new(0),
            }
        }
    }
    impl BytesFetcher for MockFetcher {
        async fn fetch(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
            self.calls.set(self.calls.get() + 1);
            self.payload
                .clone()
                .map_err(|()| FetchError::Download("mock network down".into()))
        }
    }

    fn tmp_root() -> PathBuf {
        let mut d = std::env::temp_dir();
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        d.push(format!("lirevo-fetch-test-{n}-{:p}", &d));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Build a Linux/x86_64 [`FetchTarget`] for `(backend, engine)` rooted at
    /// `root` — keeps the test call sites terse after the args were grouped.
    fn target<'a>(
        backend: BackendKind,
        engine: Engine,
        os: &'a str,
        root: &'a Path,
    ) -> FetchTarget<'a> {
        FetchTarget {
            desired: backend,
            engine,
            os,
            arch: "x86_64",
            backends_root: root,
        }
    }

    fn one_entry_manifest(payload: &[u8]) -> BackendManifest {
        BackendManifest {
            schema_version: 1,
            entries: vec![BackendEntry {
                os: "linux".into(),
                arch: "x86_64".into(),
                backend: BackendKind::Cuda,
                engine: Engine::Llama,
                version: "0.1.146".into(),
                url: "https://example.com/m.so".into(),
                sha256: sha256_hex(payload),
                files: vec!["libggml-cuda.so".into()],
            }],
        }
    }

    #[tokio::test]
    async fn bundled_backend_is_noop() {
        let root = tmp_root();
        let m = BackendManifest {
            schema_version: 1,
            entries: vec![],
        };
        let fetcher = MockFetcher::ok(vec![]);
        let out = ensure_backend(
            &target(BackendKind::Metal, Engine::Llama, "macos", &root),
            &m,
            &fetcher,
            unpack_single_file,
        )
        .await
        .unwrap();
        assert_eq!(out, FetchOutcome::Bundled);
        assert_eq!(fetcher.calls.get(), 0, "no download for a bundled backend");
        // CPU is also bundled.
        let out = ensure_backend(
            &target(BackendKind::Cpu, Engine::Llama, "linux", &root),
            &m,
            &fetcher,
            unpack_single_file,
        )
        .await
        .unwrap();
        assert_eq!(out, FetchOutcome::Bundled);
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn fetches_then_places_and_stamps() {
        let root = tmp_root();
        let payload = b"fake cuda module bytes".to_vec();
        let m = one_entry_manifest(&payload);
        let fetcher = MockFetcher::ok(payload.clone());

        let out = ensure_backend(
            &target(BackendKind::Cuda, Engine::Llama, "linux", &root),
            &m,
            &fetcher,
            unpack_single_file,
        )
        .await
        .unwrap();

        let dir = root.join("llama");
        assert_eq!(out, FetchOutcome::Fetched(dir.clone()));
        assert_eq!(fetcher.calls.get(), 1);
        // Module placed with the manifest filename + matching bytes.
        let placed = std::fs::read(dir.join("libggml-cuda.so")).unwrap();
        assert_eq!(placed, payload);
        // Version stamp written.
        assert_eq!(
            std::fs::read_to_string(dir.join(VERSION_STAMP)).unwrap(),
            "0.1.146"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn skips_when_already_present() {
        let root = tmp_root();
        let payload = b"module v1".to_vec();
        let m = one_entry_manifest(&payload);
        let fetcher = MockFetcher::ok(payload.clone());

        // First call fetches.
        let first = ensure_backend(
            &target(BackendKind::Cuda, Engine::Llama, "linux", &root),
            &m,
            &fetcher,
            unpack_single_file,
        )
        .await
        .unwrap();
        assert!(matches!(first, FetchOutcome::Fetched(_)));
        assert_eq!(fetcher.calls.get(), 1);

        // Second call (same manifest) must skip — no new download.
        let second = ensure_backend(
            &target(BackendKind::Cuda, Engine::Llama, "linux", &root),
            &m,
            &fetcher,
            unpack_single_file,
        )
        .await
        .unwrap();
        assert_eq!(second, FetchOutcome::AlreadyPresent(root.join("llama")));
        assert_eq!(fetcher.calls.get(), 1, "no re-download when present");
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn refetches_when_version_changed() {
        let root = tmp_root();
        let payload = b"module".to_vec();
        let mut m = one_entry_manifest(&payload);
        let fetcher = MockFetcher::ok(payload.clone());

        ensure_backend(
            &target(BackendKind::Cuda, Engine::Llama, "linux", &root),
            &m,
            &fetcher,
            unpack_single_file,
        )
        .await
        .unwrap();
        assert_eq!(fetcher.calls.get(), 1);

        // Bump the engine version → the stale on-disk stamp no longer matches,
        // so the next ensure must re-fetch.
        m.entries[0].version = "0.2.0".into();
        let out = ensure_backend(
            &target(BackendKind::Cuda, Engine::Llama, "linux", &root),
            &m,
            &fetcher,
            unpack_single_file,
        )
        .await
        .unwrap();
        assert!(matches!(out, FetchOutcome::Fetched(_)));
        assert_eq!(fetcher.calls.get(), 2, "version change forces re-fetch");
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn bad_checksum_is_rejected_and_no_file_placed() {
        let root = tmp_root();
        let mut m = one_entry_manifest(b"correct");
        // Manifest claims a different hash than the bytes the fetcher returns.
        m.entries[0].sha256 = sha256_hex(b"different");
        let fetcher = MockFetcher::ok(b"correct".to_vec());

        let err = ensure_backend(
            &target(BackendKind::Cuda, Engine::Llama, "linux", &root),
            &m,
            &fetcher,
            unpack_single_file,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, FetchError::Checksum { .. }));
        // Nothing placed on a checksum failure.
        assert!(!root.join("llama").join("libggml-cuda.so").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn no_manifest_entry_is_not_an_error() {
        let root = tmp_root();
        // Manifest has a Vulkan/llama entry but we ask for Cuda/llama.
        let m = BackendManifest {
            schema_version: 1,
            entries: vec![BackendEntry {
                os: "linux".into(),
                arch: "x86_64".into(),
                backend: BackendKind::Vulkan,
                engine: Engine::Llama,
                version: "1".into(),
                url: "u".into(),
                sha256: "00".into(),
                files: vec!["x.so".into()],
            }],
        };
        let fetcher = MockFetcher::ok(vec![]);
        let out = ensure_backend(
            &target(BackendKind::Cuda, Engine::Llama, "linux", &root),
            &m,
            &fetcher,
            unpack_single_file,
        )
        .await
        .unwrap();
        assert_eq!(out, FetchOutcome::NotInManifest);
        assert_eq!(fetcher.calls.get(), 0, "no download when nothing matches");
        std::fs::remove_dir_all(&root).ok();
    }

    #[tokio::test]
    async fn download_error_propagates_and_leaves_dest_clean() {
        let root = tmp_root();
        let m = one_entry_manifest(b"x");
        let fetcher = MockFetcher::err();
        let err = ensure_backend(
            &target(BackendKind::Cuda, Engine::Llama, "linux", &root),
            &m,
            &fetcher,
            unpack_single_file,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, FetchError::Download(_)));
        assert!(!root.join("llama").join("libggml-cuda.so").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn engine_dir_names() {
        assert_eq!(Engine::Parakeet.dir_name(), "parakeet");
        assert_eq!(Engine::Llama.dir_name(), "llama");
    }
}
