//! Local embeddings: ONNX inference on this machine, never a model API.
//!
//! The model (bge-small-en-v1.5, quantized, 384 dimensions) is fetched once
//! from HuggingFace and cached under `~/.aichip/models` — an artifact
//! download in the same class as a cargo dependency, carrying no user
//! content anywhere. Everything after that first fetch is offline.
//!
//! The embedder is a lazy global behind a mutex: fastembed's `embed` is
//! synchronous CPU work, so every call runs under `spawn_blocking`, and the
//! mutex both guards the lazy init and serializes the heavy part.

use std::sync::{Mutex, OnceLock};

use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};

/// Tags every stored chunk. Retrieval filters on it, so swapping models makes
/// old rows invisible (and re-embedded by the next reconcile) rather than
/// garbage-ranked against vectors from a different space.
pub const MODEL_TAG: &str = "bge-small-en-v1.5-q/384";
pub const DIM: usize = 384;

/// Where the embedder stands, for the documents panel to report.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "detail")]
pub enum EmbedStatus {
    /// Never asked for yet.
    NotReady,
    /// First call in flight — likely the one-time model download.
    Downloading,
    Ready,
    /// Why, verbatim. A later call retries: the commonest cause is a network
    /// blip during the one-time download, and "failed forever" would turn
    /// that into a reinstall.
    Failed(String),
}

static EMBEDDER: OnceLock<Mutex<Option<TextEmbedding>>> = OnceLock::new();
static STATUS: Mutex<Option<EmbedStatus>> = Mutex::new(None);

pub fn status() -> EmbedStatus {
    STATUS
        .lock()
        .unwrap()
        .clone()
        .unwrap_or(EmbedStatus::NotReady)
}

fn set_status(s: EmbedStatus) {
    *STATUS.lock().unwrap() = Some(s);
}

/// Embed a batch. Blocking work runs off the async runtime.
pub async fn embed_batch(texts: Vec<String>) -> anyhow::Result<Vec<Vec<f32>>> {
    if texts.is_empty() {
        return Ok(vec![]);
    }
    if status() == EmbedStatus::NotReady {
        set_status(EmbedStatus::Downloading);
    }
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<Vec<f32>>> {
        let cell = EMBEDDER.get_or_init(|| Mutex::new(None));
        let mut guard = cell.lock().unwrap();
        if guard.is_none() {
            let cache = std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".aichip")
                .join("models");
            std::fs::create_dir_all(&cache)?;
            let model = TextEmbedding::try_new(
                TextInitOptions::new(EmbeddingModel::BGESmallENV15Q)
                    .with_cache_dir(cache)
                    .with_show_download_progress(false),
            )?;
            *guard = Some(model);
        }
        let model = guard.as_mut().expect("initialized above");
        Ok(model.embed(texts, Some(64))?)
    })
    .await?;

    match &result {
        Ok(_) => set_status(EmbedStatus::Ready),
        Err(e) => set_status(EmbedStatus::Failed(e.to_string())),
    }
    result
}

/// f32 slice → little-endian bytes, the storage format.
pub fn to_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

/// The inverse. Errors on a length no vector could have — a truncated read
/// must be loud, not a silently shorter vector that cosines against nothing.
pub fn from_bytes(b: &[u8]) -> anyhow::Result<Vec<f32>> {
    if b.len() % 4 != 0 {
        anyhow::bail!(
            "embedding blob of {} bytes is not a whole number of f32s",
            b.len()
        );
    }
    Ok(b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

/// Cosine similarity. 0.0 — never a panic — on dimension mismatch or a zero
/// vector, because a bad row should rank last, not take retrieval down.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_round_trip_exactly() {
        let v = vec![0.0f32, 1.5, -3.25, f32::MIN_POSITIVE, 1e30];
        assert_eq!(from_bytes(&to_bytes(&v)).unwrap(), v);
    }

    #[test]
    fn a_truncated_blob_is_an_error_not_a_shorter_vector() {
        let mut b = to_bytes(&[1.0, 2.0]);
        b.pop();
        assert!(from_bytes(&b).is_err());
    }

    #[test]
    fn cosine_behaves_at_the_edges() {
        let a = [1.0, 0.0, 0.0];
        assert!((cosine(&a, &a) - 1.0).abs() < 1e-6);
        assert_eq!(cosine(&a, &[0.0, 1.0, 0.0]), 0.0);
        // Mismatched dimensions and zero vectors rank last, never panic.
        assert_eq!(cosine(&a, &[1.0, 0.0]), 0.0);
        assert_eq!(cosine(&a, &[0.0, 0.0, 0.0]), 0.0);
        assert_eq!(cosine(&[], &[]), 0.0);
    }
}
