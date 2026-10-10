//! Live context state: the option/scheme word shared by a context and
//! every instance it allocated, plus the context-level open/save laws.

use std::path::Path;
use std::sync::Arc;

use oxpinyin_core::{DoublePinyinScheme, FullPinyinScheme, OptionBits, ZhuyinScheme};
use oxpinyin_engine::{Config, ConfigValue};
use oxpinyin_runtime::{FileType, OpenError, RowFault, Runtime, TableConf, TableConfError};
use oxpinyin_user::SystemVersions;
use oxpinyin_user::pin_stderr;
use oxpinyin_user::{ChunkHeaderField, UserConfLaw, UserStore, UserStoreError};

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
}

impl SaveOutcome {
    /// The C ABI's answer: only [`SaveOutcome::Saved`] is `true`.
    #[must_use]
    pub fn is_saved(self) -> bool {
        matches!(self, Self::Saved)
    }
}

/// Why a context did not open — what `pinyin_init` / `zhuyin_init` hide
/// behind NULL. Carried out of [`ContextCore::try_open`] so the facades
/// can log it; the return value stays NULL either way.
#[derive(Debug)]
#[non_exhaustive]
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

/// The fixed diagnostic the C facades log at the user marker's class-(c)
/// refusal: the step that fails and the condition, in the shape the
/// register row for the site records. Upstream's own `check_format` never
/// gets to report it — `to_table_database_format_type` `abort()`s first
/// (`table_info.cpp:132`) — so this line is what a consumer's log shows
/// where the pin's process would have died. Exactly one line per attempt.
pub const UNKNOWN_DATABASE_FORMAT_WARNING: &str =
    "check_format: unknown database format in user.conf";

impl OpenFailure {
    /// Whether this failure is the user marker's class-(c) refusal: a
    /// `user.conf` whose `database format:` field upstream's
    /// `to_table_database_format_type` does not know, and therefore
    /// `abort()`s on (`table_info.cpp:122-133`, reached from `:353-354`).
    ///
    /// The C facades log [`UNKNOWN_DATABASE_FORMAT_WARNING`] for it and
    /// answer NULL; nothing was cleaned and no marker was written, which
    /// is also where upstream stops.
    #[must_use]
    pub fn unknown_database_format(&self) -> bool {
        matches!(
            self,
            Self::Runtime(oxpinyin_runtime::OpenError::UnknownDatabaseFormat(_))
        )
    }

