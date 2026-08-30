//! Location of the checksum-pinned model20 text export.
//!
//! `tools/model/fetch-model.sh` downloads the archive pinned by
//! `docs/findings/model-provenance.md`, verifies SHA-256, and extracts the
//! eighteen files into a build cache. This module only *finds* that cache so
//! tests and benches can take a model-dir path; it does not read
//! frequencies, score, or change what the decoder consumes.
//!
//! Canonical home: `pinyin-oracle` re-exports this module (its bins and
//! public API keep `pinyin_oracle::model_cache`), and the capi benches
//! consume it directly — dev-dependency edges only, never a
//! `oxpinyin-capi` → `pinyin-oracle` edge.

use std::path::{Path, PathBuf};

/// Pinned SourceForge URL from `docs/findings/model-provenance.md`.
pub const MODEL20_URL: &str =
    "https://downloads.sourceforge.net/libpinyin/models/model20.text.tar.gz";

/// Pinned SHA-256 of [`MODEL20_URL`], lowercase hex.
pub const MODEL20_SHA256: &str = "59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155";

/// Environment variable naming the extracted model directory explicitly.
pub const MODEL_DIR_ENV: &str = "PINYIN_MODEL_DIR";

/// Environment variable naming the fetch-script cache root (`…/extracted`
/// is appended).
pub const MODEL_CACHE_ENV: &str = "PINYIN_MODEL_CACHE";

/// Directory name under `target/` (or `CARGO_TARGET_DIR`) used as the default
/// cache root.
pub const DEFAULT_CACHE_DIR_NAME: &str = "model20";

/// Subdirectory of the cache root that holds the eighteen export files.
pub const EXTRACTED_SUBDIR: &str = "extracted";

/// The eighteen files the pinned archive must expand to.
///
/// Inventory from `docs/findings/model-provenance.md`: `interpolation2.text`
/// plus seventeen `.table` files. Keep in lockstep with
/// `tools/model/fetch-model.sh`.
pub const EXPECTED_MODEL_FILES: &[&str] = &[
    "art.table",
    "culture.table",
    "economy.table",
    "gb_char.table",
    "gbk_char.table",
    "geology.table",
    "history.table",
    "interpolation2.text",
    "life.table",
    "merged.table",
    "nature.table",
    "opengram.table",
    "people.table",
    "punct.table",
    "science.table",
    "society.table",
    "sport.table",
    "technology.table",
];

/// Why a candidate model directory is not usable.
#[derive(Debug, Clone, Eq, PartialEq)]
#[non_exhaustive]
pub enum ModelDirError {
    /// The path is missing or is not a directory.
    NotADirectory {
        /// Path that was examined.
        path: PathBuf,
    },
    /// The directory exists but does not contain every expected export file.
    Incomplete {
        /// Path that was examined.
        path: PathBuf,
        /// Expected file names that were absent.
        missing: Vec<&'static str>,
    },
}

impl core::fmt::Display for ModelDirError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotADirectory { path } => {
                write!(
                    formatter,
                    "model export directory {path:?} is not a directory"
                )
            }
            Self::Incomplete { path, missing } => write!(
                formatter,
                "model export directory {path:?} is missing {missing:?}; \
                 run tools/model/fetch-model.sh"
            ),
        }
    }
}

impl std::error::Error for ModelDirError {}

/// Expected export file names that are not present as files under `dir`.
#[must_use]
pub fn missing_model_files(dir: &Path) -> Vec<&'static str> {
    EXPECTED_MODEL_FILES
        .iter()
        .copied()
        .filter(|name| !dir.join(name).is_file())
        .collect()
}

/// Whether `dir` contains every file in [`EXPECTED_MODEL_FILES`].
#[must_use]
pub fn model_dir_is_complete(dir: &Path) -> bool {
    dir.is_dir() && missing_model_files(dir).is_empty()
}

/// Accepts `dir` only when it holds the eighteen expected export files.
///
/// # Errors
///
/// Returns [`ModelDirError::NotADirectory`] when `dir` is not a directory,
/// or [`ModelDirError::Incomplete`] when any expected file is missing.
pub fn verified_model_dir(dir: &Path) -> Result<PathBuf, ModelDirError> {
    if !dir.is_dir() {
        return Err(ModelDirError::NotADirectory {
            path: dir.to_path_buf(),
        });
    }
    let missing = missing_model_files(dir);
    if !missing.is_empty() {
        return Err(ModelDirError::Incomplete {
            path: dir.to_path_buf(),
            missing,
        });
    }
    Ok(dir.to_path_buf())
}

/// Default extracted directory under the workspace `target/` tree.
///
/// Honours `CARGO_TARGET_DIR` when set; otherwise `<workspace>/target/model20/extracted`.
#[must_use]
pub fn default_extracted_dir() -> Option<PathBuf> {
    if let Some(target) = std::env::var_os("CARGO_TARGET_DIR") {
        return Some(
            PathBuf::from(target)
                .join(DEFAULT_CACHE_DIR_NAME)
                .join(EXTRACTED_SUBDIR),
        );
    }
    workspace_root().map(|root| {
        root.join("target")
            .join(DEFAULT_CACHE_DIR_NAME)
            .join(EXTRACTED_SUBDIR)
    })
}

