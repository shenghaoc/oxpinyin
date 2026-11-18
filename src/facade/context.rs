//! Live context state: the option/scheme word shared by a context and
//! every instance it allocated, plus the context-level open/save laws.

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use crate::data::c_stderr;
use crate::engine::SessionSettings;
use crate::runtime::{
    FileType, OpenError, RemapStep, RowFault, Runtime, TableConfError, TableConfSite,
    directory_reads_as_a_chunk,
};
use crate::user::{
    ChunkHeaderField, JournalFault, MaskLogs, UserConfLaw, UserStore, UserStoreError,
};
use crate::{DoublePinyinScheme, FullPinyinScheme, OptionBits, ZhuyinScheme};

/// Where `mask_out`'s remap walk ended before it finished.
enum MaskStop {
    /// A library's saved log reached a header assertion (rows 91, 92).
    Journal(JournalFault),
    /// A library is a directory the pin asserts on (row 96).
    Directory,
}

/// The `mask_out` answer for a fault of the user tables.
fn mask_error_outcome(error: &UserStoreError) -> MaskOutOutcome {
    match error {
        UserStoreError::OverlongIndexKey => MaskOutOutcome::OverlongIndexKey,
        UserStoreError::NonTokenUserBigramKey => MaskOutOutcome::NonTokenUserBigramKey,
        UserStoreError::ShortUserBigramValue => MaskOutOutcome::ShortUserBigramValue,
        UserStoreError::PartialUserBigramTail => MaskOutOutcome::PartialUserBigramTail,
        UserStoreError::ResidualUserBigramGram => MaskOutOutcome::ResidualUserBigramGram,
        UserStoreError::Io(_)
        | UserStoreError::Store(_)
        | UserStoreError::Decode
        | UserStoreError::InvalidPhrase
        | UserStoreError::Persistence(_)
        | UserStoreError::UnknownDatabaseFormat
        | UserStoreError::ChunkHeaderWrite(_)
        | UserStoreError::LibraryChunkHeaderRead
        | UserStoreError::SaveRemapFailed
        | UserStoreError::SaveTooLarge
        | UserStoreError::UnigramTotalOverflow
        | UserStoreError::Journal(_)
        | UserStoreError::LibraryLoop(_)
        | UserStoreError::TrainingSetFreqMiss
        | UserStoreError::UserReadingRemoval
        | UserStoreError::SessionSeed(_)
        | UserStoreError::FailedUserBigramStore => MaskOutOutcome::Done(false),
        UserStoreError::FatalCopy => MaskOutOutcome::FatalCopy,
    }
}

