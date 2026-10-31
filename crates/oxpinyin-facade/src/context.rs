//! Live context state: the option/scheme word shared by a context and
//! every instance it allocated, plus the context-level open/save laws.

use std::path::Path;
use std::sync::Arc;

use oxpinyin_core::{DoublePinyinScheme, FullPinyinScheme, OptionBits, ZhuyinScheme};
use oxpinyin_engine::{Config, ConfigValue};
use oxpinyin_runtime::{
    FileType, OpenError, RowFault, Runtime, TableConfError, directory_reads_as_a_chunk,
};
use oxpinyin_user::c_stderr;
use oxpinyin_user::{ChunkHeaderField, JournalFault, UserConfLaw, UserStore, UserStoreError};

/// What `save_user` did, for the C facades' `pinyin_save` / `zhuyin_save`
/// and the `pinyin_end_add_phrases` import commit.
///
/// Upstream's save never stops at a filesystem failure and answers `true`
/// (`pinyin.cpp:1132-1147`); the one failure it *does* stop at is the
/// chunk header `assert` (`memory_chunk.h:543`/`:547`), where it dies.
/// The class (c) answer is that failure alone, so it must be told apart
/// from the quiet `false` and from the pin's `true`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveOutcome {
    /// No user directory, nothing modified, or an internal encode/compact
    /// failure the pin has no counterpart for: the deliberate quiet
    /// `false`.
    NotSaved,
    /// The file set was written and renamed: the pin's `true`.
    Saved,
    /// A chunk file's `MemoryChunk::save` header write failed; the pin
    /// `assert`s and dies of SIGABRT. The C ABI answers `false` and the
    /// facade logs exactly one warning in its own domain.
    ChunkWriteFailed(ChunkHeaderField),
    /// A loaded system library's file is a directory the pin maps again and
    /// asserts on (`memory_chunk.h:493`, `_write_files`' diff of each system
    /// library, `pinyin.cpp:956`, `zhuyin.cpp:589`); the pin dies of SIGABRT
    /// before it writes anything. The C ABI answers `false` and the facade
    /// logs exactly one warning in its own domain.
    ChunkHeaderRead,
}

impl SaveOutcome {
    /// The C ABI's answer: only [`SaveOutcome::Saved`] is `true`.
    #[must_use]
    pub fn is_saved(self) -> bool {
        matches!(self, Self::Saved)
    }
}

/// What `mask_out` did, for the C facades' `pinyin_mask_out` /
/// `zhuyin_mask_out`.
///
/// The pin's `pinyin_mask_out` calls `m_pinyin_table->mask_out` (the
/// facade over the system and user `ChewingLargeTable2`s) and ignores the
/// answer; the user table's `mask_out` walks every `user_pinyin_index.bin`
/// record and dies of SIGABRT at the `switch` whose `default` is
/// `abort()` when a key is past `MAX_PHRASE_LENGTH` syllables
/// (`chewing_large_table2_bdb.cpp:529`). The class (c) answer is that
/// failure alone, told apart from the quiet `true`/`false`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaskOutOutcome {
    /// The store's mask ran (`bool` is its own success), or there is no
    /// user store to mask (the pin's `false`).
    Done(bool),
    /// The user pinyin index carried a key past `MAX_PHRASE_LENGTH`
    /// syllables; the C ABI answers `false` and the facade logs one
    /// warning in its own domain.
    OverlongIndexKey,
    /// The user bigram carried a key that is not a four-byte
    /// `phrase_token_t`; the pin's `get_all_items` asserts
    /// (`ngram_bdb.cpp:199`). The C ABI answers `false` and logs one
    /// warning.
    NonTokenUserBigramKey,
    /// The user bigram carried a value shorter than a `guint32`
    /// `total_freq`, or a partial eight-byte item tail. The pin's
    /// `get_total_freq` asserts on the former (`memory_chunk.h:390`);
    /// `SingleGram::mask_out` reads past the latter (`ngram.cpp:76-86`).
    /// The C ABI answers `false` and logs one warning.
    ShortUserBigramValue,
    /// Masking the user bigram removed every item of a gram whose total
    /// its items did not cover, leaving a residual `total_freq`; the pin's
    /// `get_length` asserts (`ngram.cpp:70`, from `Bigram::mask_out`,
    /// `ngram_bdb.cpp:243`). The C ABI answers `false` and logs one
    /// warning.
    ResidualUserBigramGram,
    /// A loaded system library's file is a directory the pin reloads and
    /// asserts on (`memory_chunk.h:493`, `pinyin.cpp:1265`,
    /// `zhuyin.cpp:800`). The C ABI answers `false` and logs one warning.
    ChunkHeaderRead,
    /// A loaded system library's user `.dbin`, reloaded through
    /// `merge_with_mask` after the user tables were masked, reached a pin
    /// assertion ([`JournalFault`], rows 91, 92, 97 and 108). The pin dies
    /// there; the C adapters abort.
    Journal(JournalFault),
}

impl MaskOutOutcome {
    /// The C ABI's answer: [`MaskOutOutcome::Done`] carries the store's
    /// own answer; the abort site is `false`.
    #[must_use]
    pub fn is_done(self) -> bool {
        matches!(self, Self::Done(true))
    }
}

/// Why a context did not open — what `pinyin_init` / `zhuyin_init` hide
/// behind NULL. Carried out of [`ContextCore::try_open`] so the facades
/// can log it; the return value stays NULL either way.
#[derive(Debug)]
pub enum OpenFailure {
    /// Legacy empty-path failure, retained for source compatibility.
    /// Current opens resolve an empty system path against the working directory.
    EmptySystemDir,
    /// The runtime could not open the system directory.
    Runtime(OpenError),
}

