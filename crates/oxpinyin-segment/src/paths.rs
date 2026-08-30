//! Path discovery for the fetched model cache and the export directory.
//!
//! Mirrors `pinyin_oracle::locate_model_dir` without depending on the
//! oracle crate (the segmenter is portable and never-ship; it must not
//! pull the Linux-first harness).

use std::path::{Path, PathBuf};

use oxpinyin_data::default_store_file;

use crate::error::SegmentError;
use crate::model::parse_table_conf_lambda;

/// Environment variable naming the extracted model20 directory.
pub const MODEL_DIR_ENV: &str = "PINYIN_MODEL_DIR";

/// Environment variable naming the fetch-script cache root (`…/extracted`
/// is appended).
pub const MODEL_CACHE_ENV: &str = "PINYIN_MODEL_CACHE";

/// Environment variable naming the export directory.
pub const EXPORT_DIR_ENV: &str = "PINYIN_EXPORT_DIR";

/// Default export directory used by the oracle integration tests.
pub const DEFAULT_EXPORT_DIR: &str = "/tmp/oxpinyin-export";

/// Table stems the segmenter needs from the export directory; the
/// on-disk names carry the compiled-in peer backend's extension
/// (`default_store_file` — `.kct` for Kyoto Cabinet, `.redb` for redb,
/// `.lmdb` for LMDB, `.tkt` for tkrzw).
pub const EXPORT_STEMS: &[&str] = &["phrase_index", "bigram"];

/// Files the segmenter needs from the fetched model20 cache.
pub const MODEL_FILES: &[&str] = &["interpolation2.text"];

/// Locations of the three tables `Segmenter::open` reads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SegmenterPaths {
    /// The `phrase_index` table, in the compiled-in backend's format,
    /// from the export directory.
    pub phrase_index: PathBuf,
    /// The `bigram` table, in the compiled-in backend's format, from the
    /// export directory.
    pub bigram: PathBuf,
    /// `interpolation2.text` from the fetched model20 cache.
    pub interpolation2: PathBuf,
}

impl SegmenterPaths {
    /// Resolves the export directory and the model20 cache.
    ///
    /// # Errors
    ///
    /// Returns [`SegmentError::MissingPath`] when either side is absent.
    pub fn discover() -> Result<Self, SegmentError> {
        let export = locate_export_dir().ok_or_else(|| SegmentError::MissingPath {
            detail: format!(
                "no export at ${EXPORT_DIR_ENV} or {DEFAULT_EXPORT_DIR}; \
                 set ${{EXPORT_DIR_ENV}} to an export directory such as \
                 fixtures/w3/, which holds the committed tables"
            ),
        })?;
        let model = locate_model_dir().ok_or_else(|| SegmentError::MissingPath {
            detail: format!(
                "no model20 cache at ${MODEL_DIR_ENV} / ${MODEL_CACHE_ENV} / \
                 target/model20/extracted; run tools/model/fetch-model.sh"
            ),
        })?;
        Ok(Self::from_dirs(&export, &model))
    }

    /// Builds paths from an export directory and a model20 directory.
    #[must_use]
    pub fn from_dirs(export: &Path, model: &Path) -> Self {
        Self {
            phrase_index: export.join(default_store_file("phrase_index")),
            bigram: export.join(default_store_file("bigram")),
            interpolation2: model.join("interpolation2.text"),
        }
    }
}

/// Locates a complete system-table export directory
/// (`oxpinyin-datagen compile` output, or the historical export) holding
/// the tables in the compiled-in backend's format.
#[must_use]
pub fn locate_export_dir() -> Option<PathBuf> {
    if let Some(raw) = std::env::var_os(EXPORT_DIR_ENV) {
        let path = PathBuf::from(raw);
        if dir_has_tables(&path, EXPORT_STEMS) {
            return Some(path);
        }
        return None;
    }
    let default = PathBuf::from(DEFAULT_EXPORT_DIR);
    dir_has_tables(&default, EXPORT_STEMS).then_some(default)
}

/// Locates a complete extracted model20 directory.
#[must_use]
pub fn locate_model_dir() -> Option<PathBuf> {
    if let Some(raw) = std::env::var_os(MODEL_DIR_ENV) {
        let path = PathBuf::from(raw);
        return dir_has(&path, MODEL_FILES).then_some(path);
    }
    if let Some(raw) = std::env::var_os(MODEL_CACHE_ENV) {
        let candidate = PathBuf::from(raw).join("extracted");
        if dir_has(&candidate, MODEL_FILES) {
            return Some(candidate);
        }
    }
    if let Some(target) = std::env::var_os("CARGO_TARGET_DIR") {
        let candidate = PathBuf::from(target).join("model20").join("extracted");
        if dir_has(&candidate, MODEL_FILES) {
            return Some(candidate);
        }
    }
    workspace_root().and_then(|root| {
        let candidate = root.join("target").join("model20").join("extracted");
        dir_has(&candidate, MODEL_FILES).then_some(candidate)
    })
}

/// Reads λ from `table.conf` if the file exists and parses.
#[must_use]
pub fn load_lambda(path: Option<&Path>) -> Option<f32> {
    let path = path?;
    let text = std::fs::read_to_string(path).ok()?;
    parse_table_conf_lambda(&text)
}

fn dir_has(dir: &Path, names: &[&str]) -> bool {
    dir.is_dir() && names.iter().all(|name| dir.join(name).is_file())
}

/// `dir_has` over table stems: the checked name of each stem is its
/// compiled-in-backend form (`phrase_index.kct`, `phrase_index.redb`, …).
fn dir_has_tables(dir: &Path, stems: &[&str]) -> bool {
    dir.is_dir()
        && stems
            .iter()
            .all(|stem| dir.join(default_store_file(stem)).is_file())
}

fn workspace_root() -> Option<PathBuf> {
    let mut dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    loop {
        if dir.join("crates/oxpinyin-segment/Cargo.toml").is_file()
            && dir.join("docs/findings").is_dir()
        {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}
