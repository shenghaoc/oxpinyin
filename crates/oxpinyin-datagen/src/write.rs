//! Backend selection and the two container writers.
//!
//! Every DBM of a data directory is written through the store's raw
//! keyspace — the records libpinyin's own tables hold, no table-name
//! framing — into a tree container (`pinyin_index`, `phrase_index`,
//! `punct`, the addon pair) or the hash container (`bigram`), then
//! re-opened read-only and every row compared against the compiled rows:
//! a file is only reported written after it reads back identical.
//!
//! On Kyoto Cabinet and tkrzw the files carry libpinyin's own names and
//! are the drop-in set; on redb and LMDB the same records live in that
//! backend's container under `<stem>.<ext>`.

use std::fs;
use std::path::{Path, PathBuf};

use oxpinyin_store::{RawReadStore, WriteStore};

use crate::{DatagenError, Entries};

/// One of the six DBM files of a data directory (the datagen-side twin
/// of `oxpinyin_data::SystemDbm`, kept here so this crate does not pull
/// the runtime reader in).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DbmFile {
    /// `pinyin_index.bin`.
    PinyinIndex,
    /// `phrase_index.bin`.
    PhraseIndex,
    /// `bigram.db` — the hash container.
    Bigram,
    /// `punct.bin`.
    Punct,
    /// `addon_pinyin_index.bin`.
    AddonPinyinIndex,
    /// `addon_phrase_index.bin`.
    AddonPhraseIndex,
}

impl DbmFile {
    /// The base name without extension.
    #[must_use]
    pub const fn stem(self) -> &'static str {
        match self {
            Self::PinyinIndex => "pinyin_index",
            Self::PhraseIndex => "phrase_index",
            Self::Bigram => "bigram",
            Self::Punct => "punct",
            Self::AddonPinyinIndex => "addon_pinyin_index",
            Self::AddonPhraseIndex => "addon_phrase_index",
        }
    }

    /// libpinyin's name (`src/pinyin_internal.h:57-66`).
    #[must_use]
    pub const fn libpinyin_name(self) -> &'static str {
        match self {
            Self::PinyinIndex => "pinyin_index.bin",
            Self::PhraseIndex => "phrase_index.bin",
            Self::Bigram => "bigram.db",
            Self::Punct => "punct.bin",
            Self::AddonPinyinIndex => "addon_pinyin_index.bin",
            Self::AddonPhraseIndex => "addon_phrase_index.bin",
        }
    }

    /// Whether the file is the hash container.
    #[must_use]
    pub const fn is_hash(self) -> bool {
        matches!(self, Self::Bigram)
    }
}

