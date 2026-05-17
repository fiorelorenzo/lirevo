use std::path::PathBuf;
use std::collections::HashMap;
use std::sync::Mutex;
use serde::Serialize;
use tokio::sync::oneshot;

#[derive(Clone, Copy, Debug, Serialize)]
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
pub struct DownloadProgress {
    pub id: String,
    pub state: DownloadProgressState,
    pub bytes_received: u64,
    pub bytes_total: u64,
    pub error_message: Option<String>,
}

pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "ggml-large-v3-turbo",
        kind: ModelKind::Stt,
        display_name: "Whisper large-v3-turbo",
        description: "Best balance · CoreML supported",
        size_bytes: 1_624_000_000,
        filename: "ggml-large-v3-turbo.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
        sha256: None,
        coreml_encoder_url: Some("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-encoder.mlmodelc.zip"),
        coreml_encoder_filename: Some("ggml-large-v3-turbo-encoder.mlmodelc.zip"),
    },
    CatalogEntry {
        id: "ggml-distil-large-v3",
        kind: ModelKind::Stt,
        display_name: "Whisper distil-large-v3",
        description: "Smaller (~750 MB), similar quality, multilingual",
        size_bytes: 756_000_000,
        filename: "ggml-distil-large-v3.bin",
        url: "https://huggingface.co/distil-whisper/distil-large-v3-ggml/resolve/main/ggml-distil-large-v3.bin",
        sha256: None,
        coreml_encoder_url: None,
        coreml_encoder_filename: None,
    },
    CatalogEntry {
        id: "ggml-small-en",
        kind: ModelKind::Stt,
        display_name: "Whisper small.en",
        description: "English only, very fast (~500 MB)",
        size_bytes: 488_000_000,
        filename: "ggml-small.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin",
        sha256: None,
        coreml_encoder_url: None,
        coreml_encoder_filename: None,
    },
    CatalogEntry {
        id: "qwen3-4b-instruct-2507-q4",
        kind: ModelKind::Llm,
        display_name: "Qwen3 4B Instruct 2507 (Q4_K_M)",
        description: "Recommended default. Multilingual, non-thinking.",
        size_bytes: 2_500_000_000,
        filename: "Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
        url: "https://huggingface.co/lmstudio-community/Qwen3-4B-Instruct-2507-GGUF/resolve/main/Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
        sha256: None,
        coreml_encoder_url: None,
        coreml_encoder_filename: None,
    },
    CatalogEntry {
        id: "llama-3.2-3b-instruct-q4",
        kind: ModelKind::Llm,
        display_name: "Llama 3.2 3B Instruct (Q4_K_M)",
        description: "Meta alternative, ~2 GB.",
        size_bytes: 2_020_000_000,
        filename: "Llama-3.2-3B-Instruct-Q4_K_M.gguf",
        url: "https://huggingface.co/lmstudio-community/Llama-3.2-3B-Instruct-GGUF/resolve/main/Llama-3.2-3B-Instruct-Q4_K_M.gguf",
        sha256: None,
        coreml_encoder_url: None,
        coreml_encoder_filename: None,
    },
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
use tokio::io::AsyncWriteExt;

#[derive(Debug)]
pub(crate) enum DownloadError {
    Cancelled,
    Failed(String),
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
        let _ = app.emit("download:progress", DownloadProgress {
            id: entry.id.to_string(),
            state: DownloadProgressState::Downloading,
            bytes_received: received,
            bytes_total: total,
            error_message: None,
        });
    }
    file.flush().await.map_err(|e| DownloadError::Failed(format!("flush: {e}")))?;
    drop(file);

    tokio::fs::rename(&tmp, &dest).await
        .map_err(|e| DownloadError::Failed(format!("rename: {e}")))?;

    // T17 fills CoreML encoder extraction.
    if entry.coreml_encoder_url.is_some() {
        download_and_extract_coreml(app, entry, cancel_rx).await?;
    }

    Ok(())
}

pub(crate) async fn download_and_extract_coreml(
    _app: &tauri::AppHandle,
    _entry: &CatalogEntry,
    _cancel_rx: &mut oneshot::Receiver<()>,
) -> Result<(), DownloadError> {
    // T17 fills.
    Ok(())
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
