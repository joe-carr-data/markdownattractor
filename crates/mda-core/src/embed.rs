//! Card embeddings (ADR-0004).
//!
//! An [`Embedder`] turns texts into L2-normalised vectors. [`LocalEmbedder`] runs
//! `bge-small-en-v1.5` (quantised) through `fastembed` on the CPU, downloading the model into
//! the cache directory the first time it is used. Nothing here touches the store; the
//! pipeline decides what to embed and where to put it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(feature = "embeddings")]
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};

#[cfg(feature = "embeddings")]
use crate::Error;
use crate::Result;
use crate::card::SectionSummary;
use crate::config::{Config, EmbedText, Embeddings};

/// Name stored with every vector produced by [`LocalEmbedder`]. Changing the model means
/// changing this string, which invalidates every stored vector.
pub const LOCAL_SMALL_MODEL: &str = "bge-small-en-v1.5-q";

/// Dimension of [`LOCAL_SMALL_MODEL`].
pub const LOCAL_SMALL_DIM: usize = 384;

/// Texts per call to the model. Small enough to keep memory flat, large enough to amortise
/// the tokenizer.
pub const EMBED_BATCH: usize = 32;

/// Something that turns texts into vectors.
pub trait Embedder: Send + Sync {
    /// Stable model name, stored beside every vector (with the embedding-text variant's
    /// suffix when one is configured, so vectors of different texts are never mixed).
    fn model(&self) -> &str;
    /// Vector length.
    fn dim(&self) -> usize;
    /// `true` when [`Embedder::embed`] can run without fetching anything (model on disk or
    /// already loaded). Search asks this so a query never waits for a download.
    fn ready(&self) -> bool;
    /// Embed `texts`, one L2-normalised vector each, in order. Blocking: call from
    /// `spawn_blocking` inside async code.
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

/// A [`SectionSummary`] plus what surrounds it, flattened into the text that gets embedded
/// (the v1 order). Cards, not raw text: the vector should match the questions people ask
/// (plan §5).
#[must_use]
pub fn embed_text(
    title: Option<&str>,
    heading_path: &[String],
    summary: &SectionSummary,
) -> String {
    embed_text_as(EmbedText::V1, title, heading_path, summary)
}

/// The embedded text of a card under a configured variant (benchmark tuning, plan §3).
#[must_use]
pub fn embed_text_as(
    variant: EmbedText,
    title: Option<&str>,
    heading_path: &[String],
    summary: &SectionSummary,
) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(8);
    if let Some(t) = title.filter(|t| !t.trim().is_empty()) {
        parts.push(t.trim().to_owned());
    }
    if !heading_path.is_empty() {
        parts.push(heading_path.join(" › "));
    }
    let questions = summary.questions_answered.join(" ");
    if variant == EmbedText::QuestionsFirst && !questions.is_empty() {
        parts.push(questions.clone());
    }
    parts.push(summary.tldr.trim().to_owned());
    parts.push(summary.summary.trim().to_owned());
    if !summary.keywords.is_empty() {
        parts.push(summary.keywords.join(", "));
    }
    if variant != EmbedText::QuestionsFirst && !questions.is_empty() {
        parts.push(questions);
    }
    if variant == EmbedText::WithEntities {
        let e = &summary.entities;
        let entities: Vec<&str> = e
            .people
            .iter()
            .chain(&e.orgs)
            .chain(&e.products)
            .chain(&e.technologies)
            .chain(&e.files_paths)
            .chain(&e.commands)
            .map(String::as_str)
            .collect();
        if !entities.is_empty() {
            parts.push(entities.join(", "));
        }
    }
    parts.retain(|p| !p.is_empty());
    parts.join("\n")
}

/// An embedder whose model id carries the embedding-text variant, so the store keeps the
/// vectors of each text apart (`bge-small-en-v1.5-q+questions-first`).
pub struct VariantEmbedder {
    inner: Arc<dyn Embedder>,
    model: String,
}