/// A storage backend with a producer.
///
/// The four variants are peers behind the same `WriteStore`; the same
/// compiled row stream reads back identically under each. [`Self::DEFAULT`]
/// is [`Self::KyotoCabinet`], matching `oxpinyin_store::DefaultStore` under
/// the workspace's default feature set; it names the selected backend, not
/// a privileged implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Backend {
    /// redb (`.redb` files). Requires the `redb` cargo feature; select
    /// it with `--no-default-features --features redb`.
    Redb,
    /// LMDB, single-file environments (`.lmdb` files). Requires the `lmdb`
    /// cargo feature.
    Lmdb,
    /// Tkrzw — libpinyin's `--with-dbm=Tkrzw` files. Requires the `tkrzw`
    /// cargo feature.
    Tkrzw,
    /// Kyoto Cabinet — libpinyin's `--with-dbm=KyotoCabinet` files.
    /// Requires the `kyotocabinet` cargo feature (on by default — Kyoto
    /// Cabinet is the workspace's default selection).
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

    /// File extension of this backend's containers on redb and LMDB
    /// (`oxpinyin_store::DEFAULT_STORE_EXT` for the same selection); on
    /// the drop-in backends the files carry libpinyin's names instead.
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

    /// Whether libpinyin itself builds against this DBM library: its
    /// data directory is then this backend's file set, byte for byte the
    /// records and name for name the files.
    #[must_use]
    pub const fn is_libpinyin_dbm(self) -> bool {
        matches!(self, Self::KyotoCabinet | Self::Tkrzw)
    }

    /// The `database format:` token of the emitted `table.conf` — the
    /// string the corresponding libpinyin build writes
    /// (`SystemTableInfo2::load` accepts `KyotoCabinet` / `Tkrzw`); the
    /// oxpinyin-only containers name themselves, which only oxpinyin's
    /// λ reader ever sees.
    #[must_use]
    pub const fn database_format_token(self) -> &'static str {
        match self {
            Self::KyotoCabinet => "KyotoCabinet",
            Self::Tkrzw => "Tkrzw",
            Self::Redb => "redb",
            Self::Lmdb => "LMDB",
        }
    }

    /// The file name of `dbm` on this backend.
    #[must_use]
    pub fn dbm_file_name(self, dbm: DbmFile) -> String {
        if self.is_libpinyin_dbm() {
            dbm.libpinyin_name().to_owned()
        } else {
            format!("{}.{}", dbm.stem(), self.extension())
        }
    }

    /// The path of `dbm` under `out_dir` on this backend.
    #[must_use]
    pub fn dbm_path(self, out_dir: &Path, dbm: DbmFile) -> PathBuf {
        out_dir.join(self.dbm_file_name(dbm))
    }

    /// Writes `entries` as `dbm` under `out_dir`: the raw keyspace of a
    /// tree container, or the hash container for `bigram`.
    ///
    /// # Errors
    ///
    /// Fails if the backend was not compiled in, or on any store or
    /// verification failure.
    pub fn write_dbm(
        self,
        out_dir: &Path,
        dbm: DbmFile,
        entries: &Entries,
    ) -> Result<PathBuf, DatagenError> {
        let path = self.dbm_path(out_dir, dbm);
        if dbm.is_hash() {
            self.write_hash(&path, entries)?;
        } else {
            self.write_raw(&path, entries)?;
        }
        Ok(path)
    }

    /// Writes `entries` to `path` through this backend's raw keyspace
    /// (a tree container).
    ///
    /// # Errors
    ///
    /// Fails if the backend was not compiled in, or on any store or
    /// verification failure.
    pub fn write_raw(self, path: &Path, entries: &Entries) -> Result<(), DatagenError> {
        match self {
            #[cfg(feature = "redb")]
            Self::Redb => write_raw_with::<oxpinyin_store::RedbStore>(path, entries),
            #[cfg(feature = "lmdb")]
            Self::Lmdb => write_raw_with::<oxpinyin_store::LmdbStore>(path, entries),
            #[cfg(feature = "tkrzw")]
            Self::Tkrzw => write_raw_with::<oxpinyin_store::TkrzwStore>(path, entries),
            #[cfg(feature = "kyotocabinet")]
            Self::KyotoCabinet => write_raw_with::<oxpinyin_store::KcStore>(path, entries),
            #[allow(unreachable_patterns)]
            backend => Err(not_compiled(backend)),
        }
    }

    /// Writes `entries` to a hash container at `path` (libpinyin's
    /// `bigram.db` container class) through this backend.
    ///
    /// # Errors
    ///
    /// Fails if the backend was not compiled in, or on any store or
    /// verification failure.
    pub fn write_hash(self, path: &Path, entries: &Entries) -> Result<(), DatagenError> {
        match self {
            #[cfg(feature = "redb")]
            Self::Redb => write_hash_with::<oxpinyin_store::RedbStore>(path, entries),
            #[cfg(feature = "lmdb")]
            Self::Lmdb => write_hash_with::<oxpinyin_store::LmdbStore>(path, entries),
            #[cfg(feature = "tkrzw")]
            Self::Tkrzw => write_hash_with::<oxpinyin_store::TkrzwStore>(path, entries),
            #[cfg(feature = "kyotocabinet")]
            Self::KyotoCabinet => write_hash_with::<oxpinyin_store::KcStore>(path, entries),
            #[allow(unreachable_patterns)]
            backend => Err(not_compiled(backend)),
        }
    }

    /// Reads every raw `(key, value)` pair of the tree container at
    /// `path`, in ascending key-byte order.
    ///
    /// # Errors
    ///
    /// Fails if the backend was not compiled in, or on any store failure.
    pub fn read_all_raw(self, path: &Path) -> Result<Entries, DatagenError> {
        fn collect<S: RawReadStore>(path: &Path) -> Result<Entries, DatagenError> {
            let store = S::open_read_only(path)?;
            let mut rows: Entries = Vec::new();
            store.range_raw(
                std::ops::Bound::Unbounded,
                std::ops::Bound::Unbounded,
                &mut |key, value| {
                    rows.push((key.to_vec(), value.to_vec()));
                    Ok(())
                },
            )?;
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
            backend => Err(not_compiled(backend)),
        }
    }

    /// Point-reads `key` in the hash container at `path`.
    ///
    /// # Errors
    ///
    /// Fails if the backend was not compiled in, or on any store failure.
    pub fn get_hash(self, path: &Path, key: &[u8]) -> Result<Option<Vec<u8>>, DatagenError> {
        fn get<S: RawReadStore>(path: &Path, key: &[u8]) -> Result<Option<Vec<u8>>, DatagenError> {
            let store = S::open_hash_read_only(path)?;
            Ok(store.get_raw(key)?)
        }
        match self {
            #[cfg(feature = "redb")]
            Self::Redb => get::<oxpinyin_store::RedbStore>(path, key),
            #[cfg(feature = "kyotocabinet")]
            Self::KyotoCabinet => get::<oxpinyin_store::KcStore>(path, key),
            #[cfg(feature = "lmdb")]
            Self::Lmdb => get::<oxpinyin_store::LmdbStore>(path, key),
            #[cfg(feature = "tkrzw")]
            Self::Tkrzw => get::<oxpinyin_store::TkrzwStore>(path, key),
            #[allow(unreachable_patterns)]
            backend => Err(not_compiled(backend)),
        }
    }

    /// Counts the rows of the hash container at `path` — the reverse
    /// direction of [`Self::get_hash`], whose per-key point reads cannot
    /// see rows the caller did not ask about.
    ///
    /// # Errors
    ///
    /// Fails if the backend was not compiled in, or on any store failure.
    pub fn count_hash(self, path: &Path) -> Result<u64, DatagenError> {
        fn count<S: RawReadStore>(path: &Path) -> Result<u64, DatagenError> {
            let store = S::open_hash_read_only(path)?;
            Ok(store.count_raw()?)
        }
        match self {
            #[cfg(feature = "redb")]
            Self::Redb => count::<oxpinyin_store::RedbStore>(path),
            #[cfg(feature = "kyotocabinet")]
            Self::KyotoCabinet => count::<oxpinyin_store::KcStore>(path),
            #[cfg(feature = "lmdb")]
            Self::Lmdb => count::<oxpinyin_store::LmdbStore>(path),
            #[cfg(feature = "tkrzw")]
            Self::Tkrzw => count::<oxpinyin_store::TkrzwStore>(path),
            #[allow(unreachable_patterns)]
            backend => Err(not_compiled(backend)),
        }
    }
}