/// Locates a complete extracted model directory.
///
/// Order:
///
/// 1. [`MODEL_DIR_ENV`] (`PINYIN_MODEL_DIR`) — must be complete if set;
/// 2. [`MODEL_CACHE_ENV`] (`PINYIN_MODEL_CACHE`) `/extracted`;
/// 3. `$CARGO_TARGET_DIR/model20/extracted` when `CARGO_TARGET_DIR` is set;
/// 4. `<workspace>/target/model20/extracted`.
///
/// # Errors
///
/// Returns [`ModelDirError`] only when `PINYIN_MODEL_DIR` is set and unusable.
/// An unset environment and a missing default cache yield `Ok(None)` so
/// callers can skip rather than fail.
pub fn locate_model_dir() -> Result<Option<PathBuf>, ModelDirError> {
    if let Some(raw) = std::env::var_os(MODEL_DIR_ENV) {
        return verified_model_dir(&PathBuf::from(raw)).map(Some);
    }

    if let Some(raw) = std::env::var_os(MODEL_CACHE_ENV) {
        let candidate = PathBuf::from(raw).join(EXTRACTED_SUBDIR);
        if let Ok(path) = verified_model_dir(&candidate) {
            return Ok(Some(path));
        }
    }

    if let Some(candidate) = default_extracted_dir()
        && let Ok(path) = verified_model_dir(&candidate)
    {
        return Ok(Some(path));
    }

    Ok(None)
}

/// `/tmp/oxpinyin-export` or `$PINYIN_EXPORT_DIR`; asserts the exported
/// tables exist, so benches refuse to start on fixture-less hosts. The
/// tables are opened through the compiled-in backend, so their extension
/// follows [`oxpinyin_store::default_store_file`] (`.kct` under the KC
/// default, `.redb` under `--no-default-features --features redb`, …).
///
/// # Panics
///
/// When the export directory does not hold the three committed tables.
#[must_use]
pub fn export_dir() -> PathBuf {
    let dir = std::env::var_os("PINYIN_EXPORT_DIR")
        .map_or_else(|| PathBuf::from("/tmp/oxpinyin-export"), PathBuf::from);
    for stem in ["pinyin_index", "phrase_index", "bigram"] {
        let name = oxpinyin_store::default_store_file(stem);
        assert!(
            dir.join(&name).is_file(),
            "exported tables missing at {} ({name}); tables are committed under fixtures/w3/",
            dir.display()
        );
    }
    dir
}

/// Workspace root that contains the training crates and the provenance finding.
fn workspace_root() -> Option<PathBuf> {
    workspace_root_from(Path::new(env!("CARGO_MANIFEST_DIR")))
}

fn workspace_root_from(start: &Path) -> Option<PathBuf> {
    let mut dir = start;
    loop {
        if dir.join("crates/pinyin-oracle/Cargo.toml").is_file()
            && dir.join("docs/findings/model-provenance.md").is_file()
        {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EXPECTED_MODEL_FILES, MODEL20_SHA256, MODEL20_URL, ModelDirError, missing_model_files,
        model_dir_is_complete, verified_model_dir, workspace_root,
    };
    use std::path::PathBuf;

    #[test]
    fn expected_export_is_interpolation2_and_seventeen_tables() {
        assert_eq!(EXPECTED_MODEL_FILES.len(), 18);
        assert_eq!(
            EXPECTED_MODEL_FILES
                .iter()
                .filter(|name| name.ends_with(".table"))
                .count(),
            17
        );
        assert!(EXPECTED_MODEL_FILES.contains(&"interpolation2.text"));
        assert_eq!(
            EXPECTED_MODEL_FILES
                .iter()
                .filter(|name| **name == "interpolation2.text")
                .count(),
            1
        );
    }

    #[test]
    fn pin_constants_match_the_frozen_provenance() {
        assert_eq!(
            MODEL20_URL,
            "https://downloads.sourceforge.net/libpinyin/models/model20.text.tar.gz"
        );
        assert_eq!(
            MODEL20_SHA256,
            "59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155"
        );
    }

    #[test]
    fn workspace_root_is_this_repository() {
        let root = workspace_root().expect("compiled from the workspace");
        assert!(root.join("tools/model/fetch-model.sh").is_file());
    }

    #[test]
    fn missing_directory_is_incomplete() {
        let path = PathBuf::from("/no/such/oxpinyin-model-dir");
        assert!(!model_dir_is_complete(&path));
        assert_eq!(missing_model_files(&path).len(), 18);
        assert!(matches!(
            verified_model_dir(&path),
            Err(ModelDirError::NotADirectory { .. })
        ));
    }
}
