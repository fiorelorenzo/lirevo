//! Multilingual sentence embeddings via ONNX Runtime (`ort` crate).
//!
//! Default model: `sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2`
//! exported to ONNX. ~118 MB. URL + SHA pinned in profiles/v1.toml.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use ndarray::Array2;
use ort::session::Session;
use ort::value::TensorRef;
use sha2::{Digest, Sha256};
use tokenizers::Tokenizer;

#[derive(Debug, Clone)]
pub struct EmbedderConfig {
    pub cache_dir: PathBuf,
    pub model_url: String,
    pub tokenizer_url: String,
    pub model_sha256: String,
    pub tokenizer_sha256: String,
}

pub struct Embedder {
    session: Session,
    tokenizer: Tokenizer,
    needs_token_type_ids: bool,
    output_name: String,
}

impl Embedder {
    pub fn load(cfg: &EmbedderConfig) -> Result<Self> {
        std::fs::create_dir_all(&cfg.cache_dir).context("create cache dir")?;
        let model_path = ensure_file(
            &cfg.cache_dir,
            "model.onnx",
            &cfg.model_url,
            &cfg.model_sha256,
        )?;
        let tok_path = ensure_file(
            &cfg.cache_dir,
            "tokenizer.json",
            &cfg.tokenizer_url,
            &cfg.tokenizer_sha256,
        )?;

        let session = build_session(&model_path)?;
        let needs_token_type_ids = session
            .inputs()
            .iter()
            .any(|i| i.name() == "token_type_ids");
        let output_name = pick_output_name(&session)?;
        let tokenizer =
            Tokenizer::from_file(&tok_path).map_err(|e| anyhow::anyhow!("tokenizer load: {e}"))?;
        Ok(Self {
            session,
            tokenizer,
            needs_token_type_ids,
            output_name,
        })
    }

    pub fn embed(&mut self, text: &str) -> Result<Vec<f32>> {
        let enc = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!("tokenize: {e}"))?;
        let ids: Vec<i64> = enc.get_ids().iter().map(|&i| i64::from(i)).collect();
        let mask: Vec<i64> = enc
            .get_attention_mask()
            .iter()
            .map(|&i| i64::from(i))
            .collect();
        let type_ids: Vec<i64> = enc.get_type_ids().iter().map(|&i| i64::from(i)).collect();
        let n = ids.len();
        if n == 0 {
            bail!("tokenizer produced zero tokens");
        }

        let ids_t = Array2::from_shape_vec((1, n), ids)?;
        let mask_t = Array2::from_shape_vec((1, n), mask.clone())?;
        let type_ids_t = Array2::from_shape_vec((1, n), type_ids)?;

        let outputs = if self.needs_token_type_ids {
            self.session.run(ort::inputs![
                "input_ids" => TensorRef::from_array_view(&ids_t)?,
                "attention_mask" => TensorRef::from_array_view(&mask_t)?,
                "token_type_ids" => TensorRef::from_array_view(&type_ids_t)?,
            ])?
        } else {
            self.session.run(ort::inputs![
                "input_ids" => TensorRef::from_array_view(&ids_t)?,
                "attention_mask" => TensorRef::from_array_view(&mask_t)?,
            ])?
        };

        let (shape, data) = outputs[self.output_name.as_str()].try_extract_tensor::<f32>()?;
        let dims: &[i64] = shape;
        if dims.len() != 3 || dims[0] != 1 || usize::try_from(dims[1]).ok() != Some(n) {
            bail!("unexpected output shape: {dims:?} (expected [1, {n}, hidden])");
        }
        let hidden = usize::try_from(dims[2]).context("hidden dim usize")?;

        // Mean-pool with the attention mask.
        let mut sum = vec![0.0_f32; hidden];
        let mut denom = 0.0_f32;
        for (i, m) in mask.iter().enumerate() {
            if *m == 0 {
                continue;
            }
            denom += 1.0;
            let row_start = i * hidden;
            for h in 0..hidden {
                sum[h] += data[row_start + h];
            }
        }
        if denom == 0.0 {
            bail!("all-zero attention mask");
        }
        for v in &mut sum {
            *v /= denom;
        }
        Ok(sum)
    }
}

