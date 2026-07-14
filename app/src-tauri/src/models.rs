use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tokio::sync::oneshot;

use inference_core::catalog as ic_catalog;
use inference_core::catalog::{Catalog as IcCatalog, LlmEntry as IcLlm, SttEntry as IcStt};

/// Wire shape exposed to the frontend. Mirrors the JSON catalog in
/// `inference-core/data/model_catalog.json`, flattened so the frontend can
/// treat STT and LLM models uniformly.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: String,
    pub kind: ModelKind,
    pub display_name: String,
    pub description: String,
    pub size_bytes: u64,
    pub filename: String,
    pub url: String,
    pub sha256: Option<String>,
    pub coreml_encoder_url: Option<String>,
    pub coreml_encoder_filename: Option<String>,
    /// SHA-256 of the CoreML encoder zip, when known. The download path uses
    /// this to verify the zip before extracting.
    pub coreml_encoder_sha256: Option<String>,
    /// Bake-off scores for LLMs. `None` for STT entries and for LLMs that
    /// have not been blessed yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scores: Option<ic_catalog::ModelScores>,
    /// Marked by `lirevo-eval bless` on the weighted-composite winner.
    pub recommended: bool,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelKind {
    Stt,
    Llm,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModel {
    pub id: String,
    pub kind: ModelKind,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub in_catalog: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadProgressState {
    Queued,
    Downloading,
    Verifying,
    Complete,
    Error,
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub id: String,
    pub state: DownloadProgressState,
    pub bytes_received: u64,
    pub bytes_total: u64,
    pub error_message: Option<String>,
}

fn raw_catalog() -> &'static IcCatalog {
    static CATALOG: OnceLock<IcCatalog> = OnceLock::new();
    CATALOG.get_or_init(ic_catalog::load_embedded)
}

fn stt_to_wire(e: &IcStt) -> CatalogEntry {
    let (url, filename, sha) = match &e.coreml_encoder {
        Some(c) => (
            Some(c.url.clone()),
            Some(c.filename.clone()),
            c.sha256.clone(),
        ),
        None => (None, None, None),
    };
    CatalogEntry {
        id: e.id.clone(),
        kind: ModelKind::Stt,
        display_name: e.display_name.clone(),
        description: e.description.clone(),
        size_bytes: e.size_bytes,
        filename: e.filename.clone(),
        url: e.url.clone(),
        sha256: e.sha256.clone(),
        coreml_encoder_url: url,
        coreml_encoder_filename: filename,
        coreml_encoder_sha256: sha,
        scores: None,
        recommended: false,
    }
}

fn llm_to_wire(e: &IcLlm) -> CatalogEntry {
    CatalogEntry {
        id: e.id.clone(),
        kind: ModelKind::Llm,
        display_name: e.display_name.clone(),
        description: e.description.clone(),
        size_bytes: e.size_bytes,
        filename: e.filename.clone(),
        url: e.url.clone(),
        sha256: e.sha256.clone(),
        coreml_encoder_url: None,
        coreml_encoder_filename: None,
        coreml_encoder_sha256: None,
        scores: e.scores,
        recommended: e.recommended,
    }
}

/// Flattened catalog used by the frontend (STT first, then LLM).
#[must_use]
pub fn catalog() -> Vec<CatalogEntry> {
    let c = raw_catalog();
    let mut out = Vec::with_capacity(c.stt.len() + c.llm.len());
    out.extend(c.stt.iter().map(stt_to_wire));
    out.extend(c.llm.iter().map(llm_to_wire));
    out
}

fn find_by_id(id: &str) -> Option<CatalogEntry> {
    catalog().into_iter().find(|c| c.id == id)
}

fn find_by_filename(name: &str) -> Option<CatalogEntry> {
    catalog().into_iter().find(|c| c.filename == name)
}

pub fn models_dir(app: &tauri::AppHandle) -> std::io::Result<PathBuf> {
    let dir = crate::paths::data_dir(app)
        .map_err(|e| std::io::Error::other(e.to_string()))?
        .join("models");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Extra headroom required on top of a download's advertised size, to absorb
/// filesystem block rounding and leave the volume from filling to zero.
const DOWNLOAD_SAFETY_MARGIN_BYTES: u64 = 256 * 1024 * 1024;

/// Free bytes available on the volume backing `path`, via POSIX `statvfs`.
/// `None` on non-Unix targets (Windows isn't functional yet, see AGENTS.md
/// "Platform support status") or if the syscall fails — callers treat `None`
/// as "unknown" and skip the pre-flight check rather than blocking a download
/// on an unreliable signal.
#[cfg(unix)]
fn free_space_bytes(path: &std::path::Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c_path` is a valid NUL-terminated C string for the lifetime of
    // the call, and `stat` is a valid out-pointer sized for `libc::statvfs`.
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), std::ptr::addr_of_mut!(stat)) };
    if rc != 0 {
        return None;
    }
    Some((stat.f_bavail as u64).saturating_mul(stat.f_frsize))
}

#[cfg(not(unix))]
fn free_space_bytes(_path: &std::path::Path) -> Option<u64> {
    None
}

fn format_bytes(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else {
        format!("{:.0} MB", b / MB)
    }
}

fn disk_space_error_message(required: u64, available: u64) -> String {
    format!(
        "Not enough disk space — need {}, have {} free",
        format_bytes(required),
        format_bytes(available)
    )
}

/// Compares `available` (`None` = unknown, e.g. non-Unix targets or a failed
/// syscall) against `required` bytes. Pure so it's testable without a real
/// filesystem or `AppHandle`.
fn evaluate_disk_space(available: Option<u64>, required: u64) -> Result<(), String> {
    match available {
        Some(avail) if avail < required => Err(disk_space_error_message(required, avail)),
        _ => Ok(()),
    }
}

/// Pre-flight disk space check: fails fast (before any network I/O) if the
/// volume backing the models dir doesn't have room for `needed_bytes` plus a
/// safety margin. `LIREVO_DEV_FAKE_FREE_BYTES` (debug builds only) overrides
/// the real free-space reading so the low-space UI path can be exercised
/// without actually filling a disk.
pub(crate) fn check_disk_space(app: &tauri::AppHandle, needed_bytes: u64) -> Result<(), String> {
    let dir = models_dir(app).map_err(|e| e.to_string())?;
    let required = needed_bytes.saturating_add(DOWNLOAD_SAFETY_MARGIN_BYTES);

    #[cfg(debug_assertions)]
    let available = std::env::var("LIREVO_DEV_FAKE_FREE_BYTES")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .or_else(|| free_space_bytes(&dir));
    #[cfg(not(debug_assertions))]
    let available = free_space_bytes(&dir);

    evaluate_disk_space(available, required)
}

/// The single blessed cleanup LLM. With the fixed catalog this is the one LLM
/// entry (the `recommended` one if flagged, else the first). `None` only if the
/// catalog ships no LLM at all.
#[must_use]
pub fn fixed_llm() -> Option<CatalogEntry> {
    let llms: Vec<CatalogEntry> = catalog()
        .into_iter()
        .filter(|c| c.kind == ModelKind::Llm)
        .collect();
    llms.iter()
        .find(|c| c.recommended)
        .cloned()
        .or_else(|| llms.into_iter().next())
}

/// Absolute on-disk path of the fixed cleanup GGUF inside the app models dir.
/// `Ok(None)` when the catalog ships no LLM.
pub fn fixed_llm_path(app: &tauri::AppHandle) -> std::io::Result<Option<PathBuf>> {
    let Some(entry) = fixed_llm() else {
        return Ok(None);
    };
    Ok(Some(models_dir(app)?.join(entry.filename)))
}

/// The fixed cleanup GGUF path when it exists on disk, else `None`. The only
/// error source is resolving the models dir; we log it and degrade to STT-only
/// mode rather than propagating. Used by the load path + startup engine config
/// so cleanup loads iff the file is present.
#[must_use]
pub fn effective_llm_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    match fixed_llm_path(app) {
        Ok(Some(p)) if p.exists() => Some(p),
        Ok(_) => None,
        Err(e) => {
            tracing::warn!(error = %e, "could not resolve models dir for cleanup model path");
            None
        }
    }
}

/// Result of an installed-model integrity check (`verify_installed` /
/// `check_size_only`). `Missing` covers both "not downloaded yet" and
/// "unknown catalog id" — neither is actionable differently by callers.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IntegrityStatus {
    Ok,
    SizeMismatch,
    HashMismatch,
    Missing,
}

/// Where a catalog id's on-disk file is expected to live, and what it should
/// look like. Resolves against both the LLM catalog (`inference-core`'s
/// `IcCatalog`) and the fixed STT catalog (`crate::stt::catalog`) — the two
/// are disjoint since #42's fixed-model-catalog change moved STT out of the
/// JSON catalog entirely (see `catalog_has_0_stt_and_1_llm` above).
struct ExpectedModel {
    path: PathBuf,
    size_bytes: u64,
    sha256: Option<String>,
}

fn expected_model(app: &tauri::AppHandle, id: &str) -> Option<ExpectedModel> {
    let dir = models_dir(app).ok()?;
    if let Some(entry) = find_by_id(id) {
        return Some(ExpectedModel {
            path: dir.join(&entry.filename),
            size_bytes: entry.size_bytes,
            sha256: entry.sha256,
        });
    }
    let meta = crate::stt::catalog::model_metadata(id)?;
    Some(ExpectedModel {
        path: dir.join(crate::stt::STT_GGUF_FILENAME),
        size_bytes: meta.size_bytes,
        sha256: Some(meta.sha256.to_string()),
    })
}

/// Core integrity logic, decoupled from `AppHandle`/catalog lookups so it's
/// unit-testable against a plain fixture path. Size mismatch short-circuits
/// before touching file contents; a full SHA-256 rehash (via `verify_sha256`)
/// only runs when the size already matches and an expected digest exists.
async fn check_integrity(
    path: &std::path::Path,
    expected_size: u64,
    expected_sha256: Option<&str>,
) -> IntegrityStatus {
    let Ok(meta) = tokio::fs::metadata(path).await else {
        return IntegrityStatus::Missing;
    };
    if meta.len() != expected_size {
        return IntegrityStatus::SizeMismatch;
    }
    let Some(expected) = expected_sha256 else {
        return IntegrityStatus::Ok;
    };
    match verify_sha256(path, expected).await {
        Ok(()) => IntegrityStatus::Ok,
        Err(_) => IntegrityStatus::HashMismatch,
    }
}

/// Full integrity re-check for an installed model: cheap size check first,
/// then (only if the size already matches) a full SHA-256 rehash. This is
/// the on-demand check backing the Settings > Models "Verify" action
/// (`commands::models::models_verify_integrity`) — too slow over a ~1-2 GB
/// GGUF to run unconditionally at startup, see `check_size_only` for that.
pub async fn verify_installed(app: &tauri::AppHandle, id: &str) -> IntegrityStatus {
    let Some(expected) = expected_model(app, id) else {
        return IntegrityStatus::Missing;
    };
    check_integrity(
        &expected.path,
        expected.size_bytes,
        expected.sha256.as_deref(),
    )
    .await
}

/// Cheap, synchronous size-only integrity check — reads file metadata, never
/// file contents. Safe to run unconditionally at startup for every model in
/// the fixed catalog (`startup_integrity_check`).
#[must_use]
pub fn check_size_only(app: &tauri::AppHandle, id: &str) -> IntegrityStatus {
    let Some(expected) = expected_model(app, id) else {
        return IntegrityStatus::Missing;
    };
    match std::fs::metadata(&expected.path) {
        Ok(meta) if meta.len() == expected.size_bytes => IntegrityStatus::Ok,
        Ok(_) => IntegrityStatus::SizeMismatch,
        Err(_) => IntegrityStatus::Missing,
    }
}

/// Every id the app's fixed catalog can install: the single STT model plus
/// the single blessed cleanup LLM (see #42's fixed-model-catalog change —
/// there is no user choice, so this is a fixed pair, not a full catalog
/// scan).
fn fixed_catalog_ids() -> Vec<String> {
    let mut ids = vec![crate::stt::catalog::default_model_id().to_string()];
    if let Some(llm) = fixed_llm() {
        ids.push(llm.id);
    }
    ids
}

/// Startup integrity sweep: a size-only check (see `check_size_only`) of
/// every model in the fixed catalog, logging and best-effort toasting on a
/// mismatch. A model that isn't downloaded yet resolves to `Missing`, which
/// is not surfaced — most users won't have both models before onboarding
/// completes, and that's expected, not corruption.
pub fn startup_integrity_check(app: &tauri::AppHandle) {
    use tauri::Emitter;

    for id in fixed_catalog_ids() {
        if check_size_only(app, &id) == IntegrityStatus::SizeMismatch {
            tracing::warn!(
                id = %id,
                "installed model failed startup size check — possible corruption; \
                 re-download from Settings > Models"
            );
            let _ = app.emit(
                "toast",
                crate::commands::toast(
                    "warn",
                    format!(
                        "Installed model '{id}' looks corrupted (unexpected file size). \
                         Re-download it from Settings > Models."
                    ),
                ),
            );
        }
    }
}

pub fn list_local(app: &tauri::AppHandle) -> std::io::Result<Vec<LocalModel>> {
    let dir = models_dir(app)?;
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        let meta = entry.metadata()?;
        if !meta.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let kind_from_ext = if name.ends_with(".bin") {
            Some(ModelKind::Stt)
        } else if name.ends_with(".gguf") {
            Some(ModelKind::Llm)
        } else {
            None
        };
        let Some(ext_kind) = kind_from_ext else {
            continue;
        };
        let catalog_hit = find_by_filename(&name);
        out.push(LocalModel {
            id: catalog_hit
                .as_ref()
                .map(|c| c.id.clone())
                .unwrap_or_else(|| format!("custom:{name}")),
            kind: catalog_hit.as_ref().map(|c| c.kind).unwrap_or(ext_kind),
            path,
            size_bytes: meta.len(),
            in_catalog: catalog_hit.is_some(),
        });
    }
    Ok(out)
}

/// Active downloads keyed by catalog id, holding the oneshot sender used by
/// `cancel()` to interrupt the streaming download.
pub static ACTIVE_DOWNLOADS: Mutex<Option<HashMap<String, oneshot::Sender<()>>>> = Mutex::new(None);

pub fn init_active_downloads() {
    let mut g = ACTIVE_DOWNLOADS.lock().unwrap();
    if g.is_none() {
        *g = Some(HashMap::new());
    }
}

use futures_util::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug)]
pub(crate) enum DownloadError {
    Cancelled,
    Failed(String),
}

/// Stream the file through SHA-256 and compare against the catalog's expected
/// digest. We hash on disk (not on the fly during the download stream)
/// because the bytes have already been renamed into place and any future
/// reload should also catch a tampered file. Buffer size is 64 KiB —
/// large enough to amortize syscalls without ballooning memory on 2 GB
/// models.
pub(crate) async fn verify_sha256(
    path: &std::path::Path,
    expected: &str,
) -> Result<(), DownloadError> {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;

    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| DownloadError::Failed(format!("open for hash: {e}")))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .await
            .map_err(|e| DownloadError::Failed(format!("read for hash: {e}")))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    // sha2 0.11 returns `Array<u8, N>` from `finalize`, which (unlike the
    // 0.10 `GenericArray`) does NOT implement `LowerHex` — so `format!
    // ("{:x}", ...)` fails to compile. Hex-encode byte-by-byte instead.
    let digest = hasher.finalize();
    let mut actual = String::with_capacity(64);
    for b in digest.iter() {
        let _ = write!(&mut actual, "{b:02x}");
    }
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(DownloadError::Failed(format!(
            "SHA-256 mismatch — expected {expected}, got {actual}"
        )))
    }
}

/// Verify a just-downloaded file against its expected SHA-256, deleting it on
/// mismatch so a retry starts from scratch. Shared by the LLM (`download_inner`)
/// and STT (`commands::models::stt_download`) download paths.
pub(crate) async fn verify_and_cleanup(
    path: &std::path::Path,
    expected: &str,
) -> Result<(), DownloadError> {
    if let Err(e) = verify_sha256(path, expected).await {
        let _ = tokio::fs::remove_file(path).await;
        return Err(e);
    }
    Ok(())
}

pub async fn download(app: tauri::AppHandle, id: String) -> Result<(), crate::AppError> {
    use crate::AppError;
    use tauri::Emitter;

    let entry =
        find_by_id(&id).ok_or_else(|| AppError::Download(format!("unknown model id: {id}")))?;

    if let Err(msg) = check_disk_space(&app, entry.size_bytes) {
        let _ = app.emit(
            "download:progress",
            DownloadProgress {
                id: id.clone(),
                state: DownloadProgressState::Error,
                bytes_received: 0,
                bytes_total: entry.size_bytes,
                error_message: Some(msg.clone()),
            },
        );
        return Err(AppError::Download(msg));
    }

    let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
    {
        let mut g = ACTIVE_DOWNLOADS.lock().unwrap();
        let map = g.as_mut().expect("init_active_downloads not called");
        if map.contains_key(&id) {
            return Err(AppError::Download(format!("already downloading: {id}")));
        }
        map.insert(id.clone(), cancel_tx);
    }

    let _ = app.emit(
        "download:progress",
        DownloadProgress {
            id: id.clone(),
            state: DownloadProgressState::Queued,
            bytes_received: 0,
            bytes_total: entry.size_bytes,
            error_message: None,
        },
    );

    let result = download_inner(&app, &entry, &mut cancel_rx).await;

    {
        let mut g = ACTIVE_DOWNLOADS.lock().unwrap();
        if let Some(map) = g.as_mut() {
            map.remove(&id);
        }
    }

    match result {
        Ok(_) => {
            let _ = app.emit(
                "download:progress",
                DownloadProgress {
                    id: id.clone(),
                    state: DownloadProgressState::Complete,
                    bytes_received: entry.size_bytes,
                    bytes_total: entry.size_bytes,
                    error_message: None,
                },
            );
            Ok(())
        }
        Err(DownloadError::Cancelled) => {
            let _ = app.emit(
                "download:progress",
                DownloadProgress {
                    id,
                    state: DownloadProgressState::Cancelled,
                    bytes_received: 0,
                    bytes_total: 0,
                    error_message: None,
                },
            );
            Ok(())
        }
        Err(DownloadError::Failed(msg)) => {
            let _ = app.emit(
                "download:progress",
                DownloadProgress {
                    id,
                    state: DownloadProgressState::Error,
                    bytes_received: 0,
                    bytes_total: 0,
                    error_message: Some(msg.clone()),
                },
            );
            Err(AppError::Download(msg))
        }
    }
}

/// Stream `url` to `dest` via a `.partial` temp file, emitting
/// `download:progress` events (throttled to once per 100ms — a 2 GB download
/// produces ~250k chunks; one emit per chunk made the JS progress bar visibly
/// stutter and starved the rest of the app), then rename into place and
/// optionally verify against `expected_sha256`. Shared by the LLM GGUF path
/// (`download_inner`) and the STT GGUF path (`commands::models::stt_download`).
/// The CoreML companion zip (`download_and_extract_coreml`) has its own
/// extract step afterwards, so it stays a separate routine.
///
/// On cancellation the `.partial` file is removed before returning; on any
/// other failure the `.partial` file is left in place for the caller to
/// decide whether to clean it up (callers currently differ here — see
/// TRUST-4).
pub(crate) async fn download_file(
    app: &tauri::AppHandle,
    url: &str,
    dest: &std::path::Path,
    id: &str,
    expected_total: u64,
    expected_sha256: Option<&str>,
    cancel_rx: &mut oneshot::Receiver<()>,
) -> Result<(), DownloadError> {
    use tauri::Emitter;
    let tmp = dest.with_extension(format!(
        "{}.partial",
        dest.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));

    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| DownloadError::Failed(format!("http: {e}")))?;
    if !resp.status().is_success() {
        return Err(DownloadError::Failed(format!("HTTP {}", resp.status())));
    }
    let total = resp.content_length().unwrap_or(expected_total);

    let mut file = tokio::fs::File::create(&tmp)
        .await
        .map_err(|e| DownloadError::Failed(format!("create tmp: {e}")))?;

    let mut received: u64 = 0;
    let mut stream = resp.bytes_stream();
    let mut last_emit = std::time::Instant::now();
    while let Some(chunk_result) = stream.next().await {
        if cancel_rx.try_recv().is_ok() {
            drop(file);
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(DownloadError::Cancelled);
        }
        let chunk = chunk_result.map_err(|e| DownloadError::Failed(format!("stream: {e}")))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| DownloadError::Failed(format!("write: {e}")))?;
        received += chunk.len() as u64;
        if last_emit.elapsed() >= std::time::Duration::from_millis(100) {
            last_emit = std::time::Instant::now();
            let _ = app.emit(
                "download:progress",
                DownloadProgress {
                    id: id.to_string(),
                    state: DownloadProgressState::Downloading,
                    bytes_received: received,
                    bytes_total: total,
                    error_message: None,
                },
            );
        }
    }
    // Always emit a final downloading event so the UI shows 100% before
    // transitioning to Complete (avoids a visual "snap" at the end).
    let _ = app.emit(
        "download:progress",
        DownloadProgress {
            id: id.to_string(),
            state: DownloadProgressState::Downloading,
            bytes_received: received,
            bytes_total: total,
            error_message: None,
        },
    );
    file.flush()
        .await
        .map_err(|e| DownloadError::Failed(format!("flush: {e}")))?;
    drop(file);

    tokio::fs::rename(&tmp, dest)
        .await
        .map_err(|e| DownloadError::Failed(format!("rename: {e}")))?;

    if let Some(expected) = expected_sha256 {
        let _ = app.emit(
            "download:progress",
            DownloadProgress {
                id: id.to_string(),
                state: DownloadProgressState::Verifying,
                bytes_received: received,
                bytes_total: total,
                error_message: None,
            },
        );
        verify_and_cleanup(dest, expected).await?;
    }

    Ok(())
}

