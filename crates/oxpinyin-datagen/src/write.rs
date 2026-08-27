//! Backend-generic table writing and backend selection.
//!
//! Every table is the frozen single-`data`-table schema
//! (`docs/findings/data-layer-export.md`) written through
//! [`WriteStore`], so the same compiled entries produce a redb, LMDB, or
//! Tkrzw file. After writing, the file is re-opened read-only and every
//! row is compared against the compiled entries — a table is only reported
//! written after it reads back identical.

use std::fs;
use std::path::{Path, PathBuf};

use oxpinyin_store::WriteStore;

use crate::{DatagenError, Entries};

/// The single table name of the frozen schema.
pub const TABLE: &str = "data";

/// A storage backend with a native producer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Backend {
    /// redb — the default engine backend (`.redb` files).
    Redb,
    /// LMDB, single-file environments (`.lmdb` files). Requires the `lmdb`
    /// cargo feature.
    Lmdb,
    /// Tkrzw TreeDBM (`.tkt` files). Requires the `tkrzw` cargo feature.
    Tkrzw,
}

impl Backend {
    /// Parses a `--backend` argument.
    ///
    /// # Errors
    ///
    /// Unknown backend names.
    pub fn parse(name: &str) -> Result<Self, DatagenError> {
        match name {
            "redb" => Ok(Self::Redb),
            "lmdb" => Ok(Self::Lmdb),
            "tkrzw" => Ok(Self::Tkrzw),
            other => Err(DatagenError::Consistency(format!(
                "unknown backend {other:?} (expected redb, lmdb, or tkrzw)"
            ))),
        }
    }

    /// Whether this backend was compiled in (its cargo feature is on).
    #[must_use]
    pub fn available(self) -> bool {
        match self {
            Self::Redb => true,
            Self::Lmdb => cfg!(feature = "lmdb"),
            Self::Tkrzw => cfg!(feature = "tkrzw"),
        }
    }

    /// File extension for this backend's tables.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Redb => "redb",
            Self::Lmdb => "lmdb",
            Self::Tkrzw => "tkt",
        }
    }

    /// The cargo feature that compiles this backend in (differs from the
    /// file extension for Tkrzw).
    #[must_use]
    pub fn feature(self) -> &'static str {
        match self {
            Self::Redb => "redb",
            Self::Lmdb => "lmdb",
            Self::Tkrzw => "tkrzw",
        }
    }

    /// Output path for a table base name (e.g. `pinyin_index`).
    #[must_use]
    pub fn table_path(self, out_dir: &Path, base: &str) -> PathBuf {
        out_dir.join(format!("{base}.{}", self.extension()))
    }

    /// Writes `entries` to `path` through this backend.
    ///
    /// # Errors
    ///
    /// Fails if the backend was not compiled in, or on any store or
    /// verification failure.
    pub fn write(self, path: &Path, entries: &Entries) -> Result<(), DatagenError> {
        match self {
            Self::Redb => write_with::<oxpinyin_store::RedbStore>(path, entries),
            #[cfg(feature = "lmdb")]
            Self::Lmdb => write_with::<oxpinyin_store::LmdbStore>(path, entries),
            #[cfg(feature = "tkrzw")]
            Self::Tkrzw => write_with::<oxpinyin_store::TkrzwStore>(path, entries),
            #[allow(unreachable_patterns)]
            backend => Err(DatagenError::Consistency(format!(
                "backend {:?} requires rebuilding this crate with --features {}",
                backend,
                backend.feature()
            ))),
        }
    }

    /// Reads every `(key, value)` pair of the table at `path`, in ascending
    /// key-byte order.
    ///
    /// # Errors
    ///
    /// Fails if the backend was not compiled in, or on any store failure.
    pub fn read_all(self, path: &Path) -> Result<Entries, DatagenError> {
        use oxpinyin_store::ReadStore;
        fn collect<S: ReadStore>(path: &Path) -> Result<Entries, DatagenError> {
            let store = S::open_read_only(path)?;
            let mut rows: Entries = Vec::new();
            store.for_each(TABLE, &mut |key, value| {
                rows.push((key.to_vec(), value.to_vec()));
                Ok(())
            })?;
            Ok(rows)
        }
        match self {
            Self::Redb => collect::<oxpinyin_store::RedbStore>(path),
            #[cfg(feature = "lmdb")]
            Self::Lmdb => collect::<oxpinyin_store::LmdbStore>(path),
            #[cfg(feature = "tkrzw")]
            Self::Tkrzw => collect::<oxpinyin_store::TkrzwStore>(path),
            #[allow(unreachable_patterns)]
            backend => Err(DatagenError::Consistency(format!(
                "backend {:?} requires rebuilding this crate with --features {}",
                backend,
                backend.feature()
            ))),
        }
    }
}

/// Writes `entries` (in writer order) into a fresh store at `path`, then
/// verifies the file by reading every row back.
///
/// An existing file at `path` is replaced.
///
/// # Errors
///
/// Store or verification failures; the verification fails when the row
/// count or any `(key, value)` differs from `entries`.
pub fn write_with<S: WriteStore>(path: &Path, entries: &Entries) -> Result<(), DatagenError> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    {
        let store = S::create(path)?;
        store.write(|txn| {
            for (key, value) in entries {
                txn.put(TABLE, key, value)?;
            }
            Ok(())
        })?;
    }
    // Drop the writer before verifying: redb locks the file per process.

    // Verify: the store's own iteration (ascending key bytes) must contain
    // exactly the compiled entries.
    let read_only = S::open_read_only(path)?;
    let mut index = 0_usize;
    let mut mismatch: Option<String> = None;
    let mut expected = entries.clone();
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    read_only.for_each(TABLE, &mut |key, value| {
        match expected.get(index) {
            Some((want_key, want_value)) if want_key == key && want_value == value => index += 1,
            Some((want_key, _)) => {
                mismatch = Some(format!(
                    "row {index}: key {:02x?} != expected {:02x?}",
                    key, want_key
                ));
                return Err(oxpinyin_store::StoreError::Backend("mismatch".into()));
            }
            None => {
                mismatch = Some(format!("row {index}: unexpected extra key {key:02x?}"));
                return Err(oxpinyin_store::StoreError::Backend("mismatch".into()));
            }
        }
        Ok(())
    })?;
    if let Some(message) = mismatch {
        return Err(DatagenError::Consistency(format!(
            "{} verification failed: {message}",
            path.display()
        )));
    }
    if index != expected.len() {
        return Err(DatagenError::Consistency(format!(
            "{} verification failed: {} of {} rows present",
            path.display(),
            index,
            expected.len()
        )));
    }
    Ok(())
}