impl core::fmt::Display for OpenFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptySystemDir => f.write_str("system directory is empty"),
            Self::Runtime(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for OpenFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::EmptySystemDir => None,
            Self::Runtime(error) => Some(error),
        }
    }
}

impl OpenFailure {
    /// Whether this failure is the user marker's unknown format: a
    /// `user.conf` whose `database format:` field upstream's
    /// `to_table_database_format_type` does not know, and therefore
    /// `abort()`s on (`table_info.cpp:122-133`, reached from `:353-354`,
    /// register row 44).
    ///
    /// The C facades abort the process for it; nothing was cleaned and no
    /// marker was written, which is also where upstream stops.
    #[must_use]
    pub fn unknown_database_format(&self) -> bool {
        matches!(
            self,
            Self::Runtime(oxpinyin_runtime::OpenError::UnknownDatabaseFormat(_))
        )
    }

    /// Whether a system library is a directory the pin's
    /// `MemoryChunk::mmap` asserts on while it loads at init
    /// (`memory_chunk.h:493`). The C facades answer NULL with one warning.
    #[must_use]
    pub fn chunk_header_read(&self) -> bool {
        matches!(self, Self::Runtime(error) if error.chunk_header_read())
    }

    /// The system `table.conf` failure behind this open, if that is why it
    /// failed: [`TableConfError::Header`] is the pin's ordinary `false`
    /// return (the facade writes `load %s failed!`, no warning); every
    /// other kind is a site the pin dies on, answered with one warning.
    #[must_use]
    pub fn table_conf_error(&self) -> Option<TableConfError> {
        match self {
            Self::Runtime(oxpinyin_runtime::OpenError::TableConf(error)) => Some(*error),
            Self::EmptySystemDir | Self::Runtime(_) => None,
        }
    }

    /// The pin assertion a default library's `.dbin` merge reached at init
    /// ([`JournalFault`], rows 91 and 108), if that is why the open stopped.
    /// The C facades abort on it.
    #[must_use]
    pub fn journal_fault(&self) -> Option<JournalFault> {
        match self {
            Self::Runtime(oxpinyin_runtime::OpenError::Journal(fault)) => Some(*fault),
            Self::EmptySystemDir | Self::Runtime(_) => None,
        }
    }
}

/// The pin `assert`s on the type of the `table.conf` row a library call
/// names: `pinyin.cpp:457` / `zhuyin.cpp:372` (a default library that is
/// neither `SYSTEM_FILE` nor `USER_FILE`), `pinyin.cpp:491` (an addon row that
/// is neither `DICTIONARY` nor `NOT_USED`), or `MemoryChunk` reading a
/// directory because the row names no file (`memory_chunk.h:493`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LibraryRowAssert {
    /// `SYSTEM_FILE == m_file_type || USER_FILE == m_file_type` failed.
    NotLoadable,
    /// `DICTIONARY == m_file_type` failed.
    NotDictionary,
    /// `ret_len == sizeof(length)` failed in `MemoryChunk::mmap`.
    NullChunkName,
    /// The same assert, for a library file that is a directory the pin maps
    /// again at this call (`memory_chunk.h:493`).
    ChunkHeaderRead,
}

/// Why [`ContextCore::load_phrase_library`] did not load.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LibraryLoadFault {
    /// The pin `assert`s on the library's row.
    Row(LibraryRowAssert),
    /// The reloaded system library's `.dbin` merge reached a pin assertion
    /// ([`JournalFault`], rows 91 and 108; `pinyin.cpp:276`,
    /// `zhuyin.cpp:219`). The library stays unloaded; the C adapters abort.
    Journal(JournalFault),
}

/// Whether the pin writes its `mmap %s failed!` line for a row that names no
/// chunk. `g_build_filename(dir, NULL, NULL)` is `dir`, so `MemoryChunk::mmap`
/// opens the directory itself (`pinyin.cpp:252-256`, `:286-290`,
/// `zhuyin.cpp:196-200` at 074a2219). Where the directory reads as a chunk,
/// the `read` of its length word fails and the pin asserts
/// (`memory_chunk.h:493`) before writing anything. Where `lseek(SEEK_END)`
/// fails or answers under the header (tmpfs), `mmap` refuses it
/// (`memory_chunk.h:486-489`), the pin writes `mmap %s failed!` naming `dir`
/// and then dies of SIGSEGV on the empty chunk (`phrase_index.cpp:354`). The
/// answer and warning stay the abort site's either way. Unix only, as
/// `phrase_library::directory_chunk_fault` is.
fn writes_null_chunk_line(dir: &Path, reads_as_a_chunk: impl FnOnce(&Path) -> bool) -> bool {
    cfg!(unix) && !reads_as_a_chunk(dir)
}

/// Whether the pin's `MemoryChunk::mmap` asserts (`memory_chunk.h:493`) on
/// `dir` read as the chunk of a row that names none — the complement of
/// [`writes_null_chunk_line`]: where it does not assert, it writes
/// `mmap %s failed!` and dies of SIGSEGV on the empty chunk instead (class
/// (b)). The C facades abort the process on the assert shape.
#[must_use]
pub fn null_chunk_asserts(dir: &Path) -> bool {
    !writes_null_chunk_line(dir, directory_reads_as_a_chunk)
}

