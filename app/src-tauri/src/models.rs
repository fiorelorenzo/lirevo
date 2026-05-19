use std::path::PathBuf;
use std::collections::HashMap;
use std::sync::Mutex;
use serde::Serialize;
use tokio::sync::oneshot;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: &'static str,
    pub kind: ModelKind,
    pub display_name: &'static str,
    pub description: &'static str,
    pub size_bytes: u64,
    pub filename: &'static str,
    pub url: &'static str,
    pub sha256: Option<&'static str>,
    pub coreml_encoder_url: Option<&'static str>,
    pub coreml_encoder_filename: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelKind { Stt, Llm }

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

// SHA256 hashes are the git-LFS object IDs from the Hugging Face repo
// metadata at the time the catalog was last refreshed. If HF re-uploads a
// file, these need to be updated — `download_inner` rejects mismatches.
pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "ggml-large-v3-turbo",
        kind: ModelKind::Stt,
        display_name: "Whisper large-v3-turbo",
        description: "Best balance · CoreML supported",
        size_bytes: 1_624_555_275,
        filename: "ggml-large-v3-turbo.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
        sha256: Some("1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69"),
        coreml_encoder_url: Some("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-encoder.mlmodelc.zip"),
        coreml_encoder_filename: Some("ggml-large-v3-turbo-encoder.mlmodelc.zip"),
    },
    CatalogEntry {
        id: "ggml-distil-large-v3",
        kind: ModelKind::Stt,
        display_name: "Whisper distil-large-v3",
        description: "Faster, similar quality (~1.5 GB, multilingual)",
        size_bytes: 1_519_521_155,
        filename: "ggml-distil-large-v3.bin",
        url: "https://huggingface.co/distil-whisper/distil-large-v3-ggml/resolve/main/ggml-distil-large-v3.bin",
        sha256: Some("2883a11b90fb10ed592d826edeaee7d2929bf1ab985109fe9e1e7b4d2b69a298"),
        coreml_encoder_url: None,
        coreml_encoder_filename: None,
    },
    CatalogEntry {
        id: "ggml-small-en",
        kind: ModelKind::Stt,
        display_name: "Whisper small.en",
        description: "English only, very fast (~490 MB)",
        size_bytes: 487_614_201,
        filename: "ggml-small.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin",
        sha256: Some("c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d"),
        coreml_encoder_url: None,
        coreml_encoder_filename: None,
    },
    CatalogEntry {
        id: "qwen3-4b-instruct-2507-q4",
        kind: ModelKind::Llm,
        display_name: "Qwen3 4B Instruct 2507 (Q4_K_M)",
        description: "Recommended default. Multilingual, non-thinking.",
        size_bytes: 2_497_280_448,
        filename: "Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
        url: "https://huggingface.co/lmstudio-community/Qwen3-4B-Instruct-2507-GGUF/resolve/main/Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
        sha256: Some("8cdb57cbb880d313736a9bc4e3d3d2485f145b5e19cf33783746e753e82641fc"),
        coreml_encoder_url: None,
        coreml_encoder_filename: None,
    },
    CatalogEntry {
        id: "llama-3.2-3b-instruct-q4",
        kind: ModelKind::Llm,
        display_name: "Llama 3.2 3B Instruct (Q4_K_M)",
        description: "Meta alternative, ~2 GB.",
        size_bytes: 2_019_377_440,
        filename: "Llama-3.2-3B-Instruct-Q4_K_M.gguf",
        url: "https://huggingface.co/lmstudio-community/Llama-3.2-3B-Instruct-GGUF/resolve/main/Llama-3.2-3B-Instruct-Q4_K_M.gguf",
        sha256: Some("e4f1a04d927b09ec18eb2f233d85ecd760fc2d35cec97e37f8604d3632210d9a"),
        coreml_encoder_url: None,
        coreml_encoder_filename: None,
    },
];

/// CoreML encoder zip SHA256s, keyed by `coreml_encoder_filename`. Kept here
/// instead of on CatalogEntry to avoid bloating the struct with an Optional
/// second hash that only Whisper-CoreML entries use.
pub const COREML_ZIP_SHA256: &[(&str, &str)] = &[
    ("ggml-large-v3-turbo-encoder.mlmodelc.zip",
     "84bedfe895bd7b5de6e8e89a0803dfc5addf8c0c5bc4c937451716bf7cf7988a"),
];