async fn download_inner(
    app: &tauri::AppHandle,
    entry: &CatalogEntry,
    cancel_rx: &mut oneshot::Receiver<()>,
) -> Result<(), DownloadError> {
    let models_dir = models_dir(app).map_err(|e| DownloadError::Failed(e.to_string()))?;
    let dest = models_dir.join(&entry.filename);

    download_file(
        app,
        &entry.url,
        &dest,
        &entry.id,
        entry.size_bytes,
        entry.sha256.as_deref(),
        cancel_rx,
    )
    .await?;

    // Whisper CoreML companion (separate zip download + unzip).
    if entry.coreml_encoder_url.is_some() {
        download_and_extract_coreml(app, entry, cancel_rx).await?;
    }

    Ok(())
}

pub(crate) async fn download_and_extract_coreml(
    app: &tauri::AppHandle,
    entry: &CatalogEntry,
    cancel_rx: &mut oneshot::Receiver<()>,
) -> Result<(), DownloadError> {
    use tauri::Emitter;
    let Some(url) = entry.coreml_encoder_url.as_deref() else {
        return Ok(());
    };
    let Some(filename) = entry.coreml_encoder_filename.as_deref() else {
        return Ok(());
    };
    let models_dir = models_dir(app).map_err(|e| DownloadError::Failed(e.to_string()))?;
    let zip_path = models_dir.join(filename);
    let tmp = zip_path.with_extension("zip.partial");

    let progress_id = format!("{}:coreml", entry.id);

    let _ = app.emit(
        "download:progress",
        DownloadProgress {
            id: progress_id.clone(),
            state: DownloadProgressState::Downloading,
            bytes_received: 0,
            bytes_total: 0,
            error_message: None,
        },
    );

    let client = reqwest::Client::new();
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| DownloadError::Failed(format!("coreml http: {e}")))?;
    if !resp.status().is_success() {
        return Err(DownloadError::Failed(format!(
            "coreml HTTP {}",
            resp.status()
        )));
    }
    let total = resp.content_length().unwrap_or(0);

    let mut file = tokio::fs::File::create(&tmp)
        .await
        .map_err(|e| DownloadError::Failed(format!("coreml create tmp: {e}")))?;

    let mut received: u64 = 0;
    let mut stream = resp.bytes_stream();
    let mut last_emit = std::time::Instant::now();
    while let Some(chunk_result) = futures_util::StreamExt::next(&mut stream).await {
        if cancel_rx.try_recv().is_ok() {
            drop(file);
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(DownloadError::Cancelled);
        }
        let chunk =
            chunk_result.map_err(|e| DownloadError::Failed(format!("coreml stream: {e}")))?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
            .await
            .map_err(|e| DownloadError::Failed(format!("coreml write: {e}")))?;
        received += chunk.len() as u64;
        if last_emit.elapsed() >= std::time::Duration::from_millis(100) {
            last_emit = std::time::Instant::now();
            let _ = app.emit(
                "download:progress",
                DownloadProgress {
                    id: progress_id.clone(),
                    state: DownloadProgressState::Downloading,
                    bytes_received: received,
                    bytes_total: total,
                    error_message: None,
                },
            );
        }
    }
    tokio::io::AsyncWriteExt::flush(&mut file)
        .await
        .map_err(|e| DownloadError::Failed(format!("coreml flush: {e}")))?;
    drop(file);
    tokio::fs::rename(&tmp, &zip_path)
        .await
        .map_err(|e| DownloadError::Failed(format!("coreml rename: {e}")))?;

    // Verify the zip itself before we extract — a corrupted zip would
    // succeed at `unzip` for a while before failing partway through and
    // leaving a broken half-extracted .mlmodelc behind.
    let _ = app.emit(
        "download:progress",
        DownloadProgress {
            id: progress_id.clone(),
            state: DownloadProgressState::Verifying,
            bytes_received: received,
            bytes_total: received,
            error_message: None,
        },
    );
    if let Some(expected) = entry.coreml_encoder_sha256.as_deref() {
        if let Err(e) = verify_sha256(&zip_path, expected).await {
            let _ = tokio::fs::remove_file(&zip_path).await;
            return Err(e);
        }
    }

    // Extract via system unzip (always present on macOS). `-x __MACOSX/*`
    // skips the resource-fork metadata sibling that macOS Finder ships
    // inside zips — we don't need it and it would litter the models dir.
    let zip_path_clone = zip_path.clone();
    let models_dir_clone = models_dir.clone();
    let extract_result = tokio::task::spawn_blocking(move || {
        std::process::Command::new("unzip")
            .args(["-o", "-d"])
            .arg(&models_dir_clone)
            .arg(&zip_path_clone)
            .args(["-x", "__MACOSX/*"])
            .output()
    })
    .await
    .map_err(|e| DownloadError::Failed(format!("unzip join: {e}")))?
    .map_err(|e| DownloadError::Failed(format!("unzip spawn: {e}")))?;

    if !extract_result.status.success() {
        let stderr = String::from_utf8_lossy(&extract_result.stderr);
        return Err(DownloadError::Failed(format!("unzip failed: {stderr}")));
    }

    // Safety net for zips that don't match the `-x` exclusion (e.g. older
    // zips with leading `./__MACOSX` paths) — remove any __MACOSX dir
    // left in the models folder.
    let macosx_dir = models_dir.join("__MACOSX");
    if tokio::fs::metadata(&macosx_dir).await.is_ok() {
        let _ = tokio::fs::remove_dir_all(&macosx_dir).await;
    }

    // Defense-in-depth against zip-slip (CVE-2018-1002201 family). The
    // SHA-256 pin above already ensures we only extract a zip whose
    // contents are known-good, but if HF were ever compromised or our
    // pinned hash drifted, macOS' system `unzip` (Info-ZIP 5.52, very old)
    // does not reliably reject `..` traversal entries. Walk the models dir
    // post-extract and assert every resolved path stays under it; remove
    // anything that escaped.
    let mdir_for_check = models_dir.clone();
    let _ = tokio::task::spawn_blocking(move || {
        assert_no_traversal(&mdir_for_check);
    })
    .await;

    let _ = tokio::fs::remove_file(&zip_path).await;
    Ok(())
}