/// Writes the pin's `mmap %s failed!` line for `dir` with
/// [`c_stderr::mmap_failed`], if [`writes_null_chunk_line`] says the pin
/// writes it on this file system.
fn null_chunk_refused(dir: &Path) {
    if writes_null_chunk_line(dir, directory_reads_as_a_chunk) {
        c_stderr::mmap_failed(dir);
    }
}

/// Bit 30 of [`LiveOptions::double_scheme`]: when set, the live
/// scheme's fallback table is suppressed — upstream's half-mutation
/// (`pinyin_parser2.cpp:580` + `:614`; `upstream-divergences.md`
/// row 5b).  A valid-scheme store clears it implicitly (values 1–6
/// never have bit 30), and the out-of-enum arm sets it with
/// `fetch_or`.
pub const FALLBACK_CLEARED_BIT: i32 = 1 << 30;

/// The live option/scheme state a context owns and every instance it
/// allocates shares: `set_options`/`set_*_scheme` on the context remask
/// already-allocated instances through these handles.
///
/// Both C-ABI facades carry the same seven fields; the seed word differs
/// per facade and is the caller's choice at [`ContextCore::open`].
#[derive(Clone)]
pub struct LiveOptions {
    /// Live `PINYIN_INCOMPLETE` bit.
    pub incomplete: Arc<std::sync::atomic::AtomicBool>,
    /// Live double-pinyin scheme (header discriminant value, bits 0–29)
    /// with the fallback-cleared flag packed into bit 30
    /// ([`FALLBACK_CLEARED_BIT`]).
    pub double_scheme: Arc<std::sync::atomic::AtomicI32>,
    /// Live Zhuyin scheme (header discriminant value).
    pub zhuyin_scheme: Arc<std::sync::atomic::AtomicI32>,
    /// Live full-pinyin scheme (header discriminant value).
    pub full_scheme: Arc<std::sync::atomic::AtomicI32>,
    /// Live `USE_TONE` bit.
    pub use_tone: Arc<std::sync::atomic::AtomicBool>,
    /// Live `FORCE_TONE` bit (nested under `USE_TONE` by the zhuyin
    /// parser; written by both facades' `set_options`, read by neither —
    /// the parsers take it off the option word itself).
    pub force_tone: Arc<std::sync::atomic::AtomicBool>,
    /// Live option word.
    pub options: Arc<std::sync::atomic::AtomicU32>,
}

impl LiveOptions {
    /// Seeds the live state the way an init does: the bools derive off
    /// `option_word` (so `pinyin_init`'s `USE_TONE` seeds incomplete OFF,
    /// `zhuyin_init`'s `USE_TONE | FORCE_TONE` seeds both tone bits ON), and the three schemes start at the header defaults
    /// both facades share (MS double, Standard zhuyin, Hanyu full).
    #[must_use]
    pub(crate) fn new(option_word: u32) -> Self {
        let bits = OptionBits::from_bits(option_word);
        Self {
            incomplete: Arc::new(std::sync::atomic::AtomicBool::new(
                bits.contains(oxpinyin_core::PINYIN_INCOMPLETE),
            )),
            double_scheme: Arc::new(std::sync::atomic::AtomicI32::new(
                DoublePinyinScheme::Ms as i32,
            )),
            zhuyin_scheme: Arc::new(std::sync::atomic::AtomicI32::new(
                ZhuyinScheme::Standard as i32,
            )),
            full_scheme: Arc::new(std::sync::atomic::AtomicI32::new(
                FullPinyinScheme::Hanyu as i32,
            )),
            use_tone: Arc::new(std::sync::atomic::AtomicBool::new(
                bits.contains(oxpinyin_core::USE_TONE),
            )),
            force_tone: Arc::new(std::sync::atomic::AtomicBool::new(
                bits.contains(oxpinyin_core::FORCE_TONE),
            )),
            options: Arc::new(std::sync::atomic::AtomicU32::new(option_word)),
        }
    }
}

/// State behind a facade's context handle, minus the C parts: the shared
/// assembly, the user-learning store, the layered configuration, and the
/// live option/scheme word every allocated instance shares.
///
/// Dropping it is the facade's fini: the user store it opened makes the
/// fini-time `user.conf` write of the law it was opened under —
/// libpinyin lowers the open counter its init raised, libzhuyin writes
/// nothing (`oxpinyin_user::persistence::fini`). A context that is never
/// dropped, like a process that dies before its `pinyin_fini`, leaves the
/// raised counter on disk.
pub struct ContextCore {
    /// The layered configuration instances are opened with (the pinned
    /// upstream defaults).
    pub config: Config,
    /// The shared concrete assembly; `None` under a user-store-only
    /// context.
    pub runtime: Option<Runtime>,
    /// The user-learning store, shared by value-clone with every
    /// instance. `None` when the caller passed no usable user directory —
    /// an unusable dir must not make init fail; training degrades to
    /// refusing, upstream-style.
    pub user: Option<UserStore>,
    /// The live option/scheme word, shared with every instance.
    pub live: LiveOptions,
}

