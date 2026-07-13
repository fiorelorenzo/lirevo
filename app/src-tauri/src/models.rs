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

async fn download_inner(
    app: &tauri::AppHandle,
    entry: &CatalogEntry,
    cancel_rx: &mut oneshot::Receiver<()>,
) -> Result<(), DownloadError> {
    use tauri::Emitter;
    let models_dir = models_dir(app).map_err(|e| DownloadError::Failed(e.to_string()))?;
    let dest = models_dir.join(&entry.filename);
    let tmp = dest.with_extension(format!(
        "{}.partial",
        dest.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));

    let client = reqwest::Client::new();
    let resp = client
        .get(&entry.url)
        .send()
        .await
        .map_err(|e| DownloadError::Failed(format!("http: {e}")))?;
    if !resp.status().is_success() {
        return Err(DownloadError::Failed(format!("HTTP {}", resp.status())));
    }
    let total = resp.content_length().unwrap_or(entry.size_bytes);

    let mut file = tokio::fs::File::create(&tmp)
        .await
        .map_err(|e| DownloadError::Failed(format!("create tmp: {e}")))?;

    let mut received: u64 = 0;
    let mut stream = resp.bytes_stream();
    // Emit at most every 100ms so we don't flood the IPC channel (a 2 GB
    // download produces ~250k chunks; one emit per chunk made the JS
    // progress bar visibly stutter and starved the rest of the app).
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
                    id: entry.id.clone(),
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
            id: entry.id.clone(),
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

    tokio::fs::rename(&tmp, &dest)
        .await
        .map_err(|e| DownloadError::Failed(format!("rename: {e}")))?;

    if let Some(expected) = &entry.sha256 {
        let _ = app.emit(
            "download:progress",
            DownloadProgress {
                id: entry.id.clone(),
                state: DownloadProgressState::Verifying,
                bytes_received: received,
                bytes_total: total,
                error_message: None,
            },
        );
        verify_and_cleanup(&dest, expected).await?;
    }

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