/// What `save_user` did, for the C facades' `pinyin_save` / `zhuyin_save`
/// and the `pinyin_end_add_phrases` import commit.
///
/// Upstream's save never stops at a filesystem failure and answers `true`
/// (`pinyin.cpp:1132-1147`); the one failure it *does* stop at is the
/// chunk header `assert` (`memory_chunk.h:543`/`:547`), where it dies.
/// The facade reports that failure alone, so it can be told apart from the
/// quiet `false` and from the pin's `true`; the C adapters abort on it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SaveOutcome {
    /// No user directory, nothing modified, or an internal encode/compact
    /// failure the pin has no counterpart for: the deliberate quiet
    /// `false`.
    NotSaved,
    /// The file set was written and renamed: the pin's `true`.
    Saved,
    /// A chunk file's `MemoryChunk::save` header write failed; the pin
    /// `assert`s and dies of SIGABRT. The C adapters abort.
    ChunkWriteFailed(ChunkHeaderField),
    /// A loaded system library's file is a directory the pin maps again and
    /// asserts on (`memory_chunk.h:493`, `_write_files`' diff of each system
    /// library, `pinyin.cpp:955`, `zhuyin.cpp:588`; register row 96). The
    /// save stopped at that library, with the earlier libraries' `.tmp`
    /// files written and, for libzhuyin, renamed, as the pin leaves them.
    /// The C adapters abort.
    ChunkHeaderRead,
    /// A tkrzw or Berkeley DB record copy failed where the pin's
    /// `copy_tkrzwdb` or `copy_bdb` asserts (`tkrzwdb_utils.h:65`, `:72`;
    /// `bdb_utils.h:61`, `:67`). The save stopped in that file's copy:
    /// libpinyin's earlier `.tmp` files written and none renamed,
    /// libzhuyin's earlier files renamed, no marker. The C adapters abort.
    FatalCopy,
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
/// (`chewing_large_table2_bdb.cpp:529`). The facade reports that failure
/// alone, told apart from the quiet `true`/`false`; the C adapters abort on it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaskOutOutcome {
    /// The store's mask ran (`bool` is its own success), or there is no
    /// user store to mask (the pin's `false`).
    Done(bool),
    /// The user pinyin index carried a key past `MAX_PHRASE_LENGTH`
    /// syllables; the C adapters abort, as the pin does.
    OverlongIndexKey,
    /// The user bigram carried a key that is not a four-byte
    /// `phrase_token_t`; the pin's `get_all_items` asserts
    /// (`ngram_bdb.cpp:199`). The C adapters abort, as the pin does.
    NonTokenUserBigramKey,
    /// The user bigram carried a value shorter than a `guint32`
    /// `total_freq` under a key the mask leaves to be loaded: the pin's
    /// `SingleGram::mask_out` asserts in `get_total_freq`
    /// (`memory_chunk.h:390`, `ngram.cpp:80`; register row 85). The C
    /// adapters abort.
    ShortUserBigramValue,
    /// The user bigram carried a partial eight-byte item tail under a key
    /// the mask leaves to be loaded: `SingleGram::mask_out` reads past it
    /// (`ngram.cpp:76-86`, register row 98, class (b)). The C ABI answers
    /// `false` and logs one warning.
    PartialUserBigramTail,
    /// Masking the user bigram removed every item of a gram whose total
    /// its items did not cover, leaving a residual `total_freq`; the pin's
    /// `get_length` asserts (`ngram.cpp:70`, from `Bigram::mask_out`,
    /// `ngram_bdb.cpp:243`). The C adapters abort.
    ResidualUserBigramGram,
    /// A loaded system library's file is a directory the pin reloads and
    /// asserts on (`memory_chunk.h:493`, `pinyin.cpp:1265`,
    /// `zhuyin.cpp:799`; register row 96), after the user tables were masked
    /// and the libraries before it reloaded. The C adapters abort.
    ChunkHeaderRead,
    /// A loaded system library's user `.dbin`, reloaded through
    /// `merge_with_mask` after the user tables were masked, reached a pin
    /// assertion ([`JournalFault`], rows 91, 92, 97 and 108). The pin dies
    /// there; the C adapters abort.
    Journal(JournalFault),
    /// A tkrzw or Berkeley DB record copy failed where the pin's
    /// `copy_tkrzwdb` or `copy_bdb` asserts (`tkrzwdb_utils.h:65`, `:72`;
    /// `bdb_utils.h:61`, `:67`). The C adapters abort. `mask_out` does not
    /// copy, so it never answers this today; it keeps
    /// [`UserStoreError::FatalCopy`] from reading as a soft `false`.
    FatalCopy,
}

impl MaskOutOutcome {}

/// Why a context did not open — what `pinyin_init` / `zhuyin_init` hide
/// behind NULL. Carried out of [`ContextCore::try_open`] so the facades
/// can log it; the return value stays NULL either way.
#[derive(Debug)]
pub enum OpenFailure {
    /// The runtime could not open the system directory.
    Runtime(OpenError),
}

impl core::fmt::Display for OpenFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Runtime(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for OpenFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Runtime(error) => Some(error),
        }
    }
}

/// How the C facade reports a failed init. The ABI adapter performs the
/// abort or writes the diagnostic in its own log domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitRefusal {
    /// The pin asserts or aborts.
    Abort,
    /// The pin writes the raw table-conf load-failed line.
    TableConfLoadFailed,
    /// A table-conf fault covered by the class (b) warning policy.
    TableConfWarning(&'static str),
    /// An ordinary open failure, reported with the caller's ABI name.
    OpenWarning,
}