impl ContextCore {
    /// Opens a context the way an init does: system tables plus the
    /// optional user dir, health-checked, with `option_word` as the
    /// seeding word (per-facade: `PINYIN_DEFAULT_OPTION_WORD` /
    /// `ZHUYIN_DEFAULT_OPTION_WORD`) and `law` as the user dir's
    /// `user.conf` lifecycle (per-facade: [`UserConfLaw::Pinyin`] /
    /// [`UserConfLaw::Zhuyin`]).
    ///
    /// Both directories are paths, not text: the pin keeps `g_strdup` of the
    /// caller's bytes (`pinyin.cpp:332`, `zhuyin.cpp:276`) and opens them
    /// as they are, so a directory whose name is not UTF-8 opens too.
    ///
    /// `user_dir` is the init's own argument: `None` is a NULL user dir —
    /// transient user state — while `Some("")` is a user dir, the working
    /// directory, as upstream's `g_strdup("")` is (#619). The two are
    /// different inputs.
    ///
    /// An empty system path resolves files in the working directory.
    /// `None` means the runtime cannot open. [`Self::try_open`] says why.
    #[must_use]
    pub fn open(
        system_dir: &Path,
        user_dir: Option<&Path>,
        option_word: u32,
        law: UserConfLaw,
    ) -> Option<Self> {
        Self::try_open(system_dir, user_dir, option_word, law).ok()
    }

    /// [`Self::open`] with the failure kept: the C facades log it before
    /// answering NULL, so a consumer can tell a missing directory from a
    /// corrupt table without string-matching the logged message.
    ///
    /// # Errors
    ///
    /// [`OpenFailure::Runtime`] with the typed [`OpenError`].
    pub fn try_open(
        system_dir: &Path,
        user_dir: Option<&Path>,
        option_word: u32,
        law: UserConfLaw,
    ) -> Result<Self, OpenFailure> {
        let runtime = Runtime::open_with_law(system_dir, user_dir, law).map_err(|error| {
            // A system library the pin cannot map: its line, then the
            // pin dies on the empty chunk (`pinyin.cpp:256`,
            // `zhuyin.cpp:200`); this open answers NULL as it did. The
            // line follows `check_format`'s, so it is the shim's too.
            if let Some(path) = error.unmappable_library() {
                c_stderr::mmap_failed(path);
            } else if error.null_system_chunk() {
                null_chunk_refused(system_dir);
            }
            OpenFailure::Runtime(error)
        })?;
        // The libraries whose file is absent load as unloaded and the open
        // goes on; the pin's lines for them follow the user profile's
        // (`check_format` runs before the library loop), on its stream.
        for path in runtime.unmapped_system_libraries() {
            c_stderr::mmap_failed(path);
        }
        let user = runtime.user_store();
        Ok(Self {
            config: Config::default(),
            runtime: Some(runtime),
            user,
            live: LiveOptions::new(option_word),
        })
    }

    /// `set_options`'s law: the word is stored, the bools it carries are
    /// mirrored into their live flags, and the `incomplete-pinyin`
    /// configuration key follows the word so sessions opened later agree
    /// with sessions already allocated.
    pub fn set_options(&mut self, options: u32) {
        let enabled = (options & oxpinyin_core::PINYIN_INCOMPLETE) != 0;
        let use_tone = (options & oxpinyin_core::USE_TONE) != 0;
        let force_tone = (options & oxpinyin_core::FORCE_TONE) != 0;
        self.config
            .set("incomplete-pinyin", ConfigValue::Bool(enabled));
        let live = &self.live;
        live.incomplete
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
        live.use_tone
            .store(use_tone, std::sync::atomic::Ordering::Relaxed);
        live.force_tone
            .store(force_tone, std::sync::atomic::Ordering::Relaxed);
        live.options
            .store(options, std::sync::atomic::Ordering::Relaxed);
    }

    /// Allocates one instance's orchestration state over this context's
    /// assembly. `None` without a runtime (nothing to decode with).
    #[must_use]
    pub fn alloc_instance(&self) -> Option<crate::instance::InstanceCore> {
        let runtime = self.runtime.as_ref()?;
        let session = runtime.new_session(&self.config).ok()?;
        Some(crate::instance::InstanceCore::new(
            session,
            self.user.clone(),
            runtime.dict(),
            runtime.lm(),
            self.live.clone(),
        ))
    }

    /// `save`'s body: [`SaveOutcome::NotSaved`] without a user dir or when
    /// unmodified, [`SaveOutcome::Saved`] after a dirty save, and
    /// [`SaveOutcome::ChunkWriteFailed`] at the one write the pin dies on.
    ///
    /// The pin never stops at a failing write or rename: it prints what
    /// failed (`rename %s to %s failed.`, `write %s failed.`, raw
    /// `fprintf(stderr, …)`, `pinyin.cpp:1061`…`:1123`,
    /// `table_info.cpp:382`) and answers `true`. The store's
    /// [`oxpinyin_user::SaveReport`] says what failed; the lines are
    /// printed here, one per failure, in the pin's order.
    ///
    /// A chunk header write is the exception: `MemoryChunk::save`'s
    /// `assert`s (`memory_chunk.h:543`, `:547`) kill the process there, so
    /// the store's `save_reporting` stops instead of reporting, and this
    /// surface must fail the call. The caller logs the point.
    ///
    /// A system library whose layout `SubPhraseIndex::load` refuses again at
    /// the save's remap is diffed against the pin's empty sub-index; the
    /// pin's `g_return_val_if_fail` critical there is not reproduced
    /// (2026-10-31 UTC ruling, `docs/findings/1297-layout-criticals.md`).
    pub fn save_user(&mut self) -> SaveOutcome {
        let Some(store) = self.user.as_ref() else {
            return SaveOutcome::NotSaved;
        };
        if !store.has_user_directory() || !store.is_modified() {
            return SaveOutcome::NotSaved;
        }
        // `pinyin_save`/`zhuyin_save` rebuild every sub-index from its
        // items before `_write_files` (`pinyin.cpp:1139`,
        // `zhuyin.cpp:554`); the failed empty sub-index's local total
        // becomes zero, while the facade aggregate already excludes it.
        let compacted = self.compact_layout_failed();
        // `_write_files` maps every loaded system library again to diff it
        // against the live one (`pinyin.cpp:956`, `zhuyin.cpp:589`), before
        // it writes anything. A library that is a directory the pin asserts
        // on kills it there (`memory_chunk.h:493`). The lines go on the
        // stream the save's rename and marker lines use.
        if self.check_system_libraries(None) {
            return SaveOutcome::ChunkHeaderRead;
        }
        let runtime = self.runtime.as_ref();
        let Some(store) = self.user.as_mut() else {
            return SaveOutcome::NotSaved;
        };
        let baseline = |nibble: u8| -> oxpinyin_user::SaveBaseline {
            let Some(runtime) = runtime else {
                return oxpinyin_user::SaveBaseline::Keep;
            };
            match runtime.save_disk_baseline(nibble) {
                Ok(baseline) => baseline,
                Err(fault) => oxpinyin_user::SaveBaseline::Changed(
                    oxpinyin_user::SystemLibrary::failed(fault.total_freq),
                ),
            }
        };
        let report = match store.save_reporting_with_baseline(&compacted, baseline) {
            Ok(report) => report,
            Err(UserStoreError::ChunkHeaderWrite(field)) => {
                return SaveOutcome::ChunkWriteFailed(field);
            }
            Err(_) => return SaveOutcome::NotSaved,
        };
        // Each failed rename's `rename %s to %s failed.` line and a failed
        // marker's `write %s failed.` line were written as they happened,
        // in the pin's order, by the save itself.
        let _ = report;
        SaveOutcome::Saved
    }