#[must_use]
pub fn cosine(a: &[f32], b: &[f32]) -> f64 {
    let mut dot = 0.0_f64;
    let mut na = 0.0_f64;
    let mut nb = 0.0_f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let (x, y) = (f64::from(*x), f64::from(*y));
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

#[cfg(target_os = "macos")]
fn build_session(model_path: &Path) -> Result<Session> {
    use ort::execution_providers::CoreMLExecutionProvider;
    // `SessionBuilder` config methods return `Error<SessionBuilder>` (carrying
    // the builder for recovery); convert to `ort::Error<()>` so `?` can lift it
    // into `anyhow::Error` (which requires `Send + Sync`).
    let mut builder = Session::builder()?
        .with_execution_providers([CoreMLExecutionProvider::default().build()])
        .map_err(<ort::Error as From<_>>::from)?;
    Ok(builder.commit_from_file(model_path)?)
}

#[cfg(not(target_os = "macos"))]
fn build_session(model_path: &Path) -> Result<Session> {
    Ok(Session::builder()?.commit_from_file(model_path)?)
}

fn pick_output_name(session: &Session) -> Result<String> {
    // Prefer the common name for MiniLM-style encoders; otherwise pick the first
    // f32 output the model exposes. Logging the discovered name helps when a new
    // ONNX export changes conventions.
    let preferred = ["last_hidden_state", "sentence_embedding", "output"];
    for name in preferred {
        if session.outputs().iter().any(|o| o.name() == name) {
            return Ok(name.to_string());
        }
    }
    let first = session
        .outputs()
        .first()
        .ok_or_else(|| anyhow::anyhow!("model has no outputs"))?;
    tracing::warn!(
        output = %first.name(),
        "no known embedding output name; using first output"
    );
    Ok(first.name().to_string())
}

fn ensure_file(cache_dir: &Path, name: &str, url: &str, expected_sha: &str) -> Result<PathBuf> {
    let path = cache_dir.join(name);
    let expect_empty = expected_sha.is_empty();
    if path.exists() {
        let actual = sha256_file(&path)?;
        if expect_empty || actual.eq_ignore_ascii_case(expected_sha) {
            if expect_empty {
                tracing::warn!(
                    name,
                    sha = %actual,
                    "no expected sha pinned; paste this into profiles.toml"
                );
            }
            return Ok(path);
        }
        tracing::warn!(
            name,
            expected = %expected_sha,
            actual = %actual,
            "cached file sha mismatch — re-downloading"
        );
        std::fs::remove_file(&path)?;
    }
    let bytes = reqwest::blocking::get(url)?.error_for_status()?.bytes()?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let actual = hex(hasher.finalize().as_slice());
    if !expect_empty && !actual.eq_ignore_ascii_case(expected_sha) {
        bail!("downloaded {name} sha mismatch: expected {expected_sha}, got {actual}");
    }
    let mut f = File::create(&path)?;
    f.write_all(&bytes)?;
    if expect_empty {
        tracing::warn!(
            name,
            sha = %actual,
            "fetched without pinned sha; paste this into profiles.toml"
        );
    }
    Ok(path)
}

fn sha256_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Ok(hex(h.finalize().as_slice()))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(&mut s, "{b:02x}").unwrap();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::cosine;

    #[test]
    fn cosine_identical_is_one() {
        let v = vec![1.0, 2.0, 3.0];
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cosine_orthogonal_is_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!(cosine(&a, &b).abs() < 1e-9);
    }

    #[test]
    fn cosine_zero_vector_is_zero() {
        assert!(cosine(&[0.0, 0.0], &[1.0, 2.0]).abs() < 1e-9);
    }
}