impl OpenFailure {
    /// Applies the pin's failed-init decision and delegates only diagnostics
    /// to the C boundary that owns their GLib and stderr calls.
    pub fn refuse_init(
        &self,
        system_path: &Path,
        init_name: &str,
        table_conf_load_failed: impl FnOnce(&Path),
        log_warning: impl FnOnce(&str),
    ) {
        match self.init_refusal(system_path) {
            InitRefusal::Abort => std::process::abort(),
            InitRefusal::TableConfLoadFailed => table_conf_load_failed(system_path),
            InitRefusal::TableConfWarning(message) => log_warning(message),
            InitRefusal::OpenWarning => {
                log_warning(&format!("{init_name}: {self} (systemdir {system_path:?})"));
            }
        }
    }

    /// Classifies the failure at the point where init stopped.
    ///
    /// Unknown format aborts while reading user.conf (074a2219,
    /// table_info.cpp:122-133, :353-354). Journal and session faults stop
    /// after the earlier libraries and profile handling (phrase_index_logger.h:202,
    /// memory_chunk.h:390, ngram_bdb.cpp:51-52). A directory read can assert
    /// at memory_chunk.h:493, including a table row that names no chunk.
    #[must_use]
    pub fn init_refusal(&self, system_path: &Path) -> InitRefusal {
        let Self::Runtime(error) = self;
        match error {
            OpenError::UnknownDatabaseFormat(_)
            | OpenError::Journal(_)
            | OpenError::UserSession(_) => InitRefusal::Abort,
            OpenError::TableConf(conf) => {
                // table_info.cpp:119, :132, :142, :156, :175 and :276
                // abort/assert on malformed rows and enums. A chunkless
                // row depends on whether the host reads the directory as
                // a chunk. The missing source-format line keeps its warning.
                let aborts = match conf {
                    TableConfError::Abort(TableConfSite::NullSystemFile) => {
                        null_chunk_asserts(system_path)
                    }
                    TableConfError::Abort(
                        TableConfSite::SourceFormat
                        | TableConfSite::DatabaseFormat
                        | TableConfSite::Target
                        | TableConfSite::DefaultIndex
                        | TableConfSite::FileType
                        | TableConfSite::AddonIndex
                        | TableConfSite::DefaultDictionary
                        | TableConfSite::NullUserFile,
                    ) => true,
                    TableConfError::Unopenable
                    | TableConfError::Header
                    | TableConfError::UninitialisedSourceFormat => false,
                };
                if aborts {
                    InitRefusal::Abort
                } else {
                    conf.warning().map_or(
                        InitRefusal::TableConfLoadFailed,
                        InitRefusal::TableConfWarning,
                    )
                }
            }
            OpenError::Dict(_) => {
                if error.chunk_header_read() {
                    InitRefusal::Abort
                } else {
                    InitRefusal::OpenWarning
                }
            }
            OpenError::Io(..) | OpenError::Lm(_) => InitRefusal::OpenWarning,
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
/// answer and warning stay the abort site's either way. The boundary says
/// whether this platform lets the chunk loader open a directory; where it
/// does not, the pin reaches neither branch.
fn writes_null_chunk_line(dir: &Path, reads_as_a_chunk: impl FnOnce(&Path) -> bool) -> bool {
    crate::input::chunk::opens_directories() && !reads_as_a_chunk(dir)
}

/// Whether the pin's `MemoryChunk::mmap` asserts (`memory_chunk.h:493`) on
/// `dir` read as the chunk of a row that names none — the complement of
/// `writes_null_chunk_line`: where it does not assert, it writes
/// `mmap %s failed!` and dies of SIGSEGV on the empty chunk instead (class
/// (b)). The C facades abort the process on the assert shape.
#[must_use]
pub fn null_chunk_asserts(dir: &Path) -> bool {
    !writes_null_chunk_line(dir, directory_reads_as_a_chunk)
}

/// Writes the pin's `mmap %s failed!` line for `dir` with
/// [`c_stderr::mmap_failed`], if `writes_null_chunk_line` says the pin
/// writes it on this file system.
fn null_chunk_refused(dir: &Path) {
    if writes_null_chunk_line(dir, directory_reads_as_a_chunk) {
        c_stderr::mmap_failed(dir);
    }
}

/// Bit 30 of [`LiveOptionsInner::double_scheme`]: when set, the live
/// scheme's fallback table is suppressed — upstream's half-mutation
/// (`pinyin_parser2.cpp:580` + `:614`; `compatibility-policy.md`
/// row 5b).  A valid-scheme store clears it implicitly (values 1–6
/// never have bit 30), and the out-of-enum arm sets it.
pub const FALLBACK_CLEARED_BIT: i32 = 1 << 30;

/// The live option/scheme state a context owns and every instance it
/// allocates shares: `set_options`/`set_*_scheme` on the context remask
/// already-allocated instances through this one handle, as the pin's
/// instances read their context through `m_context`.
///
/// Both C-ABI facades carry the same fields; the seed word differs
/// per facade and is the caller's choice at [`ContextCore::open`].
#[derive(Clone)]
pub struct LiveOptions(Rc<LiveOptionsInner>);

/// The shared cells behind [`LiveOptions`].
pub struct LiveOptionsInner {
    /// Live double-pinyin scheme (header discriminant value, bits 0–29)
    /// with the fallback-cleared flag packed into bit 30
    /// ([`FALLBACK_CLEARED_BIT`]).
    pub double_scheme: Cell<i32>,
    /// Live Zhuyin scheme (header discriminant value).
    pub zhuyin_scheme: Cell<i32>,
    /// Live full-pinyin scheme (header discriminant value).
    pub full_scheme: Cell<i32>,
    /// Live option word. The parsers read `FORCE_TONE`, `USE_TONE` and
    /// `PINYIN_INCOMPLETE` off it.
    pub options: Cell<OptionBits>,
}

impl core::ops::Deref for LiveOptions {
    type Target = LiveOptionsInner;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl LiveOptions {
    /// Seeds the live state the way an init does: the option word is the
    /// caller's seed word, and the three schemes start at the header
    /// defaults both facades share (MS double, Standard zhuyin, Hanyu full).
    #[must_use]
    pub(crate) fn new(option_word: OptionBits) -> Self {
        Self(Rc::new(LiveOptionsInner {
            double_scheme: Cell::new(i32::from(DoublePinyinScheme::Ms)),
            zhuyin_scheme: Cell::new(i32::from(ZhuyinScheme::Standard)),
            full_scheme: Cell::new(i32::from(FullPinyinScheme::Hanyu)),
            options: Cell::new(option_word),
        }))
    }
}

/// State behind a facade's context handle, minus the C parts: the shared
/// assembly, the user-learning store, the layered configuration, and the
/// live option/scheme word every allocated instance shares.
///
/// [`Self::finish`] performs the facade's fini without releasing storage:
/// the user store it opened makes the
/// fini-time `user.conf` write of the law it was opened under —
/// libpinyin lowers the open counter its init raised, libzhuyin writes
/// nothing (`crate::user::persistence::fini`). A context that is never
/// finalised, like a process that dies before its `pinyin_fini`, leaves the
/// raised counter on disk. Drop also finalises a store that was not explicitly
/// finished; a later drop never repeats the marker write.
pub struct ContextCore {
    /// The settings instances are opened with: incomplete pinyin on until
    /// the first `set_options`, then that word's bit.
    pub settings: SessionSettings,
    /// The shared concrete assembly and the context's sole user store;
    /// instances borrow it per operation. `pinyin_init` answers NULL when
    /// the system tables do not load (`pinyin.cpp:337-339`), so every
    /// context has one.
    pub runtime: Runtime,
    /// The live option/scheme word, shared with every instance.
    pub live: LiveOptions,
}

impl ContextCore {
    /// Run public context fini now; retained C handles may still borrow the
    /// same runtime afterwards. Releasing the last owner does not repeat it.
    pub fn finish(&mut self) {
        self.runtime.finish();
    }

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
        option_word: OptionBits,
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
        option_word: OptionBits,
        law: UserConfLaw,
    ) -> Result<Self, OpenFailure> {
        // The libraries whose file is absent load as unloaded and the open
        // goes on; the pin's lines for them follow the user profile's
        // (`check_format` runs before the library loop), on its stream, and
        // stop where the loop dies.
        let runtime = Runtime::open_with_law_reporting(system_dir, user_dir, law, &mut |path| {
            c_stderr::mmap_failed(path)
        })
        .map_err(|error| {
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
        Ok(Self {
            settings: SessionSettings::default(),
            runtime,
            live: LiveOptions::new(option_word),
        })
    }

    /// `set_options`'s law: the word is stored, and the incomplete-pinyin
    /// setting follows the word so sessions opened later agree with
    /// sessions already allocated. The parsers read the option bits off
    /// the stored word.
    pub fn set_options(&mut self, options: OptionBits) {
        self.settings.incomplete_pinyin = options.contains(OptionBits::PINYIN_INCOMPLETE);
        self.live.options.set(options);
    }

    /// Allocates one instance's orchestration state over this context's
    /// assembly. `Runtime::new_session`
    /// cannot fail.
    #[must_use]
    pub fn alloc_instance(&self) -> crate::facade::instance::InstanceCore {
        let runtime = &self.runtime;
        let mut session = runtime.new_session(self.settings);
        runtime.session(&mut session).set_defer_list_builds(true);
        crate::facade::instance::InstanceCore::new(session, self.live.clone())
    }

    /// `save`'s body: [`SaveOutcome::NotSaved`] without a user dir or when
    /// unmodified, [`SaveOutcome::Saved`] after a dirty save, and
    /// [`SaveOutcome::ChunkWriteFailed`] at the one write the pin dies on.
    ///
    /// The pin never stops at a failing write or rename: it prints what
    /// failed (`rename %s to %s failed.`, `write %s failed.`, raw
    /// `fprintf(stderr, …)`, `pinyin.cpp:1061`…`:1123`,
    /// `table_info.cpp:382`) and answers `true`. The store prints one line
    /// per soft failure on stderr in the pin's order.
    ///
    /// A chunk header write is the exception: `MemoryChunk::save`'s
    /// `assert`s (`memory_chunk.h:543`, `:547`) kill the process there, so
    /// the store's `save_reporting` stops instead of reporting, and this
    /// surface must fail the call. The caller logs the point.
    ///
    /// A system library whose layout `SubPhraseIndex::load` refuses again at
    /// the save's remap is diffed against the pin's empty sub-index; the
    /// pin's `g_return_val_if_fail` critical there is not reproduced
    /// (2026-10-31 UTC ruling,
    /// `docs/findings/upstream-divergences.md#phrase-library-layout-faults-write-no-critical`).
    pub fn save_user(&mut self) -> SaveOutcome {
        let store = self.user_store();
        if !store.has_user_directory() || !store.is_modified() {
            return SaveOutcome::NotSaved;
        }
        // `pinyin_save`/`zhuyin_save` rebuild every sub-index from its
        // items before `_write_files` (`pinyin.cpp:1139`,
        // `zhuyin.cpp:554`); the failed empty sub-index's local total
        // becomes zero, while the facade aggregate already excludes it.
        let compacted = self.compact_layout_failed();
        let unloaded = self.runtime.unloaded_libraries();
        let store = self.user_store_mut();
        // `_write_files` maps each loaded system library again, in the
        // library loop, to diff it against the live one (`pinyin.cpp:955`,
        // `zhuyin.cpp:588`), after the earlier libraries' files are written.
        // A file `MemoryChunk::mmap` refuses gets its `mmap %s failed!` line
        // there (row 76), on the stream the save's rename and marker lines
        // use. A library that is a directory the pin asserts on
        // (`memory_chunk.h:493`, register row 96) stops the save there.
        let baseline =
            |nibble: u8, originals: &crate::user::SystemOriginals| -> crate::user::SaveBaseline {
                match Runtime::read_save_baseline(originals, unloaded, nibble) {
                    Ok(baseline) => baseline,
                    Err(crate::runtime::SaveRemapFault::Unmappable(path)) => {
                        c_stderr::mmap_failed(&path);
                        crate::user::SaveBaseline::Unmappable
                    }
                    Err(crate::runtime::SaveRemapFault::ChunkHeaderRead) => {
                        crate::user::SaveBaseline::ChunkHeaderRead
                    }
                }
            };
        match store.save_reporting_with_baseline(&compacted, baseline) {
            Ok(()) => (),
            Err(UserStoreError::ChunkHeaderWrite(field)) => {
                return SaveOutcome::ChunkWriteFailed(field);
            }
            Err(UserStoreError::LibraryChunkHeaderRead) => return SaveOutcome::ChunkHeaderRead,
            Err(UserStoreError::FatalCopy) => return SaveOutcome::FatalCopy,
            // Row 76: the save stopped at a library whose file cannot be
            // mapped again, with the session still dirty.
            Err(UserStoreError::SaveRemapFailed) => return SaveOutcome::NotSaved,
            // Row 112: stop before the undefined copy, preserving earlier file
            // effects and keeping the session dirty for retry.
            Err(UserStoreError::SaveTooLarge) => return SaveOutcome::NotSaved,
            Err(
                UserStoreError::Io(_)
                | UserStoreError::Store(_)
                | UserStoreError::Decode
                | UserStoreError::InvalidPhrase
                | UserStoreError::Persistence(_)
                | UserStoreError::UnknownDatabaseFormat
                | UserStoreError::UnigramTotalOverflow
                | UserStoreError::OverlongIndexKey
                | UserStoreError::NonTokenUserBigramKey
                | UserStoreError::ShortUserBigramValue
                | UserStoreError::PartialUserBigramTail
                | UserStoreError::ResidualUserBigramGram
                | UserStoreError::Journal(_)
                | UserStoreError::LibraryLoop(_)
                | UserStoreError::TrainingSetFreqMiss
                | UserStoreError::UserReadingRemoval
                | UserStoreError::SessionSeed(_)
                | UserStoreError::FailedUserBigramStore,
            ) => return SaveOutcome::NotSaved,
        };
        // Each failed rename's `rename %s to %s failed.` line and a failed
        // marker's `write %s failed.` line were written as they happened,
        // in the pin's order, by the save itself.
        SaveOutcome::Saved
    }

    /// The system libraries (with `only`, that one) whose file the pin would
    /// map again at this call and cannot: one `mmap %s failed!` each, in
    /// library order ([`c_stderr::mmap_failed`]). The pin dies on the first;
    /// nothing here does, and the call answers what it answered before.
    fn check_system_libraries(&self, only: Option<u8>) -> bool {
        let checked = self.runtime.check_system_libraries(only);
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
    /// `zhuyin.cpp:554`), at `end_add_phrases` (`pinyin.cpp:657`,
    /// `zhuyin.cpp:542`) and at the end of `mask_out` (`pinyin.cpp:1294`,
    /// `zhuyin.cpp:829`). Returns the compacted nibbles.
    #[must_use]
    pub fn compact_layout_failed(&mut self) -> Vec<u8> {
        self.runtime.compact_layout_failed()
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
    /// `merge_with_mask` (`UserStore::reload_system_libraries`); the first
    /// pin assertion a log reaches answers [`MaskOutOutcome::Journal`], after
    /// the user tables were masked and the libraries before it reloaded.
    pub fn mask_out(&mut self, mask: u32, value: u32) -> MaskOutOutcome {
        // The user tables come first (`pinyin.cpp:1228-1230`,
        // `zhuyin.cpp:763-765`): their faults precede any system library or
        // journal, so find them before opening one. This changes no state.
        let checked = match self.user_store().check_mask_out(mask, value) {
            Ok(checked) => checked,
            Err(error) => return mask_error_outcome(&error),
        };
        // Then the pin's loop (`pinyin.cpp:1260-1294`): it maps library i,
        // reads its log once and checks its headers, then goes to library
        // i+1, so the first fault in that order decides and no later library
        // is opened. A refused remap (row 76) ends the call before any
        // effect. A journal header fault (rows 91, 92) or a directory (row
        // 96) ends the walk; the replay below then meets it in the recorded
        // order, after the earlier libraries' own merges.
        let mut logs = MaskLogs::default();
        let mut stop = None;
        let store = self.runtime.user_store();
        let walk = {
            let runtime = &self.runtime;
            let mut remap = runtime.begin_system_remap();
            while let Some(step) = runtime.remap_next(&mut remap) {
                match step {
                    RemapStep::Mapped(index) => {
                        if let Some(fault) = store.read_mask_log(&mut logs, index, mask, value) {
                            stop = Some(MaskStop::Journal(fault));
                            break;
                        }
                    }
                    RemapStep::Unmappable(path) => {
                        c_stderr::mmap_failed(&path);
                        return MaskOutOutcome::Done(true);
                    }
                    RemapStep::Directory => {
                        stop = Some(MaskStop::Directory);
                        break;
                    }
                }
            }
            remap
        };
        let store = self.user_store_mut();
        // The user tables masked (`pinyin.cpp:1228-1230`, `zhuyin.cpp:763-765`).
        // The remap walk and the log reads above wrote nothing to them, so
        // the check at the top of the call still holds.
        if let Err(error) = store.apply_mask_out(checked, mask, value) {
            return mask_error_outcome(&error);
        }
        // Then the phrase index loop: each loaded system sub-index is
        // replaced before its disk log is replayed with the mask; the loop
        // skips a sub-index `get_range` reports missing (074a2219
        // pinyin.cpp:1234-1291, zhuyin.cpp:769-826), which the runtime's
        // remap leaves out. The separate phrase table is retained.
        //
        // Each library is mapped (`pinyin.cpp:1264`, its `mmap %s failed!`
        // line written there) before its log is merged; a library that is a
        // directory the pin asserts on (`memory_chunk.h:493`, register row
        // 96) ends the loop after the libraries before it were mapped and
        // merged, and a merge of theirs that asserts comes first.
        {
            let mut originals = self.runtime.finish_system_remap(walk);
            let replay = self.user_store_mut().reload_system_libraries_prefetched(
                &mut originals,
                mask,
                value,
                logs,
            );
            self.runtime.install_baselines(originals);
            // The replay stops at the first library whose merge asserts, so
            // a fault the walk found comes after the merges before it.
            match (replay, stop) {
                (Err(fault), _) | (Ok(()), Some(MaskStop::Journal(fault))) => {
                    return MaskOutOutcome::Journal(fault);
                }
                (Ok(()), Some(MaskStop::Directory)) => return MaskOutOutcome::ChunkHeaderRead,
                (Ok(()), None) => {}
            }
            // The mask ends with a compact (`pinyin.cpp:1294`,
            // `zhuyin.cpp:829`); the failed empty sub-index's local
            // total becomes zero.
            let _compacted = self.compact_layout_failed();
        }
        self.user_store_mut().compact_libraries();
        MaskOutOutcome::Done(true)
    }

    /// Whether `user_bigram.db` carried a key that is not a four-byte
    /// `phrase_token_t` ([`UserStoreError::NonTokenUserBigramKey`]). The
    /// pin's `Bigram::get_all_items` assert dies (`ngram_bdb.cpp:199`),
    /// reached by `pinyin_begin_get_bigram_phrases`.
    #[must_use]
    pub fn has_non_token_bigram_key(&self) -> bool {
        self.user_store().has_non_token_bigram_key()
    }

    /// `pinyin_load_phrase_library`'s body (074a2219 `pinyin.cpp:446-459`,
    /// `_load_phrase_library` `:234-322`; `zhuyin.cpp:361-375`, `:178-266`):
    /// an in-memory transition of the session, as the pin's. A library that
    /// is loaded answers `false` (`get_range` found its sub-index,
    /// `:242-244`). An unloaded system library is mapped again, whether or not
    /// its row names a user file (`:246-264`), and only its saved `.dbin` is
    /// merged, an unreadable one being an empty log; an unloaded `USER_FILE`
    /// library comes back from its saved chunk, or empty
    /// (`:303-321`). The runtime's visibility mask then clears and the call
    /// answers `true`. A system file that cannot be mapped again is the pin's
    /// SIGSEGV (policy row 76): the library stays unloaded behind its mask,
    /// the call answers `true`, and the next load maps the file again.
    /// Nothing is written to disk, and nothing that can fail is: the pin has
    /// no failing site here (policy row 103).
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
    pub fn load_phrase_library(&mut self, index: u32) -> Result<bool, LibraryLoadFault> {
        let runtime = &self.runtime;
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
        let mut originals = self.runtime.reload_system_library(row_index);
        // The mask clears only for a library the remap mapped. A system row
        // is mapped again whether or not it names a user file
        // (`pinyin.cpp:246-264`); a file that cannot be mapped is the pin's
        // SIGSEGV (row 76). The library then keeps its mask and the call its
        // answer, and the next load tries again, so no reader meets a
        // library whose bytes the session no longer holds.
        if !user_file && !originals.contains_key(&row_index) {
            return Ok(true);
        }
        let store = self.user_store_mut();
        let replay = {
            if user_file {
                store.load_user_library(row_index);
                Ok(())
            } else if store.is_system_library(row_index) {
                store.load_system_libraries_retaining(&mut originals)
            } else {
                // A row that names no user file has no journal in the
                // session: its new mapping, published below, is all there is
                // to replace.
                Ok(())
            }
        };
        self.runtime.install_baselines(originals);
        replay.map_err(LibraryLoadFault::Journal)?;
        Ok(self.runtime.load_library(index))
    }

    /// `load_addon_phrase_library`'s body: the runtime's addon load, with
    /// the `mmap %s failed!` line the pin writes when the library file does
    /// not map (`pinyin.cpp:290`); the answer is the runtime's, `false` for
    /// a failure.
    ///
    /// # Errors
    ///
    /// The [`LibraryRowAssert`] the pin dies on: the addon row is neither
    /// `DICTIONARY` nor `NOT_USED` (`pinyin.cpp:491`), or names no chunk.
    pub fn load_addon_phrase_library(&mut self, index: u8) -> Result<bool, LibraryRowAssert> {
        let runtime = &mut self.runtime;
        match runtime.load_system_addon_reporting(index) {
            Ok(loaded) => Ok(loaded),
            Err(crate::runtime::LibraryError::Row(fault)) => Err(match fault {
                RowFault::NotDictionary => LibraryRowAssert::NotDictionary,
                RowFault::NullChunkName => {
                    if let Some(dir) = runtime.system_dir() {
                        null_chunk_refused(dir);
                    }
                    LibraryRowAssert::NullChunkName
                }
                RowFault::ChunkHeaderRead => LibraryRowAssert::ChunkHeaderRead,
            }),
            Err(
                error @ (crate::runtime::LibraryError::Io(_)
                | crate::runtime::LibraryError::Format(_)
                | crate::runtime::LibraryError::Layout(_)
                | crate::runtime::LibraryError::Unmappable { .. }),
            ) => {
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
    /// the pin has no failing site here (policy row 103). Which indices a
    /// facade lets through is the C adapters' rule.
    #[must_use]
    pub fn unload_phrase_library(&mut self, index: u8) -> bool {
        let runtime = &self.runtime;
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
        let system_file = runtime.dict().library_visible(u32::from(index))
            && runtime.dict().system().libraries().is_loaded(index);
        {
            let store = self.user_store_mut();
            if user_file {
                store.unload_user_library(index);
            } else if system_file {
                store.reset_system_library(index);
            }
        }
        self.runtime.unload_library(u32::from(index))
    }

    /// Borrow the context's user store for one read operation.
    #[must_use]
    pub fn user_store(&self) -> &UserStore {
        self.runtime.user_store()
    }

    /// Borrow the sole learning-store owner for a write operation.
    pub fn user_store_mut(&mut self) -> &mut UserStore {
        self.runtime.user_store_mut()
    }

    /// The import iterator's close step (`pinyin_end_add_phrases`,
    /// `zhuyin_end_add_phrases`, 074a2219 `pinyin.cpp:655-660`,
    /// `zhuyin.cpp:540-545`): `FacadePhraseIndex::compact` (`:657`, `:542`)
    /// rebuilds each sub-index from its items, so a layout-failed empty
    /// sub-index's local total becomes zero, and `m_modified` arms the next
    /// save. The constructors do not compact (`pinyin.cpp:506-512`,
    /// `zhuyin.cpp:392-398`): while an iterator is open, an unload of a
    /// layout-failed library takes its local total out of the facade total
    /// (`phrase_index.cpp:261-269`).
    pub fn end_import(&mut self) {
        let _compacted = self.compact_layout_failed();
        let user = self.user_store_mut();
        user.compact_libraries();
        user.mark_modified();
    }

    /// Train retained composition after re-borrowing the current storage.
    pub fn train(
        &mut self,
        instance: &crate::facade::InstanceCore,
        index: u8,
    ) -> Result<bool, crate::engine::EngineError> {
        self.runtime.train_session(&instance.session, index)
    }
}

#[cfg(test)]
mod tests;