    /// The system libraries (with `only`, that one) whose file the pin would
    /// map again at this call and cannot: one `mmap %s failed!` each, in
    /// library order ([`c_stderr::mmap_failed`]). The pin dies on the first;
    /// nothing here does, and the call answers what it answered before.
    fn check_system_libraries(&self, only: Option<u8>) -> bool {
        let Some(runtime) = &self.runtime else {
            return false;
        };
        let checked = runtime.check_system_libraries(only);
        if checked.chunk_header_fault {
            return true;
        }
        for path in checked.unmappable {
            c_stderr::mmap_failed(&path);
        }
        false
    }

    /// `FacadePhraseIndex::compact` over the system layout-failed empty
    /// sub-indices. The pin calls it before a save (`pinyin.cpp:1139`,
    /// `zhuyin.cpp:554`), at `begin_add_phrases` (`pinyin.cpp:657`,
    /// `zhuyin.cpp:542`) and at the end of `mask_out` (`pinyin.cpp:1294`,
    /// `zhuyin.cpp:829`). Returns the compacted nibbles.
    #[must_use]
    pub fn compact_layout_failed(&self) -> Vec<u8> {
        self.runtime
            .as_ref()
            .map(oxpinyin_runtime::Runtime::compact_layout_failed)
            .unwrap_or_default()
    }

    /// `mask_out`'s body: the store-level deletion, or
    /// [`MaskOutOutcome::Done(false)`](MaskOutOutcome::Done) without a user
    /// store. The pin masks its tables, maps every loaded system library
    /// again and merges the filtered disk log (`pinyin.cpp:1253-1285`,
    /// `zhuyin.cpp:788-820` at 074a2219).
    ///
    /// A user pinyin index key past `MAX_PHRASE_LENGTH` syllables sends the
    /// pin's user `ChewingLargeTable2::mask_out` into `switch`'s
    /// `default: abort()` (`chewing_large_table2_bdb.cpp:529`); the store
    /// reports it as [`UserStoreError::OverlongIndexKey`] and this answers
    /// [`MaskOutOutcome::OverlongIndexKey`] so the facade can log it.
    ///
    /// Then, in the pin's order, each loaded `SYSTEM_FILE`/`DICTIONARY`
    /// library is mapped again and takes its user `.dbin` through
    /// `merge_with_mask` ([`UserStore::reload_system_libraries`]); the first
    /// pin assertion a log reaches answers [`MaskOutOutcome::Journal`], after
    /// the user tables were masked and the libraries before it reloaded.
    pub fn mask_out(&mut self, mask: u32, value: u32) -> MaskOutOutcome {
        if self.check_system_libraries(None) {
            return MaskOutOutcome::ChunkHeaderRead;
        }
        let Some(store) = self.user.as_mut() else {
            return MaskOutOutcome::Done(false);
        };
        // The user tables first (`pinyin.cpp:1228-1230`,
        // `zhuyin.cpp:763-765`).
        match store.mask_out(mask, value) {
            Ok(()) => {}
            Err(UserStoreError::OverlongIndexKey) => return MaskOutOutcome::OverlongIndexKey,
            Err(UserStoreError::NonTokenUserBigramKey) => {
                return MaskOutOutcome::NonTokenUserBigramKey;
            }
            Err(UserStoreError::ShortUserBigramValue) => {
                return MaskOutOutcome::ShortUserBigramValue;
            }
            Err(UserStoreError::ResidualUserBigramGram) => {
                return MaskOutOutcome::ResidualUserBigramGram;
            }
            Err(_) => return MaskOutOutcome::Done(false),
        }
        // Then the phrase index loop: each loaded system sub-index is
        // replaced before its disk log is replayed with the mask; the loop
        // skips a sub-index `get_range` reports missing (074a2219
        // pinyin.cpp:1234-1291, zhuyin.cpp:769-826), which the runtime's
        // remap leaves out. The separate phrase table is retained.
        if let Some(runtime) = self.runtime.as_ref() {
            let originals = runtime.reload_system_libraries();
            if let Err(fault) = store.reload_system_libraries(originals, mask, value) {
                return MaskOutOutcome::Journal(fault);
            }
            // The mask ends with a compact (`pinyin.cpp:1294`,
            // `zhuyin.cpp:829`); the failed empty sub-index's local
            // total becomes zero.
            let _compacted = runtime.compact_layout_failed();
        }
        store.compact_libraries();
        MaskOutOutcome::Done(true)
    }

