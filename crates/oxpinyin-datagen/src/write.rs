//! Backend-generic table writing and backend selection.
//!
//! Every table is the frozen single-`data`-table schema
//! (`docs/findings/data-layer-export.md`) written through
//! [`WriteStore`], so the same compiled entries produce a Kyoto Cabinet
//! (the default), Tkrzw, LMDB, or redb file. After writing, the file is
//! re-opened read-only and every row is compared against the compiled
//! entries — a table is only reported written after it reads back
//! identical.

use std::fs;
use std::path::{Path, PathBuf};

use oxpinyin_store::WriteStore;

use crate::{DatagenError, Entries};

/// The single table name of the frozen schema.
pub const TABLE: &str = "data";

/// A storage backend with a native producer.
///
/// The four variants (Kyoto Cabinet, redb, LMDB, tkrzw) are peer
/// producers behind the same `WriteStore` trait — the same compiled row
/// stream reads back identically under each. [`Self::DEFAULT`] is
/// [`Self::KyotoCabinet`], matching `oxpinyin_store::DefaultStore` under
/// the workspace's default feature set; it names the selected backend,
/// not a privileged implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Backend {
    /// redb (`.redb` files). Requires the `redb` cargo feature; select
    /// it with `--no-default-features --features redb`.
    Redb,
    /// LMDB, single-file environments (`.lmdb` files). Requires the `lmdb`
    /// cargo feature.
    Lmdb,
    /// Tkrzw `TreeDBM` (`.tkt` files). Requires the `tkrzw` cargo feature.
    Tkrzw,
    /// Kyoto Cabinet TreeDB (`.kct` files). Requires the `kyotocabinet`
    /// cargo feature (on by default — Kyoto Cabinet is the workspace's
    /// default selection).
    KyotoCabinet,
}

impl Backend {
    /// The default selected backend for a normal `oxpinyin-datagen
    /// compile` run — matches `oxpinyin_store::DefaultStore` under the
    /// workspace's default feature set. Kept here as an associated
    /// constant so the binary's `Options::default()` and the workspace
    /// runtime cannot silently diverge on which peer backend the default
    /// selection is.
    pub const DEFAULT: Self = Self::KyotoCabinet;

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
            "kyotocabinet" => Ok(Self::KyotoCabinet),
            other => Err(DatagenError::Consistency(format!(
                "unknown backend {other:?} (expected redb, lmdb, tkrzw, or kyotocabinet)"
            ))),
        }
    }

    /// Whether this backend was compiled in (its cargo feature is on).
    #[must_use]
    pub const fn available(self) -> bool {
        match self {
            Self::Redb => cfg!(feature = "redb"),
            Self::Lmdb => cfg!(feature = "lmdb"),
            Self::Tkrzw => cfg!(feature = "tkrzw"),
            Self::KyotoCabinet => cfg!(feature = "kyotocabinet"),
        }
    }

    /// File extension for this backend's tables.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Redb => "redb",
            Self::Lmdb => "lmdb",
            Self::Tkrzw => "tkt",
            Self::KyotoCabinet => "kct",
        }
    }

    /// The cargo feature that compiles this backend in (differs from the
    /// file extension for Tkrzw).
    #[must_use]
    pub const fn feature(self) -> &'static str {
        match self {
            Self::Redb => "redb",
            Self::Lmdb => "lmdb",
            Self::Tkrzw => "tkrzw",
            Self::KyotoCabinet => "kyotocabinet",
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
            #[cfg(feature = "redb")]
            Self::Redb => write_with::<oxpinyin_store::RedbStore>(path, entries),
            #[cfg(feature = "lmdb")]
            Self::Lmdb => write_with::<oxpinyin_store::LmdbStore>(path, entries),
            #[cfg(feature = "tkrzw")]
            Self::Tkrzw => write_with::<oxpinyin_store::TkrzwStore>(path, entries),
            #[cfg(feature = "kyotocabinet")]
            Self::KyotoCabinet => write_with::<oxpinyin_store::KcStore>(path, entries),
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
            #[cfg(feature = "redb")]
            Self::Redb => collect::<oxpinyin_store::RedbStore>(path),
            #[cfg(feature = "kyotocabinet")]
            Self::KyotoCabinet => collect::<oxpinyin_store::KcStore>(path),
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

    /// Whether this backend emits the libpinyin drop-in schema: the KC and
    /// Tkrzw producers write `pinyin_index.bin` / `phrase_index.bin` /
    /// `bigram.db` / `punct.bin` / the per-library chunk files +
    /// `table.conf` exactly as a libpinyin install ships them
    /// (`docs/findings/datagen-compat-2026-09-01.md`). The redb and LMDB
    /// producers keep the native oxpinyin schema — no drop-in requirement
    /// exists for them.
    #[must_use]
    pub const fn emits_libpinyin_schema(self) -> bool {
        matches!(self, Self::KyotoCabinet | Self::Tkrzw)
    }

    /// Writes `entries` to `path` through this backend's raw keyspace
    /// (libpinyin-schema rows — no table-name framing).
    ///
    /// # Errors
    ///
    /// Fails if the backend was not compiled in, or on any store or
    /// verification failure.
    pub fn write_raw(self, path: &Path, entries: &Entries) -> Result<(), DatagenError> {
        match self {
            #[cfg(feature = "redb")]
            Self::Redb => crate::write::write_raw_with::<oxpinyin_store::RedbStore>(path, entries),
            #[cfg(feature = "lmdb")]
            Self::Lmdb => crate::write::write_raw_with::<oxpinyin_store::LmdbStore>(path, entries),
            #[cfg(feature = "tkrzw")]
            Self::Tkrzw => {
                crate::write::write_raw_with::<oxpinyin_store::TkrzwStore>(path, entries)
            }
            #[cfg(feature = "kyotocabinet")]
            Self::KyotoCabinet => {
                crate::write::write_raw_with::<oxpinyin_store::KcStore>(path, entries)
            }
            #[allow(unreachable_patterns)]
            backend => Err(DatagenError::Consistency(format!(
                "backend {:?} requires rebuilding this crate with --features {}",
                backend,
                backend.feature()
            ))),
        }
    }

    /// Writes `entries` to a hash store at `path` (libpinyin's `bigram.db`
    /// container class) through this backend.
    ///
    /// # Errors
    ///
    /// Fails if the backend was not compiled in, or on any store or
    /// verification failure.
    pub fn write_hash(self, path: &Path, entries: &Entries) -> Result<(), DatagenError> {
        match self {
            #[cfg(feature = "redb")]
            Self::Redb => crate::write::write_hash_with::<oxpinyin_store::RedbStore>(path, entries),
            #[cfg(feature = "lmdb")]
            Self::Lmdb => crate::write::write_hash_with::<oxpinyin_store::LmdbStore>(path, entries),
            #[cfg(feature = "tkrzw")]
            Self::Tkrzw => {
                crate::write::write_hash_with::<oxpinyin_store::TkrzwStore>(path, entries)
            }
            #[cfg(feature = "kyotocabinet")]
            Self::KyotoCabinet => {
                crate::write::write_hash_with::<oxpinyin_store::KcStore>(path, entries)
            }
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
    let read_only = S::open_read_only(path)?;
    let mut rows = Vec::new();
    read_only.for_each(TABLE, &mut |key, value| {
        rows.push((key.to_vec(), value.to_vec()));
        Ok(())
    })?;
    verify_rows(path, entries, rows)
}

/// Writes libpinyin-schema rows into a fresh store at `path` through the
/// raw (unframed) keyspace — what libpinyin's own DBMs store. KC and Tkrzw
/// write the file's bare keyspace (their `RawReadStore` reads read it back
/// unchanged); redb and LMDB delegate to the well-known raw table, the
/// same delegation the raw reads use.
///
/// An existing file at `path` is replaced.
///
/// # Errors
///
/// Store or verification failures; verification compares every raw row.
pub fn write_raw_with<S: WriteStore + oxpinyin_store::RawReadStore>(
    path: &Path,
    entries: &Entries,
) -> Result<(), DatagenError> {
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
                txn.put_raw(key, value)?;
            }
            Ok(())
        })?;
    }
    // Drop the writer before verifying: redb locks the file per process.
    // `range_raw` walks the raw keyspace in ascending key-byte order on
    // every backend (KC/Tkrzw natively; redb/LMDB through the well-known
    // raw table), which is exactly the sorted expectation.
    let read_only = S::open_read_only(path)?;
    let mut rows = Vec::new();
    read_only.range_raw(
        std::ops::Bound::Unbounded,
        std::ops::Bound::Unbounded,
        &mut |key, value| {
            rows.push((key.to_vec(), value.to_vec()));
            Ok(())
        },
    )?;
    verify_rows(path, entries, rows)
}