fn not_compiled(backend: Backend) -> DatagenError {
    DatagenError::Consistency(format!(
        "backend {:?} requires rebuilding this crate with --features {}",
        backend,
        backend.feature()
    ))
}

fn replace_file(path: &Path) -> Result<(), DatagenError> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// Writes rows into a fresh tree container at `path` through the raw
/// (unframed) keyspace — what libpinyin's own DBMs store. KC and Tkrzw
/// write the file's bare keyspace (their `RawReadStore` reads read it
/// back unchanged); redb and LMDB delegate to the well-known raw table,
/// the same delegation the raw reads use.
///
/// An existing file at `path` is replaced.
///
/// # Errors
///
/// Store or verification failures; verification compares every raw row.
pub fn write_raw_with<S: WriteStore + RawReadStore>(
    path: &Path,
    entries: &Entries,
) -> Result<(), DatagenError> {
    replace_file(path)?;
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

/// Writes rows into a fresh **hash** container at `path` (libpinyin's
/// `bigram.db` container class — KC HashDB / Tkrzw HashDBM; redb and LMDB
/// have one container class and open it either way), then verifies
/// every raw row reads back by point read (a KC HashDB cursor cannot be
/// positioned from the empty key).
///
/// An existing file at `path` is replaced.
///
/// # Errors
///
/// Store or verification failures.
pub fn write_hash_with<S: WriteStore + RawReadStore>(
    path: &Path,
    entries: &Entries,
) -> Result<(), DatagenError> {
    replace_file(path)?;
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

/// Compares the read-back rows to the sorted expectation.
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