    /// Whether `user_bigram.db` carried a key that is not a four-byte
    /// `phrase_token_t` ([`UserStoreError::NonTokenUserBigramKey`]). The
    /// pin's `Bigram::get_all_items` assert dies (`ngram_bdb.cpp:199`),
    /// reached by `pinyin_begin_get_bigram_phrases`.
    #[must_use]
    pub fn has_non_token_bigram_key(&self) -> bool {
        self.user
            .as_ref()
            .is_some_and(UserStore::has_non_token_bigram_key)
    }

    /// `pinyin_load_phrase_library`'s body (074a2219 `pinyin.cpp:446-459`,
    /// `_load_phrase_library` `:234-322`; `zhuyin.cpp:361-375`, `:178-266`):
    /// an in-memory transition of the session, as the pin's. A library that
    /// is loaded answers `false` (`get_range` found its sub-index,
    /// `:242-244`). An unloaded system library is mapped again and only its
    /// saved `.dbin` is merged, an unreadable one being an empty log; an
    /// unloaded `USER_FILE` library comes back from its saved chunk, or empty
    /// (`:303-321`). The runtime's visibility mask then clears and the call
    /// answers `true`. Nothing is written to disk, and nothing that can fail
    /// is: the pin has no failing site here (policy row 103). `false`
    /// without a runtime.
    ///
    /// # Errors
    ///
    /// The pin's asserts, nothing else: [`LibraryRowAssert::NotLoadable`]
    /// when the index is in range and its default row is neither
    /// `SYSTEM_FILE` nor `USER_FILE` (`pinyin.cpp:457`, `zhuyin.cpp:372`),
    /// or [`LibraryRowAssert::ChunkHeaderRead`] when the library being
    /// loaded again is a directory (`memory_chunk.h:493`), both under
    /// [`LibraryLoadFault::Row`]; or [`LibraryLoadFault::Journal`] when the
    /// saved `.dbin`'s merge reaches an assert, with the library still
    /// unloaded.
    pub fn load_phrase_library(&self, index: u32) -> Result<bool, LibraryLoadFault> {
        let Some(runtime) = self.runtime.as_ref() else {
            return Ok(false);
        };
        let Ok(row_index) = u8::try_from(index) else {
            return Ok(false);
        };
        let Some(row) = runtime.table_conf().default_row(row_index) else {
            // Out of the sixteen sub-indices: the pin's first guard.
            return Ok(false);
        };
        if !matches!(row.file_type, FileType::SystemFile | FileType::UserFile) {
            return Err(LibraryLoadFault::Row(LibraryRowAssert::NotLoadable));
        }
        // A reload maps the file again (`pinyin.cpp:256`); a library that is
        // a directory the pin asserts on kills it there
        // (`memory_chunk.h:493`), before anything changes. A library that is
        // still loaded is not mapped again.
        if runtime.dict().library_visible(index) {
            return Ok(false);
        }
        if self.check_system_libraries(Some(row_index)) {
            return Err(LibraryLoadFault::Row(LibraryRowAssert::ChunkHeaderRead));
        }
        // The sub-index `unload` freed comes back as the system chunk plus
        // the saved log, so the session's unsaved changes to the library
        // are gone (`pinyin.cpp:264-277`, `phrase_index.cpp:237-248`;
        // `zhuyin.cpp:208-221`). The user index tables keep their entries.
        // The session takes the replacement before the mask clears, so no
        // reader sees the freed state again. The system chunk mapped below
        // sits behind the mask and is mapped again by the next load.
        //
        // A `USER_FILE` library comes back from its saved chunk, or empty
        // when there is none (`pinyin.cpp:303-321`, `zhuyin.cpp:247-265`);
        // a token the dropped session had allocated dangles as in policy
        // row 102.
        //
        // The saved `.dbin`'s merge can reach a pin assertion (rows 91 and
        // 108, `pinyin.cpp:276`, `zhuyin.cpp:219`): that is returned with
        // nothing replaced and the library still unloaded.
        let user_file = matches!(row.file_type, FileType::UserFile);
        let originals = runtime.reload_system_library(row_index);
        if let Some(mut store) = self.user.clone() {
            if user_file {
                store.load_user_library(row_index);
            } else if !originals.is_empty() {
                store
                    .load_system_libraries(originals)
                    .map_err(LibraryLoadFault::Journal)?;
            }
        }
        Ok(runtime.load_library(index))
    }

    /// `load_addon_phrase_library`'s body: the runtime's addon load, with
    /// the `mmap %s failed!` line the pin writes when the library file does
    /// not map (`pinyin.cpp:290`); the answer is the runtime's, `false` for
    /// a failure. `false` without a runtime.
    ///
    /// # Errors
    ///
    /// The [`LibraryRowAssert`] the pin dies on: the addon row is neither
    /// `DICTIONARY` nor `NOT_USED` (`pinyin.cpp:491`), or names no chunk.
    pub fn load_addon_phrase_library(&self, index: u8) -> Result<bool, LibraryRowAssert> {
        let Some(runtime) = self.runtime.as_ref() else {
            return Ok(false);
        };
        match runtime.load_system_addon_reporting(index) {
            Ok(loaded) => Ok(loaded),
            Err(oxpinyin_runtime::LibraryError::Row(fault)) => Err(match fault {
                RowFault::NotDictionary => LibraryRowAssert::NotDictionary,
                RowFault::NullChunkName => {
                    if let Some(dir) = runtime.system_dir() {
                        null_chunk_refused(dir);
                    }
                    LibraryRowAssert::NullChunkName
                }
                RowFault::ChunkHeaderRead => LibraryRowAssert::ChunkHeaderRead,
            }),
            Err(error) => {
                if let Some(path) = error.unmappable_path() {
                    c_stderr::mmap_failed(path);
                }
                Ok(false)
            }
        }
    }