impl VariantEmbedder {
    /// Wrap `inner` for `variant`; the default text needs no wrapper.
    #[must_use]
    pub fn new(inner: Arc<dyn Embedder>, variant: EmbedText) -> Self {
        let model = format!("{}{}", inner.model(), variant.suffix());
        Self { inner, model }
    }
}

impl Embedder for VariantEmbedder {
    fn model(&self) -> &str {
        &self.model
    }
    fn dim(&self) -> usize {
        self.inner.dim()
    }
    fn ready(&self) -> bool {
        self.inner.ready()
    }
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.inner.embed(texts)
    }
}

/// Scale `v` to unit length in place. A zero vector is left as is.
pub fn normalise(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// Dot product of two vectors of the same length (cosine similarity for unit vectors).
#[must_use]
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Where models are cached: the config, then `$MDA_MODEL_DIR`, then
/// `~/.cache/markdownattractor/models`. The plugin sets `MDA_MODEL_DIR` to its own data
/// directory in `.mcp.json` and in the launcher script; `CLAUDE_PLUGIN_DATA` is deliberately
/// not read here, because in a developer's shell it can belong to another plugin.
#[must_use]
pub fn cache_dir(cfg: &Config) -> PathBuf {
    if let Some(d) = &cfg.embedding_cache_dir {
        return d.clone();
    }
    if let Some(d) = std::env::var_os("MDA_MODEL_DIR").filter(|s| !s.is_empty()) {
        return PathBuf::from(d);
    }
    std::env::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".cache")
        .join("markdownattractor")
        .join("models")
}

/// What `mda doctor` reports about embeddings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbedCheck {
    /// Configured setting.
    pub embeddings: Embeddings,
    /// Model name, when embeddings are on.
    pub model: Option<String>,
    /// Cache directory.
    pub cache_dir: PathBuf,
    /// The model files are already present (no download needed).
    pub cached: bool,
}

/// Whether this binary was built with the `embeddings` feature (fastembed + ONNX Runtime).
pub const BUILT_WITH_EMBEDDINGS: bool = cfg!(feature = "embeddings");

/// Inspect the embedding setup without loading or downloading anything.
#[must_use]
pub fn check(cfg: &Config) -> EmbedCheck {
    let dir = cache_dir(cfg);
    match cfg.embeddings {
        Embeddings::Off => {
            EmbedCheck { embeddings: Embeddings::Off, model: None, cache_dir: dir, cached: false }
        }
        Embeddings::LocalSmall if !BUILT_WITH_EMBEDDINGS => EmbedCheck {
            embeddings: Embeddings::LocalSmall,
            model: None,
            cache_dir: dir,
            cached: false,
        },
        Embeddings::LocalSmall => EmbedCheck {
            embeddings: Embeddings::LocalSmall,
            model: Some(LOCAL_SMALL_MODEL.to_owned()),
            cached: LocalEmbedder::is_cached(&dir),
            cache_dir: dir,
        },
    }
}

/// The embedder the config asks for, or `None` when embeddings are off or the binary was
/// built without them. Nothing is loaded until the first call to [`Embedder::embed`].
#[must_use]
pub fn embedder_for(cfg: &Config) -> Option<Arc<dyn Embedder>> {
    match cfg.embeddings {
        Embeddings::Off => None,
        #[cfg(feature = "embeddings")]
        Embeddings::LocalSmall => {
            let base: Arc<dyn Embedder> = Arc::new(LocalEmbedder::new(cache_dir(cfg)));
            Some(match cfg.embedding_text {
                EmbedText::V1 => base,
                v => Arc::new(VariantEmbedder::new(base, v)),
            })
        }
        #[cfg(not(feature = "embeddings"))]
        Embeddings::LocalSmall => {
            tracing::warn!("built without the `embeddings` feature; search is lexical");
            None
        }
    }
}