pub fn models_dir(app: &tauri::AppHandle) -> std::io::Result<PathBuf> {
    use tauri::Manager;
    let dir = app.path().app_data_dir()
        .map_err(|e| std::io::Error::other(e.to_string()))?
        .join("models");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn list_local(app: &tauri::AppHandle) -> std::io::Result<Vec<LocalModel>> {
    let dir = models_dir(app)?;
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        let meta = entry.metadata()?;
        if !meta.is_file() { continue; }
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
        let Some(ext_kind) = kind_from_ext else { continue; };
        let catalog = CATALOG.iter().find(|c| c.filename == name);
        out.push(LocalModel {
            id: catalog.map(|c| c.id.to_string()).unwrap_or_else(|| format!("custom:{name}")),
            kind: catalog.map(|c| c.kind).unwrap_or(ext_kind),
            path,
            size_bytes: meta.len(),
            in_catalog: catalog.is_some(),
        });
    }
    Ok(out)
}

/// Active downloads: id → cancel sender. T16 fills usage.
pub static ACTIVE_DOWNLOADS: Mutex<Option<HashMap<String, oneshot::Sender<()>>>> = Mutex::new(None);

pub fn init_active_downloads() {
    let mut g = ACTIVE_DOWNLOADS.lock().unwrap();
    if g.is_none() { *g = Some(HashMap::new()); }
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
async fn verify_sha256(
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
        let _ = write!(&mut actual, "{:02x}", b);
    }
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(DownloadError::Failed(format!(
            "SHA-256 mismatch — expected {expected}, got {actual}"
        )))
    }
}