/// Writes libpinyin-schema rows into a fresh **hash** store at `path`
/// (libpinyin's `bigram.db` container class — KC HashDB / Tkrzw HashDBM),
/// then verifies every raw row reads back.
///
/// An existing file at `path` is replaced.
///
/// # Errors
///
/// Store or verification failures.
pub fn write_hash_with<S: WriteStore + oxpinyin_store::RawReadStore>(
    path: &Path,
    entries: &Entries,
) -> Result<(), DatagenError> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    {
        let store = S::create_hash(path)?;
        store.write(|txn| {
            for (key, value) in entries {
                txn.put_raw(key, value)?;
            }
            Ok(())
        })?;
    }
    let read_only = S::open_hash_read_only(path)?;
    for (key, value) in entries {
        let got = read_only.get_raw(key)?;
        if got.as_deref() != Some(value.as_slice()) {
            return Err(DatagenError::Consistency(format!(
                "{} verification failed: key {key:02x?} reads back {:?}",
                path.display(),
                got
            )));
        }
    }
    Ok(())
}

/// Compares the read-back rows to the sorted expectation; shared by the
/// writers above.
fn verify_rows(
    path: &Path,
    entries: &Entries,
    rows: Vec<(Vec<u8>, Vec<u8>)>,
) -> Result<(), DatagenError> {
    let mut expected = entries.clone();
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    if rows != expected {
        for (index, (got, want)) in rows.iter().zip(expected.iter()).enumerate() {
            if got != want {
                return Err(DatagenError::Consistency(format!(
                    "{} verification failed: row {index}: key {:02x?} != expected {:02x?}",
                    path.display(),
                    got.0,
                    want.0
                )));
            }
        }
        return Err(DatagenError::Consistency(format!(
            "{} verification failed: {} of {} rows present",
            path.display(),
            rows.len(),
            expected.len()
        )));
    }
    Ok(())
}