/// `bge-small-en-v1.5` (quantised) through fastembed. Lazily initialised; the first
/// [`Embedder::embed`] downloads ~33 MB into the cache directory if they are not there.
/// With the `embeddings` feature off, only [`LocalEmbedder::is_cached`] exists.
pub struct LocalEmbedder {
    cache_dir: PathBuf,
    #[cfg(feature = "embeddings")]
    model: Mutex<Option<fastembed::TextEmbedding>>,
}

impl std::fmt::Debug for LocalEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalEmbedder").field("cache_dir", &self.cache_dir).finish_non_exhaustive()
    }
}

impl LocalEmbedder {
    /// An embedder caching its model under `cache_dir`.
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            #[cfg(feature = "embeddings")]
            model: Mutex::new(None),
        }
    }

    /// Whether every artifact the model needs is already in `cache_dir`, laid out as
    /// `hf-hub` does it (`models--<org>--<repo>/snapshots/<rev>/<file>`). A directory that
    /// merely exists is not enough: an interrupted download leaves one behind, and loading
    /// from it would fetch the missing files, which a query must never do.
    #[must_use]
    pub fn is_cached(cache_dir: &Path) -> bool {
        const REQUIRED: &[&str] =
            &["model_optimized.onnx", "tokenizer.json", "config.json", "tokenizer_config.json"];
        let Ok(repos) = std::fs::read_dir(cache_dir) else { return false };
        repos.filter_map(std::result::Result::ok).any(|repo| {
            let name = repo.file_name().to_string_lossy().to_lowercase();
            if !name.contains("bge-small-en-v1.5") {
                return false;
            }
            let Ok(snapshots) = std::fs::read_dir(repo.path().join("snapshots")) else {
                return false;
            };
            snapshots.filter_map(std::result::Result::ok).any(|snap| {
                REQUIRED.iter().all(|f| {
                    std::fs::metadata(snap.path().join(f)).is_ok_and(|m| m.is_file() && m.len() > 0)
                })
            })
        })
    }

    #[cfg(feature = "embeddings")]
    fn lock(&self) -> MutexGuard<'_, Option<fastembed::TextEmbedding>> {
        self.model.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[cfg(feature = "embeddings")]
    fn load(&self) -> Result<()> {
        let mut slot = self.lock();
        if slot.is_some() {
            return Ok(());
        }
        std::fs::create_dir_all(&self.cache_dir).map_err(|e| Error::io(&self.cache_dir, e))?;
        let cached = Self::is_cached(&self.cache_dir);
        if !cached {
            tracing::info!(dir = %self.cache_dir.display(), "downloading the embedding model (~33 MB, once)");
        }
        let started = std::time::Instant::now();
        let opts = fastembed::TextInitOptions::new(fastembed::EmbeddingModel::BGESmallENV15Q)
            .with_cache_dir(self.cache_dir.clone())
            .with_show_download_progress(false);
        let model = fastembed::TextEmbedding::try_new(opts)
            .map_err(|e| Error::Embed(format!("cannot load {LOCAL_SMALL_MODEL}: {e}")))?;
        tracing::info!(
            ms = started.elapsed().as_millis(),
            downloaded = !cached,
            "embedding model ready"
        );
        *slot = Some(model);
        Ok(())
    }
}

#[cfg(feature = "embeddings")]
impl Embedder for LocalEmbedder {
    fn model(&self) -> &'static str {
        LOCAL_SMALL_MODEL
    }

    fn dim(&self) -> usize {
        LOCAL_SMALL_DIM
    }

    fn ready(&self) -> bool {
        self.lock().is_some() || Self::is_cached(&self.cache_dir)
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        self.load()?;
        let mut slot = self.lock();
        let model = slot.as_mut().ok_or_else(|| Error::Embed("model not loaded".to_owned()))?;
        let mut out = model
            .embed(texts, Some(EMBED_BATCH))
            .map_err(|e| Error::Embed(format!("embedding failed: {e}")))?;
        for v in &mut out {
            if v.len() != LOCAL_SMALL_DIM {
                return Err(Error::Embed(format!(
                    "model returned {} dimensions, expected {LOCAL_SMALL_DIM}",
                    v.len()
                )));
            }
            normalise(v);
        }
        Ok(out)
    }
}