    /// `FacadePhraseIndex::unload` behind `pinyin_unload_phrase_library`
    /// and `zhuyin_unload_phrase_library` (074a2219 `pinyin.cpp:464-475`,
    /// `zhuyin.cpp:379-390`, `phrase_index.cpp:261-268`): an in-memory
    /// transition of the session, as the pin's. The library's sub-index goes
    /// with everything the session changed in it and its share of the
    /// facade total, the runtime's visibility mask sets, and the call
    /// answers the runtime's `true`. Nothing is written to disk: a save
    /// skips the library while it is unloaded (`pinyin.cpp:928-933`), and
    /// the pin has no failing site here (policy row 103). `false` without a
    /// runtime. Which indices a facade lets through is the C adapters'
    /// rule.
    #[must_use]
    pub fn unload_phrase_library(&self, index: u8) -> bool {
        let Some(runtime) = self.runtime.as_ref() else {
            return false;
        };
        // `unload` deletes the sub-index and takes its total out of the
        // facade (`phrase_index.cpp:261-268`): the session's changes to the
        // library and its saved log's share go now, not at the next load.
        // A `USER_FILE` library is dropped whole and comes back from the
        // saved chunk (`pinyin.cpp:303-321`).
        let user_file = runtime
            .table_conf()
            .default_row(index)
            .is_some_and(|row| matches!(row.file_type, FileType::UserFile))
            && runtime.dict().library_visible(u32::from(index));
        let originals = runtime.current_system_library(index);
        if let Some(mut store) = self.user.clone() {
            if user_file {
                store.unload_user_library(index);
            } else if !originals.is_empty() {
                store.unload_system_libraries(originals);
            }
        }
        runtime.unload_library(u32::from(index))
    }

    /// Clone of the context's user store, if this context has one.
    #[must_use]
    pub fn user_store(&self) -> Option<UserStore> {
        self.user.clone()
    }
}

#[cfg(test)]
mod null_chunk_tests {
    use super::writes_null_chunk_line;
    use std::path::Path;

    #[cfg(unix)]
    #[test]
    fn a_directory_that_does_not_read_as_a_chunk_gets_the_pin_line_once() {
        let mut asked = 0;
        let writes = writes_null_chunk_line(Path::new("/scratch/sys-1"), |dir| {
            asked += 1;
            assert_eq!(dir, Path::new("/scratch/sys-1"));
            false
        });
        assert_eq!(asked, 1);
        assert!(writes);
    }

    #[test]
    fn a_directory_that_reads_as_a_chunk_gets_no_line() {
        assert!(!writes_null_chunk_line(Path::new("/scratch/sys-1"), |_| {
            true
        }));
    }
}

#[cfg(test)]
mod open_failure_tests {

    use super::{ContextCore, OpenFailure};
    use crate::PINYIN_DEFAULT_OPTION_WORD as WORD;
    use oxpinyin_runtime::OpenError;
    use oxpinyin_user::UserConfLaw;

    #[test]
    fn missing_system_dir_carries_the_runtime_error() {
        // A child of a fresh dir this test owns: absent by construction, and
        // nothing another process does to a shared temp path can create it.
        let root = tempfile::Builder::new()
            .prefix("oxpinyin-facade-missing-")
            .tempdir()
            .expect("temp dir");
        let dir = root.path().join("system");
        let failure = ContextCore::try_open(&dir, None, WORD, UserConfLaw::Pinyin)
            .err()
            .expect("a missing system dir cannot open");
        let OpenFailure::Runtime(error) = &failure else {
            panic!("expected a runtime failure, got {failure:?}");
        };
        // `SystemTableInfo2::load` is the first step (maintainer ruling
        // 2026-10-28 UTC, D3); its `open %s failed.` line and the init's
        // `load %s failed!` name the file.
        assert!(
            matches!(
                error,
                OpenError::TableConf(oxpinyin_runtime::TableConfError::Unopenable)
            ),
            "got {error:?}"
        );
        assert!(std::error::Error::source(&failure).is_some());
    }
}

/// Policy row 103: `FacadePhraseIndex::unload` and `_load_phrase_library`
/// are in-memory transitions that cannot fail (074a2219
/// `phrase_index.cpp:261-268`, `pinyin.cpp:234-322`), and a save skips an
/// unloaded library (`pinyin.cpp:928-933`).
#[cfg(test)]
mod library_transition_tests {
    use super::{ContextCore, SaveOutcome};
    use crate::PINYIN_DEFAULT_OPTION_WORD as WORD;
    use oxpinyin_core::{PhraseText, SyllableKey};
    use oxpinyin_user::{UserConfLaw, toned_key};
    use std::path::Path;

    const GB: u8 = 1;
    const GBK: u8 = 2;

