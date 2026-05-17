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