/// A deterministic embedder for tests: hashes words into a small vector, so texts sharing
/// words are close. Not a language model.
#[derive(Debug, Clone)]
pub struct HashEmbedder {
    dim: usize,
}

impl HashEmbedder {
    /// An embedder producing `dim`-dimensional vectors.
    pub fn new(dim: usize) -> Self {
        Self { dim: dim.max(1) }
    }
}

impl Embedder for HashEmbedder {
    fn model(&self) -> &'static str {
        "hash-test"
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn ready(&self) -> bool {
        true
    }

    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|t| {
                let mut v = vec![0.0f32; self.dim];
                for word in t.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
                    let h = blake3::hash(word.to_lowercase().as_bytes());
                    let idx = usize::from(h.as_bytes()[0]) % self.dim;
                    let sign = if h.as_bytes()[1].is_multiple_of(2) { 1.0 } else { -1.0 };
                    v[idx] += sign;
                }
                normalise(&mut v);
                v
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::Entities;

    fn summary() -> SectionSummary {
        SectionSummary {
            tldr: "Roll back with deployctl.".into(),
            summary: "The rollback runs deployctl against the previous release.".into(),
            keywords: vec!["rollback".into(), "deployctl".into()],
            questions_answered: vec!["How do I roll back?".into()],
            entities: Entities::default(),
            mentioned_dates: vec![],
            decisions: vec![],
            action_items: vec![],
        }
    }

    #[test]
    fn embed_text_variants_reorder_and_extend_the_text() {
        let mut s = summary();
        s.entities.products = vec!["deployctl".into()];
        s.entities.files_paths = vec!["ops/rollback.md".into()];
        let v1 = embed_text_as(EmbedText::V1, Some("Runbook"), &["Deploy".into()], &s);
        let qf = embed_text_as(EmbedText::QuestionsFirst, Some("Runbook"), &["Deploy".into()], &s);
        let we = embed_text_as(EmbedText::WithEntities, Some("Runbook"), &["Deploy".into()], &s);
        assert_eq!(v1, embed_text(Some("Runbook"), &["Deploy".into()], &s));
        let qf_lines: Vec<&str> = qf.lines().collect();
        assert_eq!(qf_lines[2], "How do I roll back?", "questions before the tldr: {qf}");
        assert_eq!(qf.lines().count(), v1.lines().count(), "same parts, another order");
        assert!(we.starts_with(&v1) && we.ends_with("deployctl, ops/rollback.md"), "{we}");
        assert_eq!(EmbedText::V1.suffix(), "");
        let inner: Arc<dyn Embedder> = Arc::new(HashEmbedder::new(8));
        let v = VariantEmbedder::new(Arc::clone(&inner), EmbedText::QuestionsFirst);
        assert_eq!(v.model(), format!("{}+questions-first", inner.model()));
        assert_eq!(v.dim(), inner.dim());
    }