    /// The system `table.conf` failure behind this open, if that is why it
    /// failed: [`TableConfError::Header`] is the pin's ordinary `false`
    /// return (the facade writes `load %s failed!`, no warning); every
    /// other kind is a site the pin dies on, answered with one warning.
    #[must_use]
    pub fn table_conf_error(&self) -> Option<TableConfError> {
        match self {
            Self::Runtime(oxpinyin_runtime::OpenError::TableConf(error)) => Some(*error),
            _ => None,
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
            // `zhuyin.cpp:200`); this open answers NULL as it did.
            if let Some(path) = error.unmappable_library() {
                pin_stderr::mmap_failed(path);
            }
            OpenFailure::Runtime(error)
        })?;
        // The libraries whose file is absent load as unloaded and the open
        // goes on; the pin's lines for them follow the user profile's
        // (`check_format` runs before the library loop).
        for path in runtime.unmapped_system_libraries() {
            pin_stderr::mmap_failed(path);
        }
        let user = runtime.user_store();
        Ok(Self {
            config: Config::default(),
            runtime: Some(runtime),
            user,
            live: LiveOptions::new(option_word),
        })
    }

    /// User-store-only context for standalone tools (the §9 import/export
    /// machinery): a decoder context this is not, so
    /// [`ContextCore::alloc_instance`] answers `None` for it. `law` is the
    /// user dir's `user.conf` lifecycle, as for [`Self::open`].
    #[must_use]
    pub fn new_user_only(user_dir: &str, option_word: u32, law: UserConfLaw) -> Option<Self> {
        if user_dir.is_empty() {
            return None;
        }
        // The standalone user context persists in the pin's user-dir
        // file set too: no system dir is opened, so the diff base is
        // empty (system-token deltas do not persist from a user-only
        // context) and the conformance triple is the pin's.
        let user = UserStore::open_libpinyin(
            Path::new(user_dir),
            std::collections::BTreeMap::new(),
            SystemVersions::from_conf(&TableConf::stock()),
            law,
        )
        .ok()?;
        Some(Self {
            config: Config::default(),
            runtime: None,
            user: Some(user),
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
    pub fn save_user(&mut self) -> SaveOutcome {
        let Some(store) = self.user.as_ref() else {
            return SaveOutcome::NotSaved;
        };
        if !store.has_user_directory() || !store.is_modified() {
            return SaveOutcome::NotSaved;
        }
        // `_write_files` maps every loaded system library again to diff it
        // against the live one (`pinyin.cpp:956`, `zhuyin.cpp:589`), before
        // it writes anything.
        self.report_unmappable_libraries(None);
        let Some(store) = self.user.as_mut() else {
            return SaveOutcome::NotSaved;
        };
        let report = match store.save_reporting() {
            Ok(report) => report,
            Err(UserStoreError::ChunkHeaderWrite(field)) => {
                return SaveOutcome::ChunkWriteFailed(field);
            }
            Err(_) => return SaveOutcome::NotSaved,
        };
        for (tmp, target) in &report.renames_failed {
            pin_stderr::emit(&[
                b"rename ",
                pin_stderr::path_bytes(tmp),
                b" to ",
                pin_stderr::path_bytes(target),
                b" failed.\n",
            ]);
        }
        if let Some(conf) = &report.user_conf_write_failed {
            pin_stderr::emit(&[b"write ", pin_stderr::path_bytes(conf), b" failed.\n"]);
        }
        SaveOutcome::Saved
    }

    /// The system libraries (with `only`, that one) whose file the pin would
    /// map again at this call and cannot: one `mmap %s failed!` each, in
    /// library order. The pin dies on the first; nothing here does, and the
    /// call answers what it answered before.
    fn report_unmappable_libraries(&self, only: Option<u8>) {
        let Some(runtime) = &self.runtime else {
            return;
        };
        for path in runtime.unmappable_system_libraries(only) {
            pin_stderr::mmap_failed(&path);
        }
    }

    /// `mask_out`'s body: the store-level deletion, or `false` without a
    /// user store. The pin maps every loaded system library again first
    /// (`pinyin.cpp:1265`, `zhuyin.cpp:800`).
    pub fn mask_out(&mut self, mask: u32, value: u32) -> bool {
        self.report_unmappable_libraries(None);
        self.user
            .as_mut()
            .is_some_and(|store| store.mask_out(mask, value).is_ok())
    }

    /// `load_phrase_library`'s read side: the runtime's library-load
    /// (mask-clear) rule; `false` without a runtime.
    ///
    /// # Errors
    ///
    /// [`LibraryRowAssert::NotLoadable`] where the pin `assert`s: the
    /// index is in range and its default row is neither `SYSTEM_FILE` nor
    /// `USER_FILE` (`pinyin.cpp:457`, `zhuyin.cpp:372`).
    pub fn load_phrase_library(&self, index: u32) -> Result<bool, LibraryRowAssert> {
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
            return Err(LibraryRowAssert::NotLoadable);
        }
        let loaded = runtime.load_library(index);
        if loaded && let Ok(index) = u8::try_from(index) {
            // The pin maps the file again for a library it had unloaded
            // (`pinyin.cpp:256`, `zhuyin.cpp:200`); this reload only lifts
            // the mask, and answers as before.
            self.report_unmappable_libraries(Some(index));
        }
        Ok(loaded)
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
                RowFault::NullChunkName => LibraryRowAssert::NullChunkName,
            }),
            Err(error) => {
                if let Some(path) = error.unmappable_path() {
                    pin_stderr::mmap_failed(path);
                }
                Ok(false)
            }
        }
    }

    /// `unload_phrase_library`'s read side; `false` without a runtime.
    #[must_use]
    pub fn unload_phrase_library(&self, index: u8) -> bool {
        self.runtime
            .as_ref()
            .is_some_and(|runtime| runtime.unload_library(u32::from(index)))
    }

    /// Clone of the context's user store, if this context has one.
    #[must_use]
    pub fn user_store(&self) -> Option<UserStore> {
        self.user.clone()
    }
}

#[cfg(test)]
mod open_failure_tests {

    use super::{ContextCore, OpenFailure};
    use crate::PINYIN_DEFAULT_OPTION_WORD as WORD;
    use oxpinyin_runtime::OpenError;
    use oxpinyin_user::UserConfLaw;

    #[test]
    fn missing_system_dir_carries_the_runtime_error_and_path() {
        let dir =
            std::env::temp_dir().join(format!("oxpinyin-facade-missing-{}", std::process::id()));
        let failure = ContextCore::try_open(&dir, None, WORD, UserConfLaw::Pinyin)
            .err()
            .expect("a missing system dir cannot open");
        let OpenFailure::Runtime(error) = &failure else {
            panic!("expected a runtime failure, got {failure:?}");
        };
        assert!(matches!(error, OpenError::Missing(_)), "got {error:?}");
        assert!(
            failure
                .to_string()
                .contains(dir.to_str().expect("UTF-8 temp path")),
            "the message names the path: {failure}"
        );
        assert!(std::error::Error::source(&failure).is_some());
    }
}