pub async fn download(
    app: tauri::AppHandle,
    id: String,
) -> Result<(), crate::AppError> {
    use tauri::Emitter;
    use crate::AppError;

    let entry = CATALOG.iter().find(|c| c.id == id)
        .ok_or_else(|| AppError::Download(format!("unknown model id: {id}")))?;

    let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
    {
        let mut g = ACTIVE_DOWNLOADS.lock().unwrap();
        let map = g.as_mut().expect("init_active_downloads not called");
        if map.contains_key(&id) {
            return Err(AppError::Download(format!("already downloading: {id}")));
        }
        map.insert(id.clone(), cancel_tx);
    }

    let _ = app.emit("download:progress", DownloadProgress {
        id: id.clone(),
        state: DownloadProgressState::Queued,
        bytes_received: 0,
        bytes_total: entry.size_bytes,
        error_message: None,
    });

    let result = download_inner(&app, entry, &mut cancel_rx).await;

    {
        let mut g = ACTIVE_DOWNLOADS.lock().unwrap();
        if let Some(map) = g.as_mut() { map.remove(&id); }
    }

    match result {
        Ok(_) => {
            let _ = app.emit("download:progress", DownloadProgress {
                id: id.clone(),
                state: DownloadProgressState::Complete,
                bytes_received: entry.size_bytes,
                bytes_total: entry.size_bytes,
                error_message: None,
            });
            Ok(())
        }
        Err(DownloadError::Cancelled) => {
            let _ = app.emit("download:progress", DownloadProgress {
                id,
                state: DownloadProgressState::Cancelled,
                bytes_received: 0,
                bytes_total: 0,
                error_message: None,
            });
            Ok(())
        }
        Err(DownloadError::Failed(msg)) => {
            let _ = app.emit("download:progress", DownloadProgress {
                id,
                state: DownloadProgressState::Error,
                bytes_received: 0,
                bytes_total: 0,
                error_message: Some(msg.clone()),
            });
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
    let dest = models_dir.join(entry.filename);
    let tmp = dest.with_extension(format!(
        "{}.partial",
        dest.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));

    let client = reqwest::Client::new();
    let resp = client.get(entry.url).send().await
        .map_err(|e| DownloadError::Failed(format!("http: {e}")))?;
    if !resp.status().is_success() {
        return Err(DownloadError::Failed(format!("HTTP {}", resp.status())));
    }
    let total = resp.content_length().unwrap_or(entry.size_bytes);

    let mut file = tokio::fs::File::create(&tmp).await
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
        file.write_all(&chunk).await
            .map_err(|e| DownloadError::Failed(format!("write: {e}")))?;
        received += chunk.len() as u64;
        if last_emit.elapsed() >= std::time::Duration::from_millis(100) {
            last_emit = std::time::Instant::now();
            let _ = app.emit("download:progress", DownloadProgress {
                id: entry.id.to_string(),
                state: DownloadProgressState::Downloading,
                bytes_received: received,
                bytes_total: total,
                error_message: None,
            });
        }
    }
    // Always emit a final downloading event so the UI shows 100% before
    // transitioning to Complete (avoids a visual "snap" at the end).
    let _ = app.emit("download:progress", DownloadProgress {
        id: entry.id.to_string(),
        state: DownloadProgressState::Downloading,
        bytes_received: received,
        bytes_total: total,
        error_message: None,
    });
    file.flush().await.map_err(|e| DownloadError::Failed(format!("flush: {e}")))?;
    drop(file);

    tokio::fs::rename(&tmp, &dest).await
        .map_err(|e| DownloadError::Failed(format!("rename: {e}")))?;

    if let Some(expected) = entry.sha256 {
        let _ = app.emit("download:progress", DownloadProgress {
            id: entry.id.to_string(),
            state: DownloadProgressState::Verifying,
            bytes_received: received,
            bytes_total: total,
            error_message: None,
        });
        if let Err(e) = verify_sha256(&dest, expected).await {
            // Remove the corrupted file so a retry starts from scratch.
            let _ = tokio::fs::remove_file(&dest).await;
            return Err(e);
        }
    }

    // T17 fills CoreML encoder extraction.
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
    let Some(url) = entry.coreml_encoder_url else { return Ok(()); };
    let Some(filename) = entry.coreml_encoder_filename else { return Ok(()); };
    let models_dir = models_dir(app).map_err(|e| DownloadError::Failed(e.to_string()))?;
    let zip_path = models_dir.join(filename);
    let tmp = zip_path.with_extension("zip.partial");

    let progress_id = format!("{}:coreml", entry.id);

    let _ = app.emit("download:progress", DownloadProgress {
        id: progress_id.clone(),
        state: DownloadProgressState::Downloading,
        bytes_received: 0,
        bytes_total: 0,
        error_message: None,
    });

    let client = reqwest::Client::new();
    let resp = client.get(url).send().await
        .map_err(|e| DownloadError::Failed(format!("coreml http: {e}")))?;
    if !resp.status().is_success() {
        return Err(DownloadError::Failed(format!("coreml HTTP {}", resp.status())));
    }
    let total = resp.content_length().unwrap_or(0);

    let mut file = tokio::fs::File::create(&tmp).await
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
        let chunk = chunk_result.map_err(|e| DownloadError::Failed(format!("coreml stream: {e}")))?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await
            .map_err(|e| DownloadError::Failed(format!("coreml write: {e}")))?;
        received += chunk.len() as u64;
        if last_emit.elapsed() >= std::time::Duration::from_millis(100) {
            last_emit = std::time::Instant::now();
            let _ = app.emit("download:progress", DownloadProgress {
                id: progress_id.clone(),
                state: DownloadProgressState::Downloading,
                bytes_received: received,
                bytes_total: total,
                error_message: None,
            });
        }
    }
    tokio::io::AsyncWriteExt::flush(&mut file).await
        .map_err(|e| DownloadError::Failed(format!("coreml flush: {e}")))?;
    drop(file);
    tokio::fs::rename(&tmp, &zip_path).await
        .map_err(|e| DownloadError::Failed(format!("coreml rename: {e}")))?;

    // Verify the zip itself before we extract — a corrupted zip would
    // succeed at `unzip` for a while before failing partway through and
    // leaving a broken half-extracted .mlmodelc behind.
    let _ = app.emit("download:progress", DownloadProgress {
        id: progress_id.clone(),
        state: DownloadProgressState::Verifying,
        bytes_received: received,
        bytes_total: received,
        error_message: None,
    });
    if let Some(expected) = COREML_ZIP_SHA256
        .iter()
        .find(|(name, _)| *name == filename)
        .map(|(_, hash)| *hash)
    {
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
    fn catalog_has_3_stt_and_2_llm() {
        let stt = CATALOG.iter().filter(|c| c.kind == ModelKind::Stt).count();
        let llm = CATALOG.iter().filter(|c| c.kind == ModelKind::Llm).count();
        assert_eq!(stt, 3);
        assert_eq!(llm, 2);
    }

    #[test]
    fn catalog_ids_unique() {
        let mut seen = std::collections::HashSet::new();
        for c in CATALOG {
            assert!(seen.insert(c.id), "duplicate id: {}", c.id);
        }
    }

    #[test]
    fn catalog_filenames_match_kind_ext() {
        for c in CATALOG {
            match c.kind {
                ModelKind::Stt => assert!(c.filename.ends_with(".bin"), "{}", c.filename),
                ModelKind::Llm => assert!(c.filename.ends_with(".gguf"), "{}", c.filename),
            }
        }
    }

    #[test]
    fn coreml_encoder_paired() {
        for c in CATALOG {
            assert_eq!(c.coreml_encoder_url.is_some(), c.coreml_encoder_filename.is_some());
        }
    }
}