    #[test]
    fn embed_text_joins_the_card_fields_in_order() {
        let t = embed_text(Some("Runbook"), &["Deploy".into(), "Rollback".into()], &summary());
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], "Runbook");
        assert_eq!(lines[1], "Deploy › Rollback");
        assert_eq!(lines[2], "Roll back with deployctl.");
        assert!(lines[4].contains("rollback, deployctl"));
        assert!(lines[5].contains("How do I roll back?"));
        let no_title = embed_text(None, &[], &summary());
        assert!(no_title.starts_with("Roll back"));
    }

    #[test]
    fn normalise_and_dot() {
        let mut v = vec![3.0, 4.0];
        normalise(&mut v);
        assert!((dot(&v, &v) - 1.0).abs() < 1e-6);
        let mut z = vec![0.0, 0.0];
        normalise(&mut z);
        assert_eq!(z, vec![0.0, 0.0]);
    }

    #[test]
    fn hash_embedder_is_deterministic_and_word_sensitive() {
        let e = HashEmbedder::new(64);
        let a =
            e.embed(&["deploy rollback".into(), "deploy rollback".into(), "zebra".into()]).unwrap();
        assert_eq!(a[0], a[1]);
        assert!(dot(&a[0], &a[1]) > 0.99);
        assert!(dot(&a[0], &a[2]).abs() < 0.9);
        assert_eq!(e.dim(), 64);
        assert!(e.embed(&[]).unwrap().is_empty());
    }

    #[test]
    fn cache_dir_precedence() {
        let cfg =
            Config { embedding_cache_dir: Some(PathBuf::from("/x/models")), ..Config::default() };
        assert_eq!(cache_dir(&cfg), PathBuf::from("/x/models"));
        let dir = cache_dir(&Config::default());
        assert!(dir.ends_with("models"), "{}", dir.display());
    }

    #[test]
    fn is_cached_requires_every_artifact() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("models--Qdrant--bge-small-en-v1.5-onnx-Q");
        std::fs::create_dir_all(&repo).unwrap();
        assert!(!LocalEmbedder::is_cached(tmp.path()), "an empty repo directory is not a model");
        let snap = repo.join("snapshots").join("abc");
        std::fs::create_dir_all(&snap).unwrap();
        for f in ["model_optimized.onnx", "tokenizer.json", "config.json"] {
            std::fs::write(snap.join(f), b"x").unwrap();
        }
        assert!(!LocalEmbedder::is_cached(tmp.path()), "one file missing");
        std::fs::write(snap.join("tokenizer_config.json"), b"x").unwrap();
        assert!(LocalEmbedder::is_cached(tmp.path()));
        std::fs::write(snap.join("model_optimized.onnx"), b"").unwrap();
        assert!(!LocalEmbedder::is_cached(tmp.path()), "an empty artifact is a broken download");
    }

    #[test]
    fn check_reports_off_and_uncached() {
        let off = check(&Config { embeddings: Embeddings::Off, ..Config::default() });
        assert_eq!(off.model, None);
        assert!(
            embedder_for(&Config { embeddings: Embeddings::Off, ..Config::default() }).is_none()
        );
        let tmp = tempfile::tempdir().unwrap();
        let cfg =
            Config { embedding_cache_dir: Some(tmp.path().to_path_buf()), ..Config::default() };
        let on = check(&cfg);
        let expected = if BUILT_WITH_EMBEDDINGS { Some(LOCAL_SMALL_MODEL) } else { None };
        assert_eq!(on.model.as_deref(), expected);
        assert!(!on.cached);
        assert_eq!(embedder_for(&cfg).is_some(), BUILT_WITH_EMBEDDINGS);
    }

    /// Downloads the model once (~33 MB) and embeds two texts. Run with
    /// `MDA_LIVE_EMBED=1 cargo nextest run -E 'test(live_embed)' --run-ignored ignored-only`.
    #[cfg(feature = "embeddings")]
    #[test]
    #[ignore = "downloads a model; opt in with MDA_LIVE_EMBED=1"]
    fn live_embed_bge_small() {
        if std::env::var("MDA_LIVE_EMBED").is_err() {
            return;
        }
        let e = LocalEmbedder::new(cache_dir(&Config::default()));
        let v = e
            .embed(&[
                "how do we roll back a deploy".into(),
                "recipe for pancakes".into(),
                "revert the release".into(),
            ])
            .unwrap();
        assert_eq!(v[0].len(), LOCAL_SMALL_DIM);
        assert!(
            dot(&v[0], &v[2]) > dot(&v[0], &v[1]),
            "rollback is closer to revert than to pancakes"
        );
    }
}