    /// A context over the committed mini tables of this build's backend,
    /// with a user directory of its own.
    fn open(user: &Path) -> ContextCore {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/w3");
        ["db", "kct", "tkt"]
            .into_iter()
            .find_map(|ext| {
                ContextCore::open(&root.join(ext), Some(user), WORD, UserConfLaw::Pinyin)
            })
            .expect("committed backend fixtures")
    }

    /// The first token of `library` and its text.
    fn first_item(ctx: &ContextCore, library: u8) -> (u32, PhraseText) {
        let token = (u32::from(library) << 24) | 1;
        let dict = ctx.runtime.as_ref().expect("runtime").dict();
        let text = dict.system().phrase_text(token).expect("text");
        (token, PhraseText::from(text.as_str().expect("scalar text")))
    }

    /// Imports `token`'s text under a reading of `tone` the chunk does not
    /// hold, and arms `m_modified` as `pinyin_end_add_phrases` does
    /// (`pinyin.cpp:658`).
    fn import(ctx: &mut ContextCore, library: u8, token: u32, text: &PhraseText, tone: u8) {
        let syllable = SyllableKey::from_text("a").expect("syllable").index();
        let length = text.char_count().expect("character count");
        let keys = vec![toned_key(syllable, tone).expect("key"); length];
        ctx.user
            .as_mut()
            .expect("store")
            .add_system_phrase_in(library, Some(token), text, &keys, Some(7))
            .expect("import");
        ctx.user.as_mut().expect("store").mark_modified();
    }

    /// The readings the session holds for `token` beyond the chunk's own.
    fn readings(ctx: &ContextCore, token: u32) -> usize {
        ctx.user
            .as_ref()
            .expect("store")
            .system_item_override(token)
            .expect("item")
            .map_or(0, |item| item.prons.len())
    }

    #[test]
    fn unload_then_save_leaves_the_unloaded_librarys_log_alone() {
        let user = tempfile::tempdir().expect("user dir");
        let mut ctx = open(user.path());
        let (gb, gb_text) = first_item(&ctx, GB);
        let (gbk, gbk_text) = first_item(&ctx, GBK);
        import(&mut ctx, GB, gb, &gb_text, 1);
        import(&mut ctx, GBK, gbk, &gbk_text, 1);
        assert_eq!(ctx.save_user(), SaveOutcome::Saved);
        let gb_log = user.path().join("gb_char.dbin");
        let gbk_log = user.path().join("gbk_char.dbin");
        let saved_gb = std::fs::read(&gb_log).expect("gb log");
        let saved_gbk = std::fs::read(&gbk_log).expect("gbk log");
        // Unsaved changes to both libraries, then GBK goes.
        import(&mut ctx, GB, gb, &gb_text, 2);
        import(&mut ctx, GBK, gbk, &gbk_text, 2);
        let with_unsaved = readings(&ctx, gbk);
        assert!(ctx.unload_phrase_library(GBK));
        assert_eq!(readings(&ctx, gbk), 0, "unload drops the sub-index");
        assert_eq!(ctx.save_user(), SaveOutcome::Saved);
        assert_eq!(
            std::fs::read(&gbk_log).expect("gbk log"),
            saved_gbk,
            "the save skips the unloaded library (`pinyin.cpp:928-933`)"
        );
        assert_ne!(
            std::fs::read(&gb_log).expect("gb log"),
            saved_gb,
            "the loaded library is saved"
        );
        // The reload merges the saved log: the unsaved reading is gone.
        assert_eq!(ctx.load_phrase_library(u32::from(GBK)), Ok(true));
        assert_eq!(readings(&ctx, gbk), with_unsaved - 1);
        assert_eq!(ctx.load_phrase_library(u32::from(GBK)), Ok(false));
    }

    #[test]
    fn unload_then_reload_restores_the_saved_library() {
        let user = tempfile::tempdir().expect("user dir");
        let mut ctx = open(user.path());
        let (gbk, text) = first_item(&ctx, GBK);
        import(&mut ctx, GBK, gbk, &text, 1);
        assert_eq!(ctx.save_user(), SaveOutcome::Saved);
        let saved = readings(&ctx, gbk);
        assert!(saved > 0);
        for _ in 0..2 {
            assert!(ctx.unload_phrase_library(GBK));
            assert!(
                !ctx.runtime
                    .as_ref()
                    .expect("runtime")
                    .dict()
                    .library_visible(u32::from(GBK))
            );
            assert_eq!(ctx.load_phrase_library(u32::from(GBK)), Ok(true));
            assert_eq!(readings(&ctx, gbk), saved);
        }
    }

    #[test]
    fn reload_after_an_unreadable_log_loads_the_system_chunk_alone() {
        let user = tempfile::tempdir().expect("user dir");
        let mut ctx = open(user.path());
        let (gbk, text) = first_item(&ctx, GBK);
        import(&mut ctx, GBK, gbk, &text, 1);
        assert_eq!(ctx.save_user(), SaveOutcome::Saved);
        assert!(readings(&ctx, gbk) > 0);
        let log = user.path().join("gbk_char.dbin");
        for unreadable in [None, Some(&b"abc"[..])] {
            assert!(ctx.unload_phrase_library(GBK));
            // `MemoryChunk::load` fails on a missing file and on one shorter
            // than its header (`memory_chunk.h:416-429`): an empty log.
            match unreadable {
                None => std::fs::remove_file(&log).expect("remove log"),
                Some(bytes) => std::fs::write(&log, bytes).expect("short log"),
            }
            assert_eq!(ctx.load_phrase_library(u32::from(GBK)), Ok(true));
            assert_eq!(readings(&ctx, gbk), 0, "the system chunk alone");
        }
    }
}