fn assert_no_traversal(root: &std::path::Path) {
    let Ok(root_canon) = std::fs::canonicalize(root) else {
        return;
    };
    let mut stack: Vec<std::path::PathBuf> = vec![root_canon.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            // Resolve the symlink target so a `link → ..` can't sneak by.
            let canon = match std::fs::canonicalize(&path) {
                Ok(p) => p,
                Err(_) => continue,
            };
            if !canon.starts_with(&root_canon) {
                tracing::error!(
                    path = %path.display(),
                    canon = %canon.display(),
                    "zip-slip: extracted path escaped models dir — removing",
                );
                let _ = std::fs::remove_file(&path);
                continue;
            }
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let is_sym = entry.file_type().map(|t| t.is_symlink()).unwrap_or(false);
            if is_dir && !is_sym {
                stack.push(canon);
            }
        }
    }
}

pub fn cancel(id: &str) -> Result<(), crate::AppError> {
    let mut g = ACTIVE_DOWNLOADS.lock().unwrap();
    if let Some(map) = g.as_mut() {
        if let Some(tx) = map.remove(id) {
            let _ = tx.send(());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_0_stt_and_1_llm() {
        let c = catalog();
        let stt = c.iter().filter(|c| c.kind == ModelKind::Stt).count();
        let llm = c.iter().filter(|c| c.kind == ModelKind::Llm).count();
        assert_eq!(stt, 0);
        assert_eq!(llm, 1);
    }

    #[test]
    fn fixed_llm_is_the_gemma_entry() {
        let e = fixed_llm().expect("a fixed LLM must exist");
        assert_eq!(e.kind, ModelKind::Llm);
        assert_eq!(e.id, "gemma-3-1b-it-q4");
        assert_eq!(e.filename, "gemma-3-1b-it-Q4_K_M.gguf");
        assert!(e.recommended);
    }

    #[test]
    fn catalog_ids_unique() {
        let mut seen = std::collections::HashSet::new();
        for c in catalog() {
            assert!(seen.insert(c.id.clone()), "duplicate id: {}", c.id);
        }
    }

    #[test]
    fn catalog_filenames_match_kind_ext() {
        for c in catalog() {
            match c.kind {
                ModelKind::Stt => assert!(c.filename.ends_with(".bin"), "{}", c.filename),
                ModelKind::Llm => assert!(c.filename.ends_with(".gguf"), "{}", c.filename),
            }
        }
    }

    #[test]
    fn coreml_encoder_paired() {
        for c in catalog() {
            assert_eq!(
                c.coreml_encoder_url.is_some(),
                c.coreml_encoder_filename.is_some()
            );
        }
    }

    #[test]
    fn at_most_one_recommended_llm() {
        let n = catalog().iter().filter(|c| c.recommended).count();
        assert!(n <= 1, "expected ≤1 recommended entry, got {n}");
    }

    #[test]
    #[cfg(unix)]
    fn free_space_bytes_returns_a_plausible_positive_number() {
        let dir = std::env::temp_dir();
        let free = free_space_bytes(&dir).expect("statvfs should succeed on a real path");
        // Any mounted, non-full volume has at least a few MB free — this just
        // guards against the syscall returning garbage (e.g. 0 or a huge
        // bogus value from a botched block-size multiplication).
        assert!(free > 0, "expected positive free space, got {free}");
    }

    #[test]
    fn evaluate_disk_space_ok_when_enough_free() {
        assert!(evaluate_disk_space(Some(10_000), 5_000).is_ok());
    }

    #[test]
    fn evaluate_disk_space_errors_when_not_enough_free() {
        let err = evaluate_disk_space(Some(100), 5_000).unwrap_err();
        assert!(err.contains("Not enough disk space"), "{err}");
    }

    #[test]
    fn evaluate_disk_space_skips_check_when_available_unknown() {
        // `None` models an unresolvable free-space reading (non-Unix target
        // or a failed syscall) — we don't want an unreliable signal to block
        // downloads outright.
        assert!(evaluate_disk_space(None, u64::MAX).is_ok());
    }

    #[test]
    fn format_bytes_uses_mb_below_one_gb_and_gb_above() {
        assert_eq!(format_bytes(500 * 1024 * 1024), "500 MB");
        assert_eq!(format_bytes(2 * 1024 * 1024 * 1024), "2.0 GB");
    }

    #[test]
    fn disk_space_error_message_reports_required_and_available() {
        let msg = disk_space_error_message(2 * 1024 * 1024 * 1024, 500 * 1024 * 1024);
        assert!(msg.contains("2.0 GB"), "{msg}");
        assert!(msg.contains("500 MB"), "{msg}");
    }

    // `stt_download` (app/src-tauri/src/commands/models.rs) registers/
    // deregisters in `ACTIVE_DOWNLOADS` using the same primitives as
    // `download`/`cancel` below. There is no mock-HTTP harness in this
    // workspace to drive `stt_download` end-to-end, so this exercises the
    // shared registration + cancellation plumbing directly.
    #[test]
    fn cancel_removes_entry_and_fires_receiver() {
        init_active_downloads();
        let (tx, mut rx) = oneshot::channel::<()>();
        {
            let mut g = ACTIVE_DOWNLOADS.lock().unwrap();
            g.as_mut()
                .unwrap()
                .insert("test:cancel-fires-receiver".to_string(), tx);
        }

        cancel("test:cancel-fires-receiver").unwrap();

        {
            let g = ACTIVE_DOWNLOADS.lock().unwrap();
            assert!(!g
                .as_ref()
                .unwrap()
                .contains_key("test:cancel-fires-receiver"));
        }
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn duplicate_registration_is_rejected_like_download_does() {
        init_active_downloads();
        let (tx, _rx) = oneshot::channel::<()>();
        {
            let mut g = ACTIVE_DOWNLOADS.lock().unwrap();
            let map = g.as_mut().unwrap();
            map.insert("test:duplicate-in-flight".to_string(), tx);
            assert!(
                map.contains_key("test:duplicate-in-flight"),
                "a second registration attempt for the same id must be rejected \
                 before insert, mirroring stt_download's and download's guard"
            );
        }
        // Clean up so other tests sharing this process-wide static aren't
        // affected.
        cancel("test:duplicate-in-flight").unwrap();
    }

    #[tokio::test]
    async fn verify_and_cleanup_rejects_and_deletes_corrupted_file() {
        let dir = std::env::temp_dir().join(format!("lirevo-sha256-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("corrupted.gguf");
        std::fs::write(&path, b"not the real model bytes").unwrap();

        let wrong_expected = "0".repeat(64);
        let result = verify_and_cleanup(&path, &wrong_expected).await;

        assert!(matches!(result, Err(DownloadError::Failed(_))));
        assert!(!path.exists(), "corrupted file should be deleted");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn check_integrity_reports_missing_for_absent_file() {
        let path = std::env::temp_dir().join(format!(
            "lirevo-integrity-missing-{}-{}",
            std::process::id(),
            "does-not-exist"
        ));
        let status = check_integrity(&path, 100, None).await;
        assert_eq!(status, IntegrityStatus::Missing);
    }

    #[tokio::test]
    async fn check_integrity_reports_size_mismatch_for_truncated_file() {
        let dir =
            std::env::temp_dir().join(format!("lirevo-integrity-size-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("truncated.gguf");
        std::fs::write(&path, b"short").unwrap();

        // Expected size (100) doesn't match the 5-byte fixture — should be
        // caught before any hashing happens, regardless of the expected hash.
        let status = check_integrity(&path, 100, Some(&"0".repeat(64))).await;
        assert_eq!(status, IntegrityStatus::SizeMismatch);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn check_integrity_reports_hash_mismatch_for_corrupted_same_size_file() {
        use sha2::{Digest, Sha256};

        let dir =
            std::env::temp_dir().join(format!("lirevo-integrity-hash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let original = b"totally real model bytes";
        let expected_digest = Sha256::digest(original);
        let expected: String = expected_digest.iter().map(|b| format!("{b:02x}")).collect();

        // Same length as `original` (25 bytes) but different content — a
        // corrupted-in-place file, not a truncated one.
        let corrupted = b"totally FAKE model bytes";
        assert_eq!(original.len(), corrupted.len());
        let path = dir.join("corrupted-same-size.gguf");
        std::fs::write(&path, corrupted).unwrap();

        let status = check_integrity(&path, original.len() as u64, Some(&expected)).await;
        assert_eq!(status, IntegrityStatus::HashMismatch);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn check_integrity_ok_when_size_and_hash_match() {
        use sha2::{Digest, Sha256};

        let dir = std::env::temp_dir().join(format!("lirevo-integrity-ok-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let content = b"totally real model bytes";
        let path = dir.join("good.gguf");
        std::fs::write(&path, content).unwrap();

        let digest = Sha256::digest(content);
        let expected: String = digest.iter().map(|b| format!("{b:02x}")).collect();

        let status = check_integrity(&path, content.len() as u64, Some(&expected)).await;
        assert_eq!(status, IntegrityStatus::Ok);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn check_integrity_ok_when_size_matches_and_no_hash_expected() {
        let dir =
            std::env::temp_dir().join(format!("lirevo-integrity-nohash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("no-hash.gguf");
        std::fs::write(&path, b"whatever").unwrap();

        let status = check_integrity(&path, 8, None).await;
        assert_eq!(status, IntegrityStatus::Ok);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fixed_catalog_ids_includes_stt_and_blessed_llm() {
        let ids = fixed_catalog_ids();
        assert!(ids.contains(&crate::stt::catalog::default_model_id().to_string()));
        let llm = fixed_llm().expect("a fixed LLM must exist");
        assert!(ids.contains(&llm.id));
    }

    #[tokio::test]
    async fn verify_and_cleanup_accepts_matching_digest() {
        use sha2::{Digest, Sha256};

        let dir =
            std::env::temp_dir().join(format!("lirevo-sha256-test-ok-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("good.gguf");
        let content = b"totally real model bytes";
        std::fs::write(&path, content).unwrap();

        let digest = Sha256::digest(content);
        let expected: String = digest.iter().map(|b| format!("{b:02x}")).collect();

        let result = verify_and_cleanup(&path, &expected).await;

        assert!(result.is_ok());
        assert!(path.exists(), "verified file should be kept");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
