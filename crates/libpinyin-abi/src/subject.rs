//! A dynamically loaded libpinyin or libzhuyin and a checked subset of its
//! public operations.
//!
//! A [`Subject`] is a shared object opened by path. It is deliberately
//! ignorant of what it is being compared with and of any pin: it reports what
//! the library does and which symbols it lacks, and leaves the verdict to the
//! caller.
//!
//! # Scope
//!
//! This is the first vertical slice, not the ABI. It covers context lifecycle,
//! options, parsing, the sentence guess and the candidate list for each
//! facade; everything else the headers declare is reached through later,
//! separately reviewed work. A symbol the loaded library does not export makes
//! only the operations that need it fail with
//! [`AbiError::SymbolMissing`]; the library itself is never rejected for lacking
//! a symbol some other revision has ([`Subject::missing_symbols`] lists them).
//!
//! # Process isolation
//!
//! Differential runs keep one library per process, so a crash or abort in one
//! subject cannot take the other's observation with it and the libraries'
//! global state (GLib, locale, allocator, function-static tables) never mixes.
//! [`Subject::open`] therefore refuses to open a second subject in a process
//! that already has one; [`Subject::open_additional`] is the explicit opt-out
//! for callers that know the two libraries do not interfere. Neither call tries
//! to mask an abort the library itself raises (some revisions assert on inputs
//! such as an apostrophe-only string): run untrusted inputs in a child process.
//!
//! The rule is about *live* subjects. A shared object is never unloaded (see
//! below), so a subject opened after another was dropped finds the library's
//! process-global state as the earlier one left it. Anything that must start
//! from a clean library state needs a fresh process, not just a fresh
//! [`Subject`].
//!
//! # Threads
//!
//! libpinyin is not thread-safe: it keeps function-static state, and the
//! pinned revision aborts when several threads each open a context. A
//! [`Subject`] and everything derived from it ([`Context`], the sessions) is
//! therefore neither `Send` nor `Sync`, and a compile-time test pins that.
//!
//! # Safety model
//!
//! Every symbol is resolved once, as a typed function pointer, when the subject
//! opens. The library handle is **never unloaded**: libpinyin and GLib keep
//! process-global state, and unloading code whose function pointers, callbacks
//! or static destructors may still be reachable is a hazard that buys nothing
//! for a short-lived tool. The cost is one leaked loader reference per
//! [`Subject`] opened, and the function pointers stay valid for the life of the
//! process. Handles are owned by RAII values that borrow what they came from
//! ([`Subject`] → [`Context`] → session), so they cannot outlive it and are
//! released in the right order.
//!
//! An owned handle is created only if it can be released. `*_fini` is resolved
//! before `*_init` runs and `*_free_instance` before `*_alloc_instance` does; a
//! library missing either fails the constructor with
//! [`AbiError::SymbolMissing`] having created nothing.
//!
//! Strings the library lends (`*_get_candidate_string` points into the
//! candidate the instance holds) are copied before the next call. The sentence
//! string is caller-owned: the public headers say "the returned sentence should
//! be freed by g_free()". It is copied and released with the `g_free` found as
//! follows. First the subject itself is asked (a symbol lookup on a library
//! handle also searches the libraries it depends on on ELF platforms, so this
//! yields the GLib the library was linked against; checked for the pinned
//! libpinyin/libzhuyin and oxpinyin's on Linux). Only if that finds nothing is
//! a GLib looked up by its usual name for the platform; that may be a different
//! GLib from the library's if several are installed, which matters little
//! because GLib 2.46 and later route `g_malloc`/`g_free` to the system
//! allocator, and is reported rather than hidden: with no `g_free` at all,
//! reading a sentence fails with `SymbolMissing { name: "g_free" }` *before*
//! the library is asked, so nothing is leaked. The fallback names are untested
//! on macOS and Windows.
//!
//! The one thing a safe wrapper cannot check is that the file really is a
//! libpinyin of the facade requested: opening a shared object runs its
//! initialisers, and calling a differently shaped function through a wrong
//! signature is undefined behaviour. Naming the subject is therefore the
//! caller's responsibility, as it is for any `dlopen`.

use core::ffi::{CStr, c_char, c_int, c_void};
use core::marker::PhantomData;
use core::ptr::{self, NonNull};
use std::ffi::CString;
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use libloading::Library;

use crate::error::AbiError;
use crate::observation::{CandidateInfo, CandidateType, Observation, ZhuyinCandidateType};

/// Which public facade of the family a library implements.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Facade {
    /// `libpinyin`: the `pinyin_*` functions.
    Pinyin,
    /// `libzhuyin`: the `zhuyin_*` functions.
    Zhuyin,
}

type InitFn = unsafe extern "C" fn(*const c_char, *const c_char) -> *mut c_void;
type VoidHandleFn = unsafe extern "C" fn(*mut c_void);
type SetOptionsFn = unsafe extern "C" fn(*mut c_void, u32) -> bool;
type AllocFn = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type HandleBoolFn = unsafe extern "C" fn(*mut c_void) -> bool;
type ParseFn = unsafe extern "C" fn(*mut c_void, *const c_char) -> usize;
type LengthFn = unsafe extern "C" fn(*mut c_void) -> usize;
type PinyinSentenceFn = unsafe extern "C" fn(*mut c_void, u8, *mut *mut c_char) -> bool;
type ZhuyinSentenceFn = unsafe extern "C" fn(*mut c_void, *mut *mut c_char) -> bool;
type PinyinCandidatesFn = unsafe extern "C" fn(*mut c_void, usize, u32) -> bool;
type ZhuyinCandidatesFn = unsafe extern "C" fn(*mut c_void, usize) -> bool;
type CountFn = unsafe extern "C" fn(*mut c_void, *mut u32) -> bool;
type CandidateFn = unsafe extern "C" fn(*mut c_void, u32, *mut *mut c_void) -> bool;
type CandidateTypeFn = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_int) -> bool;
type CandidateStringFn = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut *const c_char) -> bool;
type NbestIndexFn = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut u8) -> bool;

/// Where exported symbols come from. A loaded library in production; an
/// in-process table in the tests, which is what lets the ownership rules be
/// checked on every platform without a libpinyin.
trait SymbolSource {
    /// Looks `name` up as a `T`.
    ///
    /// # Safety
    ///
    /// `T` must be the exact function-pointer type of the symbol `name`.
    unsafe fn lookup<T: Copy>(&self, name: &str) -> Option<T>;
}

impl SymbolSource for Library {
    unsafe fn lookup<T: Copy>(&self, name: &str) -> Option<T> {
        // SAFETY: the caller guarantees `T` is the symbol's type. The symbol is
        // copied out at once; the library is never unloaded (module docs), so
        // the copy stays valid.
        unsafe { self.get::<T>(name.as_bytes()).ok().map(|symbol| *symbol) }
    }
}

/// Declares a table of optional typed symbols for one facade.
///
/// Each field is named after its symbol without the facade prefix, so the
/// symbol name is derived rather than typed twice.
macro_rules! symbol_table {
    ($(#[$meta:meta])* $name:ident, $prefix:literal, { $($field:ident: $ty:ty,)* }) => {
        $(#[$meta])*
        #[derive(Clone, Copy)]
        struct $name {
            $($field: Option<$ty>,)*
        }

        impl $name {
            /// Resolves every symbol of the table.
            ///
            /// # Safety
            ///
            /// Each field's type must be the real type of the symbol it names
            /// in the library behind `source`.
            unsafe fn resolve<S: SymbolSource>(source: &S) -> Self {
                Self {
                    // SAFETY: forwarded caller contract, field by field.
                    $($field: unsafe {
                        source.lookup::<$ty>(concat!($prefix, stringify!($field)))
                    },)*
                }
            }

            /// Names of the symbols the library does not export.
            fn missing(&self) -> Vec<&'static str> {
                let mut missing = Vec::new();
                $(if self.$field.is_none() {
                    missing.push(concat!($prefix, stringify!($field)));
                })*
                missing
            }

            $(fn $field(&self) -> Result<$ty, AbiError> {
                self.$field.ok_or(AbiError::SymbolMissing {
                    name: concat!($prefix, stringify!($field)),
                })
            })*
        }
    };
}

symbol_table! {
    /// The libpinyin subset.
    PinyinFns, "pinyin_", {
        init: InitFn,
        fini: VoidHandleFn,
        set_options: SetOptionsFn,
        alloc_instance: AllocFn,
        free_instance: VoidHandleFn,
        reset: HandleBoolFn,
        parse_more_full_pinyins: ParseFn,
        get_parsed_input_length: LengthFn,
        guess_sentence: HandleBoolFn,
        get_sentence: PinyinSentenceFn,
        guess_candidates: PinyinCandidatesFn,
        get_n_candidate: CountFn,
        get_candidate: CandidateFn,
        get_candidate_type: CandidateTypeFn,
        get_candidate_string: CandidateStringFn,
        get_candidate_nbest_index: NbestIndexFn,
    }
}

symbol_table! {
    /// The libzhuyin subset.
    ZhuyinFns, "zhuyin_", {
        init: InitFn,
        fini: VoidHandleFn,
        set_options: SetOptionsFn,
        alloc_instance: AllocFn,
        free_instance: VoidHandleFn,
        reset: HandleBoolFn,
        parse_more_full_pinyins: ParseFn,
        parse_more_chewings: ParseFn,
        get_parsed_input_length: LengthFn,
        guess_sentence: HandleBoolFn,
        get_sentence: ZhuyinSentenceFn,
        guess_candidates_after_cursor: ZhuyinCandidatesFn,
        get_n_candidate: CountFn,
        get_candidate: CandidateFn,
        get_candidate_type: CandidateTypeFn,
        get_candidate_string: CandidateStringFn,
    }
}

#[derive(Clone, Copy)]
enum Api {
    Pinyin(PinyinFns),
    Zhuyin(ZhuyinFns),
}

impl Api {
    const fn facade(&self) -> Facade {
        match self {
            Self::Pinyin(_) => Facade::Pinyin,
            Self::Zhuyin(_) => Facade::Zhuyin,
        }
    }

    fn init(&self) -> Result<InitFn, AbiError> {
        match self {
            Self::Pinyin(f) => f.init(),
            Self::Zhuyin(f) => f.init(),
        }
    }

    fn fini(&self) -> Result<VoidHandleFn, AbiError> {
        match self {
            Self::Pinyin(f) => f.fini(),
            Self::Zhuyin(f) => f.fini(),
        }
    }

    fn set_options(&self) -> Result<SetOptionsFn, AbiError> {
        match self {
            Self::Pinyin(f) => f.set_options(),
            Self::Zhuyin(f) => f.set_options(),
        }
    }
}

/// `g_free`, as exported by the GLib the subject itself uses.
type GFreeFn = unsafe extern "C" fn(*mut c_void);

/// Counts the subjects alive in a process.
struct Slots(AtomicUsize);

/// The process-wide count of live subjects.
static LIVE_SUBJECTS: Slots = Slots::new();

impl Slots {
    const fn new() -> Self {
        Self(AtomicUsize::new(0))
    }

    /// Takes the only slot, or fails if any is taken.
    fn exclusive(&'static self) -> Result<OpenGuard, AbiError> {
        self.0
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| OpenGuard(self))
            .map_err(|_| AbiError::SubjectAlreadyOpen)
    }

    /// Adds a slot alongside whatever is already taken.
    fn additional(&'static self) -> OpenGuard {
        self.0.fetch_add(1, Ordering::AcqRel);
        OpenGuard(self)
    }
}

/// Holds one slot for as long as its subject exists.
struct OpenGuard(&'static Slots);

impl Drop for OpenGuard {
    fn drop(&mut self) {
        self.0.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// A libpinyin or libzhuyin shared object, loaded by path.
pub struct Subject {
    api: Api,
    path: PathBuf,
    g_free: Option<GFreeFn>,
    missing: Vec<&'static str>,
    // Never unloaded (module docs). Wrapped so a drop cannot `dlclose`.
    _libraries: Vec<ManuallyDrop<Library>>,
    _slot: Option<OpenGuard>,
    // libpinyin is not thread-safe, so no handle derived from a subject may
    // cross threads; the raw-pointer marker makes the whole chain `!Send` and
    // `!Sync` (a test pins this).
    _single_threaded: PhantomData<*const ()>,
}

impl core::fmt::Debug for Subject {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("Subject")
            .field("facade", &self.api.facade())
            .field("path", &self.path)
            .field("missing", &self.missing)
            .finish_non_exhaustive()
    }
}

/// Platform names under which a GLib shared object is looked for when the
/// subject's own dependency chain does not provide `g_free`.
const GLIB_NAMES: &[&str] = &[
    "libglib-2.0.so.0",
    "libglib-2.0.0.dylib",
    "libglib-2.0-0.dll",
];

impl Subject {
    /// Opens the shared object at `path` as the given facade.
    ///
    /// # Errors
    ///
    /// [`AbiError::SubjectAlreadyOpen`] when this process already holds a
    /// subject, and [`AbiError::LibraryLoad`] when the loader refuses the file.
    /// A library that lacks symbols still opens; see [`Self::missing_symbols`].
    pub fn open(path: impl AsRef<Path>, facade: Facade) -> Result<Self, AbiError> {
        Self::open_in(&LIVE_SUBJECTS, path.as_ref(), facade, true)
    }

    /// Opens a subject alongside one that is already open in this process.
    ///
    /// This is the explicit opt-out of the one-library-per-process rule. Use it
    /// only for libraries known not to interfere (for example the pinyin and
    /// zhuyin halves of one build); never for the two sides of a differential.
    ///
    /// # Errors
    ///
    /// [`AbiError::LibraryLoad`] when the loader refuses the file.
    pub fn open_additional(path: impl AsRef<Path>, facade: Facade) -> Result<Self, AbiError> {
        Self::open_in(&LIVE_SUBJECTS, path.as_ref(), facade, false)
    }

    /// Takes a slot from `slots` (the only one when `exclusive`), then loads.
    /// The slot is released if loading fails.
    fn open_in(
        slots: &'static Slots,
        path: &Path,
        facade: Facade,
        exclusive: bool,
    ) -> Result<Self, AbiError> {
        let slot = if exclusive {
            slots.exclusive()?
        } else {
            slots.additional()
        };
        Self::load(path, facade, slot)
    }

    fn load(path: &Path, facade: Facade, slot: OpenGuard) -> Result<Self, AbiError> {
        // SAFETY: loading a shared object runs its initialisers. The caller
        // named this file as the subject on purpose; this is the dlopen
        // contract, restated in the module documentation.
        let library = unsafe { Library::new(path) }.map_err(|error| AbiError::LibraryLoad {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })?;

        // SAFETY: the symbol tables declare the types of the public headers for
        // the facade being opened; `facade` is the caller's statement of what
        // this file is.
        let api = unsafe {
            match facade {
                Facade::Pinyin => Api::Pinyin(PinyinFns::resolve(&library)),
                Facade::Zhuyin => Api::Zhuyin(ZhuyinFns::resolve(&library)),
            }
        };

        // The `g_free` that matches the library's allocations is the one its
        // own dependency chain resolves; a separately named GLib is the
        // fallback for platforms where a symbol lookup does not search
        // dependencies.
        // SAFETY: `g_free` is `void g_free(gpointer)`.
        let mut g_free = unsafe { library.lookup::<GFreeFn>("g_free") };
        let mut libraries = vec![ManuallyDrop::new(library)];
        if g_free.is_none() {
            for name in GLIB_NAMES {
                // SAFETY: as above; a GLib's initialisers are benign.
                if let Ok(glib) = unsafe { Library::new(name) } {
                    // SAFETY: `g_free` is `void g_free(gpointer)`.
                    g_free = unsafe { glib.lookup::<GFreeFn>("g_free") };
                    libraries.push(ManuallyDrop::new(glib));
                    if g_free.is_some() {
                        break;
                    }
                }
            }
        }

        let missing = match &api {
            Api::Pinyin(fns) => fns.missing(),
            Api::Zhuyin(fns) => fns.missing(),
        };
        Ok(Self {
            api,
            path: path.to_path_buf(),
            g_free,
            missing,
            _libraries: libraries,
            _slot: Some(slot),
            _single_threaded: PhantomData,
        })
    }

    /// The facade this subject was opened as.
    #[must_use]
    pub const fn facade(&self) -> Facade {
        self.api.facade()
    }

    /// The path the library was loaded from.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The symbols of this crate's checked subset that the library does not
    /// export. Empty for a library that has everything the subset needs.
    ///
    /// `g_free` is not listed here: it is needed only to release the sentence
    /// strings, and [`PinyinSession::sentence`] reports its absence itself.
    #[must_use]
    pub fn missing_symbols(&self) -> &[&'static str] {
        &self.missing
    }

    /// Creates a context over `system_dir` and `user_dir` (`*_init`).
    ///
    /// The user directory is the library's to write: pass a fresh, empty one for
    /// a protocol that must start from no learned state.
    ///
    /// # Errors
    ///
    /// [`AbiError::SymbolMissing`] if the library has no `*_init`,
    /// [`AbiError::PathNotRepresentable`] or [`AbiError::InteriorNul`] for a
    /// path that cannot cross the C boundary, and
    /// [`AbiError::ContextInitFailed`] if the library returns NULL.
    pub fn open_context(
        &self,
        system_dir: &Path,
        user_dir: &Path,
    ) -> Result<Context<'_>, AbiError> {
        // The destructor is checked before the constructor runs: no owned
        // handle is ever created that cannot be released.
        let fini = self.api.fini()?;
        let init = self.api.init()?;
        let system = path_to_cstring(system_dir)?;
        let user = path_to_cstring(user_dir)?;
        // SAFETY: `init` is the library's `*_init`; both arguments are valid
        // NUL-terminated strings that outlive the call.
        let raw = unsafe { init(system.as_ptr(), user.as_ptr()) };
        let handle = NonNull::new(raw).ok_or_else(|| AbiError::ContextInitFailed {
            system_dir: system_dir.to_path_buf(),
            user_dir: user_dir.to_path_buf(),
        })?;
        Ok(Context {
            subject: self,
            handle,
            fini,
        })
    }
}

fn path_to_cstring(path: &Path) -> Result<CString, AbiError> {
    let text = path
        .to_str()
        .ok_or_else(|| AbiError::PathNotRepresentable {
            path: path.to_path_buf(),
        })?;
    CString::new(text).map_err(|_| AbiError::InteriorNul { what: "path" })
}

fn input_to_cstring(input: &[u8]) -> Result<CString, AbiError> {
    CString::new(input).map_err(|_| AbiError::InteriorNul { what: "input" })
}

/// Copies a library-owned C string into an owned `String`.
///
/// # Safety
///
/// `text` must be non-null and point to a NUL-terminated string that stays
/// valid for the duration of the call.
unsafe fn copy_string(text: *const c_char, function: &'static str) -> Result<String, AbiError> {
    // SAFETY: the caller guarantees `text` is a valid NUL-terminated string.
    let bytes = unsafe { CStr::from_ptr(text) };
    bytes
        .to_str()
        .map(str::to_owned)
        .map_err(|_| AbiError::NonUtf8 { function })
}

/// A live `*_init` context. Releases it (`*_fini`) when dropped.
pub struct Context<'s> {
    subject: &'s Subject,
    handle: NonNull<c_void>,
    // Resolved before the context was created, so a context can always be
    // released: one that could not be is never made.
    fini: VoidHandleFn,
}

impl core::fmt::Debug for Context<'_> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("Context")
            .field("facade", &self.subject.facade())
            .finish_non_exhaustive()
    }
}

impl Context<'_> {
    /// Sets the option word (`*_set_options`).
    ///
    /// The word is passed through unchanged: which bits are acceptable is the
    /// caller's policy, not this crate's.
    ///
    /// # Errors
    ///
    /// [`AbiError::SymbolMissing`] without `*_set_options`, and
    /// [`AbiError::Call`] if the library refuses the word.
    pub fn set_options(&self, options: u32) -> Result<(), AbiError> {
        let set_options = self.subject.api.set_options()?;
        // SAFETY: `handle` is the live context this value owns.
        let accepted = unsafe { set_options(self.handle.as_ptr(), options) };
        if accepted {
            Ok(())
        } else {
            Err(AbiError::Call {
                function: match self.subject.facade() {
                    Facade::Pinyin => "pinyin_set_options",
                    Facade::Zhuyin => "zhuyin_set_options",
                },
            })
        }
    }

    /// Allocates a libpinyin instance on this context.
    ///
    /// # Errors
    ///
    /// [`AbiError::FacadeMismatch`] on a libzhuyin context,
    /// [`AbiError::SymbolMissing`] without the subset's symbols, and
    /// [`AbiError::InstanceAllocFailed`] if the library returns NULL.
    pub fn pinyin_session(&self) -> Result<PinyinSession<'_>, AbiError> {
        let Api::Pinyin(fns) = self.subject.api else {
            return Err(AbiError::FacadeMismatch {
                context: Facade::Zhuyin,
                requested: Facade::Pinyin,
            });
        };
        // Resolve the destructor and the allocator up front, so a library
        // missing a symbol fails here, before an instance exists to leak.
        let free = fns.free_instance()?;
        let alloc = fns.alloc_instance()?;
        // SAFETY: `handle` is the live context this value owns.
        let raw = unsafe { alloc(self.handle.as_ptr()) };
        let instance = NonNull::new(raw).ok_or(AbiError::InstanceAllocFailed)?;
        Ok(PinyinSession {
            fns,
            g_free: self.subject.g_free,
            instance,
            free,
            _context: PhantomData,
        })
    }

    /// Allocates a libzhuyin instance on this context.
    ///
    /// # Errors
    ///
    /// As [`Self::pinyin_session`], with the facades swapped.
    pub fn zhuyin_session(&self) -> Result<ZhuyinSession<'_>, AbiError> {
        let Api::Zhuyin(fns) = self.subject.api else {
            return Err(AbiError::FacadeMismatch {
                context: Facade::Pinyin,
                requested: Facade::Zhuyin,
            });
        };
        let free = fns.free_instance()?;
        let alloc = fns.alloc_instance()?;
        // SAFETY: `handle` is the live context this value owns.
        let raw = unsafe { alloc(self.handle.as_ptr()) };
        let instance = NonNull::new(raw).ok_or(AbiError::InstanceAllocFailed)?;
        Ok(ZhuyinSession {
            fns,
            g_free: self.subject.g_free,
            instance,
            free,
            _context: PhantomData,
        })
    }
}

impl Drop for Context<'_> {
    fn drop(&mut self) {
        // SAFETY: `handle` is the live context this value owns, released
        // exactly once, after every session that borrowed it.
        unsafe { (self.fini)(self.handle.as_ptr()) };
    }
}

/// One candidate of a libpinyin list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PinyinCandidate {
    /// The candidate's UTF-8 string.
    pub text: String,
    /// The raw `lookup_candidate_type_t` the library reported, unfiltered.
    pub raw_type: i32,
    /// The n-best index, read only for an n-best candidate (the accessor
    /// asserts that type) and only when the library exports it.
    pub nbest_index: Option<u8>,
}

impl PinyinCandidate {
    /// The type as libpinyin's enumeration names it, or `None` for a value the
    /// enumeration does not declare.
    #[must_use]
    pub const fn candidate_type(&self) -> Option<CandidateType> {
        CandidateType::from_raw(self.raw_type)
    }

    /// The typed record, when the raw type is one libpinyin declares.
    #[must_use]
    pub fn info(&self) -> Option<CandidateInfo> {
        Some(CandidateInfo {
            text: self.text.clone(),
            candidate_type: self.candidate_type()?,
            nbest_index: self.nbest_index,
        })
    }
}

/// One candidate of a libzhuyin list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZhuyinCandidate {
    /// The candidate's UTF-8 string.
    pub text: String,
    /// The raw `lookup_candidate_type_t` the library reported, unfiltered.
    pub raw_type: i32,
}

impl ZhuyinCandidate {
    /// The type as libzhuyin's enumeration names it, or `None` for a value the
    /// enumeration does not declare.
    #[must_use]
    pub const fn candidate_type(&self) -> Option<ZhuyinCandidateType> {
        ZhuyinCandidateType::from_raw(self.raw_type)
    }
}

/// Reads the sentence the last successful guess produced, then releases it.
///
/// # Safety
///
/// `read` must be the library's `*_get_sentence` bound to a live instance, and
/// `g_free` must release memory that library allocates.
unsafe fn take_sentence(
    g_free: GFreeFn,
    function: &'static str,
    read: impl FnOnce(*mut *mut c_char) -> bool,
) -> Result<Option<String>, AbiError> {
    let mut sentence: *mut c_char = ptr::null_mut();
    if !read(&raw mut sentence) || sentence.is_null() {
        return Ok(None);
    }
    // SAFETY: the library reported success and returned a non-null,
    // NUL-terminated, caller-owned string.
    let copied = unsafe { copy_string(sentence, function) };
    // SAFETY: the string is caller-owned and allocated by this library's GLib;
    // it is released exactly once, after the copy.
    unsafe { g_free(sentence.cast()) };
    copied.map(Some)
}

/// One libpinyin instance on a [`Context`]. Freed when dropped.
pub struct PinyinSession<'c> {
    fns: PinyinFns,
    g_free: Option<GFreeFn>,
    instance: NonNull<c_void>,
    // Resolved before the instance was allocated (see `Context`).
    free: VoidHandleFn,
    _context: PhantomData<&'c ()>,
}

impl PinyinSession<'_> {
    /// Clears the instance (`pinyin_reset`).
    ///
    /// # Errors
    ///
    /// [`AbiError::SymbolMissing`] or [`AbiError::Call`].
    pub fn reset(&mut self) -> Result<(), AbiError> {
        let reset = self.fns.reset()?;
        // SAFETY: `instance` is the live instance this value owns.
        if unsafe { reset(self.instance.as_ptr()) } {
            Ok(())
        } else {
            Err(AbiError::Call {
                function: "pinyin_reset",
            })
        }
    }

    /// Appends `input` as full pinyin and returns the bytes consumed
    /// (`pinyin_parse_more_full_pinyins`).
    ///
    /// # Errors
    ///
    /// [`AbiError::InteriorNul`] if `input` has a NUL byte, or
    /// [`AbiError::SymbolMissing`].
    pub fn parse_full_pinyin(&mut self, input: &[u8]) -> Result<usize, AbiError> {
        let parse = self.fns.parse_more_full_pinyins()?;
        let text = input_to_cstring(input)?;
        // SAFETY: live instance; `text` is NUL-terminated and outlives the call.
        Ok(unsafe { parse(self.instance.as_ptr(), text.as_ptr()) })
    }

    /// The parsed prefix length (`pinyin_get_parsed_input_length`).
    ///
    /// # Errors
    ///
    /// [`AbiError::SymbolMissing`].
    pub fn parsed_input_length(&self) -> Result<usize, AbiError> {
        let length = self.fns.get_parsed_input_length()?;
        // SAFETY: `instance` is the live instance this value owns.
        Ok(unsafe { length(self.instance.as_ptr()) })
    }

    /// Runs the sentence guess (`pinyin_guess_sentence`); `false` is the
    /// library's own answer, not an error.
    ///
    /// # Errors
    ///
    /// [`AbiError::SymbolMissing`].
    pub fn guess_sentence(&mut self) -> Result<bool, AbiError> {
        let guess = self.fns.guess_sentence()?;
        // SAFETY: `instance` is the live instance this value owns.
        Ok(unsafe { guess(self.instance.as_ptr()) })
    }

    /// Reads the n-best sentence at `index` (`pinyin_get_sentence`) and
    /// releases the string the library hands over.
    ///
    /// Some revisions assert on an `index` at or past the number of results, so
    /// only index 0 is safe after a successful guess; read higher ones only
    /// when the candidate list proves they exist.
    ///
    /// Returns `None` when the library reports no sentence.
    ///
    /// # Errors
    ///
    /// [`AbiError::SymbolMissing`] for `pinyin_get_sentence` or for `g_free`
    /// (checked before the call, so nothing is leaked), and
    /// [`AbiError::NonUtf8`].
    pub fn sentence(&mut self, index: u8) -> Result<Option<String>, AbiError> {
        let get = self.fns.get_sentence()?;
        let g_free = self
            .g_free
            .ok_or(AbiError::SymbolMissing { name: "g_free" })?;
        let instance = self.instance.as_ptr();
        // SAFETY: `get` is this library's `pinyin_get_sentence` bound to the
        // live instance, and `g_free` is the free of the GLib it allocates
        // with.
        unsafe {
            take_sentence(g_free, "pinyin_get_sentence", |out| {
                get(instance, index, out)
            })
        }
    }

    /// Builds the candidate list at `offset` under `sort`
    /// (`pinyin_guess_candidates`).
    ///
    /// # Errors
    ///
    /// [`AbiError::SymbolMissing`].
    pub fn guess_candidates(&mut self, offset: usize, sort: u32) -> Result<bool, AbiError> {
        let guess = self.fns.guess_candidates()?;
        // SAFETY: `instance` is the live instance this value owns.
        Ok(unsafe { guess(self.instance.as_ptr(), offset, sort) })
    }

    /// The uncapped candidate count (`pinyin_get_n_candidate`).
    ///
    /// # Errors
    ///
    /// [`AbiError::SymbolMissing`] or [`AbiError::Call`].
    pub fn candidate_count(&self) -> Result<u32, AbiError> {
        let count = self.fns.get_n_candidate()?;
        let mut total = 0_u32;
        // SAFETY: live instance; `total` is a valid out-pointer.
        if unsafe { count(self.instance.as_ptr(), &raw mut total) } {
            Ok(total)
        } else {
            Err(AbiError::Call {
                function: "pinyin_get_n_candidate",
            })
        }
    }

    /// Reads the candidate at `index`, copying everything it lends.
    ///
    /// # Errors
    ///
    /// [`AbiError::SymbolMissing`], [`AbiError::Call`] when the library refuses
    /// or returns NULL, and [`AbiError::NonUtf8`].
    pub fn candidate(&self, index: u32) -> Result<PinyinCandidate, AbiError> {
        let get = self.fns.get_candidate()?;
        let get_type = self.fns.get_candidate_type()?;
        let get_string = self.fns.get_candidate_string()?;
        let instance = self.instance.as_ptr();

        let mut handle: *mut c_void = ptr::null_mut();
        // SAFETY: live instance; `handle` is a valid out-pointer.
        if !unsafe { get(instance, index, &raw mut handle) } || handle.is_null() {
            return Err(AbiError::Call {
                function: "pinyin_get_candidate",
            });
        }
        let mut raw_type: c_int = 0;
        // SAFETY: `handle` was just lent by this instance and is not retained.
        if !unsafe { get_type(instance, handle, &raw mut raw_type) } {
            return Err(AbiError::Call {
                function: "pinyin_get_candidate_type",
            });
        }
        let mut text: *const c_char = ptr::null();
        // SAFETY: as above; the string is lent, valid until the next call.
        if !unsafe { get_string(instance, handle, &raw mut text) } || text.is_null() {
            return Err(AbiError::Call {
                function: "pinyin_get_candidate_string",
            });
        }
        // SAFETY: the library reported success and a non-null lent string.
        let text = unsafe { copy_string(text, "pinyin_get_candidate_string") }?;

        // The accessor asserts the candidate is an n-best one, so it is asked
        // only of those.
        let nbest_index = match (
            CandidateType::from_raw(raw_type),
            self.fns.get_candidate_nbest_index().ok(),
        ) {
            (Some(CandidateType::NbestMatch), Some(get_nbest)) => {
                let mut index = 0_u8;
                // SAFETY: as above; the type check guards the assertion.
                unsafe { get_nbest(instance, handle, &raw mut index) }.then_some(index)
            }
            _ => None,
        };
        Ok(PinyinCandidate {
            text,
            raw_type,
            nbest_index,
        })
    }

    /// Resets, parses `input`, guesses the sentence, builds the candidate list
    /// and returns the first `max_candidates`, in the order a frontend drives
    /// them (sentence guess before candidates).
    ///
    /// When nothing was parsed the sentence and candidates are left empty
    /// rather than queried.
    ///
    /// # Errors
    ///
    /// As the individual calls.
    pub fn observe(
        &mut self,
        input: &[u8],
        sort: u32,
        max_candidates: usize,
    ) -> Result<Observation<PinyinCandidate>, AbiError> {
        self.reset()?;
        let parse_return = self.parse_full_pinyin(input)?;
        let parsed_input_length = self.parsed_input_length()?;
        let mut observation = Observation {
            input: input.to_vec(),
            parse_return,
            parsed_input_length,
            sentence: None,
            candidate_total: 0,
            candidates: Vec::new(),
        };
        if parsed_input_length == 0 {
            return Ok(observation);
        }
        if self.guess_sentence()? {
            observation.sentence = self.sentence(0)?;
        }
        if self.guess_candidates(0, sort)? {
            observation.candidate_total = self.candidate_count()?;
            let shown = observation
                .candidate_total
                .min(u32::try_from(max_candidates).unwrap_or(u32::MAX));
            for index in 0..shown {
                observation.candidates.push(self.candidate(index)?);
            }
        }
        Ok(observation)
    }
}

impl Drop for PinyinSession<'_> {
    fn drop(&mut self) {
        // SAFETY: `instance` is the live instance this value owns, freed
        // exactly once, before the context it borrows.
        unsafe { (self.free)(self.instance.as_ptr()) };
    }
}

/// Which parse entry point feeds a libzhuyin instance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ZhuyinInput {
    /// `zhuyin_parse_more_full_pinyins`.
    FullPinyin,
    /// `zhuyin_parse_more_chewings`.
    Chewing,
}

/// One libzhuyin instance on a [`Context`]. Freed when dropped.
pub struct ZhuyinSession<'c> {
    fns: ZhuyinFns,
    g_free: Option<GFreeFn>,
    instance: NonNull<c_void>,
    // Resolved before the instance was allocated (see `Context`).
    free: VoidHandleFn,
    _context: PhantomData<&'c ()>,
}

impl ZhuyinSession<'_> {
    /// Clears the instance (`zhuyin_reset`).
    ///
    /// # Errors
    ///
    /// [`AbiError::SymbolMissing`] or [`AbiError::Call`].
    pub fn reset(&mut self) -> Result<(), AbiError> {
        let reset = self.fns.reset()?;
        // SAFETY: `instance` is the live instance this value owns.
        if unsafe { reset(self.instance.as_ptr()) } {
            Ok(())
        } else {
            Err(AbiError::Call {
                function: "zhuyin_reset",
            })
        }
    }

    /// Appends `input` through the chosen parse entry point and returns the
    /// bytes consumed.
    ///
    /// # Errors
    ///
    /// [`AbiError::InteriorNul`] or [`AbiError::SymbolMissing`].
    pub fn parse(&mut self, kind: ZhuyinInput, input: &[u8]) -> Result<usize, AbiError> {
        let parse = match kind {
            ZhuyinInput::FullPinyin => self.fns.parse_more_full_pinyins()?,
            ZhuyinInput::Chewing => self.fns.parse_more_chewings()?,
        };
        let text = input_to_cstring(input)?;
        // SAFETY: live instance; `text` is NUL-terminated and outlives the call.
        Ok(unsafe { parse(self.instance.as_ptr(), text.as_ptr()) })
    }

    /// The parsed prefix length (`zhuyin_get_parsed_input_length`).
    ///
    /// # Errors
    ///
    /// [`AbiError::SymbolMissing`].
    pub fn parsed_input_length(&self) -> Result<usize, AbiError> {
        let length = self.fns.get_parsed_input_length()?;
        // SAFETY: `instance` is the live instance this value owns.
        Ok(unsafe { length(self.instance.as_ptr()) })
    }

    /// Runs the sentence guess (`zhuyin_guess_sentence`).
    ///
    /// # Errors
    ///
    /// [`AbiError::SymbolMissing`].
    pub fn guess_sentence(&mut self) -> Result<bool, AbiError> {
        let guess = self.fns.guess_sentence()?;
        // SAFETY: `instance` is the live instance this value owns.
        Ok(unsafe { guess(self.instance.as_ptr()) })
    }

    /// Reads the sentence (`zhuyin_get_sentence`, which takes no index) and
    /// releases the string the library hands over.
    ///
    /// # Errors
    ///
    /// As [`PinyinSession::sentence`].
    pub fn sentence(&mut self) -> Result<Option<String>, AbiError> {
        let get = self.fns.get_sentence()?;
        let g_free = self
            .g_free
            .ok_or(AbiError::SymbolMissing { name: "g_free" })?;
        let instance = self.instance.as_ptr();
        // SAFETY: `get` is this library's `zhuyin_get_sentence` bound to the
        // live instance, and `g_free` is the free of the GLib it allocates with.
        unsafe { take_sentence(g_free, "zhuyin_get_sentence", |out| get(instance, out)) }
    }

    /// Builds the candidate list after `offset`
    /// (`zhuyin_guess_candidates_after_cursor`).
    ///
    /// # Errors
    ///
    /// [`AbiError::SymbolMissing`].
    pub fn guess_candidates_after_cursor(&mut self, offset: usize) -> Result<bool, AbiError> {
        let guess = self.fns.guess_candidates_after_cursor()?;
        // SAFETY: `instance` is the live instance this value owns.
        Ok(unsafe { guess(self.instance.as_ptr(), offset) })
    }

    /// The uncapped candidate count (`zhuyin_get_n_candidate`).
    ///
    /// # Errors
    ///
    /// [`AbiError::SymbolMissing`] or [`AbiError::Call`].
    pub fn candidate_count(&self) -> Result<u32, AbiError> {
        let count = self.fns.get_n_candidate()?;
        let mut total = 0_u32;
        // SAFETY: live instance; `total` is a valid out-pointer.
        if unsafe { count(self.instance.as_ptr(), &raw mut total) } {
            Ok(total)
        } else {
            Err(AbiError::Call {
                function: "zhuyin_get_n_candidate",
            })
        }
    }

    /// Reads the candidate at `index`, copying everything it lends.
    ///
    /// # Errors
    ///
    /// As [`PinyinSession::candidate`].
    pub fn candidate(&self, index: u32) -> Result<ZhuyinCandidate, AbiError> {
        let get = self.fns.get_candidate()?;
        let get_type = self.fns.get_candidate_type()?;
        let get_string = self.fns.get_candidate_string()?;
        let instance = self.instance.as_ptr();

        let mut handle: *mut c_void = ptr::null_mut();
        // SAFETY: live instance; `handle` is a valid out-pointer.
        if !unsafe { get(instance, index, &raw mut handle) } || handle.is_null() {
            return Err(AbiError::Call {
                function: "zhuyin_get_candidate",
            });
        }
        let mut raw_type: c_int = 0;
        // SAFETY: `handle` was just lent by this instance and is not retained.
        if !unsafe { get_type(instance, handle, &raw mut raw_type) } {
            return Err(AbiError::Call {
                function: "zhuyin_get_candidate_type",
            });
        }
        let mut text: *const c_char = ptr::null();
        // SAFETY: as above; the string is lent, valid until the next call.
        if !unsafe { get_string(instance, handle, &raw mut text) } || text.is_null() {
            return Err(AbiError::Call {
                function: "zhuyin_get_candidate_string",
            });
        }
        // SAFETY: the library reported success and a non-null lent string.
        let text = unsafe { copy_string(text, "zhuyin_get_candidate_string") }?;
        Ok(ZhuyinCandidate { text, raw_type })
    }

    /// Resets, parses `input`, guesses the sentence, builds the candidate list
    /// after the cursor and returns the first `max_candidates`.
    ///
    /// When nothing was parsed the sentence and candidates are left empty.
    ///
    /// # Errors
    ///
    /// As the individual calls.
    pub fn observe(
        &mut self,
        kind: ZhuyinInput,
        input: &[u8],
        max_candidates: usize,
    ) -> Result<Observation<ZhuyinCandidate>, AbiError> {
        self.reset()?;
        let parse_return = self.parse(kind, input)?;
        let parsed_input_length = self.parsed_input_length()?;
        let mut observation = Observation {
            input: input.to_vec(),
            parse_return,
            parsed_input_length,
            sentence: None,
            candidate_total: 0,
            candidates: Vec::new(),
        };
        if parsed_input_length == 0 {
            return Ok(observation);
        }
        if self.guess_sentence()? {
            observation.sentence = self.sentence()?;
        }
        if self.guess_candidates_after_cursor(0)? {
            observation.candidate_total = self.candidate_count()?;
            let shown = observation
                .candidate_total
                .min(u32::try_from(max_candidates).unwrap_or(u32::MAX));
            for index in 0..shown {
                observation.candidates.push(self.candidate(index)?);
            }
        }
        Ok(observation)
    }
}

impl Drop for ZhuyinSession<'_> {
    fn drop(&mut self) {
        // SAFETY: `instance` is the live instance this value owns, freed
        // exactly once, before the context it borrows.
        unsafe { (self.free)(self.instance.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::option::SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY as SORT;
    use std::cell::Cell;
    use std::collections::HashMap;

    // Per-thread counters: every test runs on its own thread, so the mock's
    // bookkeeping never leaks between tests.
    thread_local! {
        static CONTEXTS_FINISHED: Cell<usize> = const { Cell::new(0) };
        static INSTANCES_FREED: Cell<usize> = const { Cell::new(0) };
        static STRINGS_HANDED_OVER: Cell<usize> = const { Cell::new(0) };
        static STRINGS_FREED: Cell<usize> = const { Cell::new(0) };
        static NBEST_ASSERTS: Cell<usize> = const { Cell::new(0) };
        static SENTENCE_GUESSES: Cell<usize> = const { Cell::new(0) };
        static INITS: Cell<usize> = const { Cell::new(0) };
        static ALLOCS: Cell<usize> = const { Cell::new(0) };
    }

    fn bump(counter: &'static std::thread::LocalKey<Cell<usize>>) {
        counter.with(|value| value.set(value.get() + 1));
    }

    fn read(counter: &'static std::thread::LocalKey<Cell<usize>>) -> usize {
        counter.with(Cell::get)
    }

    /// The state behind a mock instance handle.
    struct Instance {
        parsed: usize,
        guessed: bool,
        candidates: Vec<(CString, c_int)>,
    }

    extern "C" fn init(system: *const c_char, _user: *const c_char) -> *mut c_void {
        bump(&INITS);
        // SAFETY: the code under test passes a valid NUL-terminated string.
        if unsafe { CStr::from_ptr(system) }.to_bytes() == b"fail" {
            return ptr::null_mut();
        }
        Box::into_raw(Box::new(0_u32)).cast()
    }

    extern "C" fn fini(context: *mut c_void) {
        bump(&CONTEXTS_FINISHED);
        // SAFETY: `context` came from `init` above and is finished once.
        drop(unsafe { Box::from_raw(context.cast::<u32>()) });
    }

    extern "C" fn set_options(_context: *mut c_void, options: u32) -> bool {
        options != 0xdead
    }

    extern "C" fn alloc_instance(_context: *mut c_void) -> *mut c_void {
        bump(&ALLOCS);
        Box::into_raw(Box::new(Instance {
            parsed: 0,
            guessed: false,
            candidates: Vec::new(),
        }))
        .cast()
    }

    extern "C" fn free_instance(instance: *mut c_void) {
        bump(&INSTANCES_FREED);
        // SAFETY: `instance` came from `alloc_instance` and is freed once.
        drop(unsafe { Box::from_raw(instance.cast::<Instance>()) });
    }

    /// Borrows the mock instance behind a handle.
    ///
    /// # Safety
    ///
    /// `instance` must come from `alloc_instance` and not be freed.
    unsafe fn state<'a>(instance: *mut c_void) -> &'a mut Instance {
        // SAFETY: forwarded caller contract.
        unsafe { &mut *instance.cast::<Instance>() }
    }

    extern "C" fn reset(instance: *mut c_void) -> bool {
        // SAFETY: a live mock instance.
        let state = unsafe { state(instance) };
        state.parsed = 0;
        state.guessed = false;
        state.candidates.clear();
        true
    }

    extern "C" fn parse(instance: *mut c_void, text: *const c_char) -> usize {
        // SAFETY: a live mock instance and a valid NUL-terminated string.
        let (state, bytes) = unsafe { (state(instance), CStr::from_ptr(text).to_bytes()) };
        state.parsed = bytes.iter().take_while(|b| b.is_ascii_lowercase()).count();
        state.parsed
    }

    extern "C" fn parsed_length(instance: *mut c_void) -> usize {
        // SAFETY: a live mock instance.
        unsafe { state(instance) }.parsed
    }

    extern "C" fn guess_sentence(instance: *mut c_void) -> bool {
        bump(&SENTENCE_GUESSES);
        // SAFETY: a live mock instance.
        let state = unsafe { state(instance) };
        state.guessed = state.parsed > 0;
        state.guessed
    }

    fn hand_over_sentence(instance: *mut c_void, out: *mut *mut c_char) -> bool {
        // SAFETY: a live mock instance.
        if !unsafe { state(instance) }.guessed {
            return false;
        }
        bump(&STRINGS_HANDED_OVER);
        // SAFETY: `out` is the valid out-pointer the code under test passes.
        unsafe { *out = CString::from(c"你好").into_raw() };
        true
    }

    extern "C" fn pinyin_get_sentence(
        instance: *mut c_void,
        _index: u8,
        out: *mut *mut c_char,
    ) -> bool {
        hand_over_sentence(instance, out)
    }

    extern "C" fn zhuyin_get_sentence(instance: *mut c_void, out: *mut *mut c_char) -> bool {
        hand_over_sentence(instance, out)
    }

    fn fill_candidates(instance: *mut c_void, types: [c_int; 3]) -> bool {
        // SAFETY: a live mock instance.
        let state = unsafe { state(instance) };
        if state.parsed == 0 {
            return false;
        }
        state.candidates = ["你好", "你", "尼"]
            .into_iter()
            .zip(types)
            .filter_map(|(text, kind)| Some((CString::new(text).ok()?, kind)))
            .collect();
        true
    }

    extern "C" fn pinyin_guess_candidates(
        instance: *mut c_void,
        _offset: usize,
        _sort: u32,
    ) -> bool {
        // n-best, normal, and a value libpinyin's enumeration does not declare.
        fill_candidates(instance, [1, 2, 99])
    }

    extern "C" fn zhuyin_guess_candidates(instance: *mut c_void, _offset: usize) -> bool {
        fill_candidates(instance, [1, 2, 3])
    }

    extern "C" fn n_candidate(instance: *mut c_void, total: *mut u32) -> bool {
        // SAFETY: a live mock instance and a valid out-pointer.
        unsafe { *total = u32::try_from(state(instance).candidates.len()).unwrap_or(0) };
        true
    }

    extern "C" fn candidate(instance: *mut c_void, index: u32, out: *mut *mut c_void) -> bool {
        // SAFETY: a live mock instance and a valid out-pointer.
        let count = unsafe { state(instance) }.candidates.len();
        let slot = usize::try_from(index).unwrap_or(usize::MAX);
        if slot >= count {
            return false;
        }
        // The handle is the 1-based position, opaque to the code under test.
        // SAFETY: valid out-pointer.
        unsafe { *out = (slot + 1) as *mut c_void };
        true
    }

    fn row<'a>(instance: *mut c_void, handle: *mut c_void) -> &'a (CString, c_int) {
        // SAFETY: a live mock instance; `handle` was produced by `candidate`.
        unsafe { &state(instance).candidates[(handle as usize) - 1] }
    }

    extern "C" fn candidate_type(
        instance: *mut c_void,
        handle: *mut c_void,
        out: *mut c_int,
    ) -> bool {
        // SAFETY: valid out-pointer.
        unsafe { *out = row(instance, handle).1 };
        true
    }

    extern "C" fn candidate_string(
        instance: *mut c_void,
        handle: *mut c_void,
        out: *mut *const c_char,
    ) -> bool {
        // SAFETY: valid out-pointer; the row outlives the call.
        unsafe { *out = row(instance, handle).0.as_ptr() };
        true
    }

    extern "C" fn nbest_index(instance: *mut c_void, handle: *mut c_void, out: *mut u8) -> bool {
        if row(instance, handle).1 != 1 {
            // The real accessor asserts; the mock counts the violation.
            bump(&NBEST_ASSERTS);
            return false;
        }
        // SAFETY: valid out-pointer.
        unsafe { *out = 0 };
        true
    }

    extern "C" fn g_free(pointer: *mut c_void) {
        bump(&STRINGS_FREED);
        // SAFETY: only strings produced by `hand_over_sentence` reach here.
        drop(unsafe { CString::from_raw(pointer.cast()) });
    }

    /// An in-process stand-in for an exported-symbol table.
    struct Mock(HashMap<&'static str, usize>);

    impl SymbolSource for Mock {
        unsafe fn lookup<T: Copy>(&self, name: &str) -> Option<T> {
            assert_eq!(size_of::<T>(), size_of::<usize>(), "{name}");
            let address = *self.0.get(name)?;
            // SAFETY: `T` is a function-pointer type of pointer size (asserted)
            // and `address` was taken from a function of that signature below.
            Some(unsafe { core::mem::transmute_copy::<usize, T>(&address) })
        }
    }

    fn common(prefix: &'static str) -> Vec<(String, usize)> {
        let named = |suffix: &str, address: usize| (format!("{prefix}{suffix}"), address);
        vec![
            named("init", init as *const () as usize),
            named("fini", fini as *const () as usize),
            named("set_options", set_options as *const () as usize),
            named("alloc_instance", alloc_instance as *const () as usize),
            named("free_instance", free_instance as *const () as usize),
            named("reset", reset as *const () as usize),
            named("parse_more_full_pinyins", parse as *const () as usize),
            named(
                "get_parsed_input_length",
                parsed_length as *const () as usize,
            ),
            named("guess_sentence", guess_sentence as *const () as usize),
            named("get_n_candidate", n_candidate as *const () as usize),
            named("get_candidate", candidate as *const () as usize),
            named("get_candidate_type", candidate_type as *const () as usize),
            named(
                "get_candidate_string",
                candidate_string as *const () as usize,
            ),
        ]
    }

    /// Leaks the symbol names so they can key the `&'static str` table.
    fn leak(rows: Vec<(String, usize)>) -> HashMap<&'static str, usize> {
        rows.into_iter()
            .map(|(name, address)| (&*Box::leak(name.into_boxed_str()), address))
            .collect()
    }

    fn pinyin_mock() -> Mock {
        let mut rows = common("pinyin_");
        rows.push((
            "pinyin_get_sentence".into(),
            pinyin_get_sentence as *const () as usize,
        ));
        rows.push((
            "pinyin_guess_candidates".into(),
            pinyin_guess_candidates as *const () as usize,
        ));
        rows.push((
            "pinyin_get_candidate_nbest_index".into(),
            nbest_index as *const () as usize,
        ));
        rows.push(("g_free".into(), g_free as *const () as usize));
        Mock(leak(rows))
    }

    fn zhuyin_mock() -> Mock {
        let mut rows = common("zhuyin_");
        rows.push((
            "zhuyin_parse_more_chewings".into(),
            parse as *const () as usize,
        ));
        rows.push((
            "zhuyin_get_sentence".into(),
            zhuyin_get_sentence as *const () as usize,
        ));
        rows.push((
            "zhuyin_guess_candidates_after_cursor".into(),
            zhuyin_guess_candidates as *const () as usize,
        ));
        rows.push(("g_free".into(), g_free as *const () as usize));
        Mock(leak(rows))
    }

    fn without(mut mock: Mock, name: &str) -> Mock {
        assert!(mock.0.remove(name).is_some(), "{name} was not in the mock");
        mock
    }

    fn subject(facade: Facade, mock: &Mock) -> Subject {
        // SAFETY: the mock's functions have exactly the signatures the tables
        // declare.
        let api = unsafe {
            match facade {
                Facade::Pinyin => Api::Pinyin(PinyinFns::resolve(mock)),
                Facade::Zhuyin => Api::Zhuyin(ZhuyinFns::resolve(mock)),
            }
        };
        let missing = match &api {
            Api::Pinyin(fns) => fns.missing(),
            Api::Zhuyin(fns) => fns.missing(),
        };
        Subject {
            api,
            path: PathBuf::from("<mock>"),
            // SAFETY: `g_free` is `void g_free(gpointer)`.
            g_free: unsafe { mock.lookup::<GFreeFn>("g_free") },
            missing,
            _libraries: Vec::new(),
            _slot: None,
            _single_threaded: PhantomData,
        }
    }

    fn dirs() -> (PathBuf, PathBuf) {
        (PathBuf::from("system"), PathBuf::from("user"))
    }

    #[test]
    fn pinyin_observation_reads_everything_and_releases_everything() {
        let subject = subject(Facade::Pinyin, &pinyin_mock());
        assert_eq!(subject.facade(), Facade::Pinyin);
        assert!(subject.missing_symbols().is_empty());
        let (system, user) = dirs();
        {
            let context = subject.open_context(&system, &user).expect("context");
            context.set_options(0x18a).expect("options");
            assert!(matches!(
                context.set_options(0xdead),
                Err(AbiError::Call {
                    function: "pinyin_set_options"
                })
            ));
            let mut session = context.pinyin_session().expect("session");
            let seen = session.observe(b"nihao", SORT, 2).expect("observation");

            assert_eq!(seen.input, b"nihao");
            assert_eq!(seen.parse_return, 5);
            assert_eq!(seen.parsed_input_length, 5);
            assert_eq!(seen.sentence.as_deref(), Some("你好"));
            assert_eq!(seen.candidate_total, 3);
            // The cap applies to what is returned, not to the reported total.
            assert_eq!(seen.candidates.len(), 2);
            assert_eq!(seen.candidates[0].text, "你好");
            assert_eq!(
                seen.candidates[0].candidate_type(),
                Some(CandidateType::NbestMatch)
            );
            assert_eq!(seen.candidates[0].nbest_index, Some(0));
            assert_eq!(
                seen.candidates[1].candidate_type(),
                Some(CandidateType::Normal)
            );
            assert_eq!(seen.candidates[1].nbest_index, None);

            // A value the enumeration does not declare is kept, not rejected.
            let odd = session.candidate(2).expect("third candidate");
            assert_eq!(odd.raw_type, 99);
            assert_eq!(odd.candidate_type(), None);
            assert_eq!(odd.info(), None);
            assert_eq!(
                seen.candidates[0].info().map(|i| i.text),
                Some("你好".to_owned())
            );
            assert!(matches!(session.candidate(3), Err(AbiError::Call { .. })));

            // The caller-owned sentence string was released, once.
            assert_eq!(read(&STRINGS_HANDED_OVER), 1);
            assert_eq!(read(&STRINGS_FREED), 1);
            // The n-best accessor, which asserts on any other type, was only
            // asked of the n-best candidate.
            assert_eq!(read(&NBEST_ASSERTS), 0);
            assert_eq!(read(&INSTANCES_FREED), 0);
        }
        assert_eq!(read(&INSTANCES_FREED), 1);
        assert_eq!(read(&CONTEXTS_FINISHED), 1);
    }

    #[test]
    fn nothing_parsed_queries_nothing_further() {
        let subject = subject(Facade::Pinyin, &pinyin_mock());
        let (system, user) = dirs();
        let context = subject.open_context(&system, &user).expect("context");
        let mut session = context.pinyin_session().expect("session");
        let seen = session.observe(b"!!", SORT, 10).expect("observation");
        assert_eq!(seen.parsed_input_length, 0);
        assert_eq!(seen.sentence, None);
        assert_eq!(seen.candidate_total, 0);
        assert!(seen.candidates.is_empty());
        assert_eq!(read(&SENTENCE_GUESSES), 0);
        assert_eq!(read(&STRINGS_HANDED_OVER), 0);
    }

    #[test]
    fn zhuyin_has_its_own_candidate_types_and_an_unindexed_sentence() {
        let subject = subject(Facade::Zhuyin, &zhuyin_mock());
        let (system, user) = dirs();
        let context = subject.open_context(&system, &user).expect("context");
        let mut session = context.zhuyin_session().expect("session");
        for kind in [ZhuyinInput::FullPinyin, ZhuyinInput::Chewing] {
            let seen = session.observe(kind, b"nihao", 10).expect("observation");
            assert_eq!(seen.sentence.as_deref(), Some("你好"));
            assert_eq!(seen.candidate_total, 3);
            let types: Vec<_> = seen
                .candidates
                .iter()
                .map(ZhuyinCandidate::candidate_type)
                .collect();
            assert_eq!(
                types,
                [
                    Some(ZhuyinCandidateType::BestMatch),
                    Some(ZhuyinCandidateType::NormalAfterCursor),
                    Some(ZhuyinCandidateType::NormalBeforeCursor),
                ]
            );
        }
        assert_eq!(read(&STRINGS_HANDED_OVER), read(&STRINGS_FREED));
        assert_eq!(read(&STRINGS_FREED), 2);
    }

    #[test]
    fn a_session_of_the_other_facade_is_refused_not_miscalled() {
        let (system, user) = dirs();
        let subject = subject(Facade::Pinyin, &pinyin_mock());
        let context = subject.open_context(&system, &user).expect("context");
        assert!(matches!(
            context.zhuyin_session(),
            Err(AbiError::FacadeMismatch {
                context: Facade::Pinyin,
                requested: Facade::Zhuyin
            })
        ));
        let subject = self::subject(Facade::Zhuyin, &zhuyin_mock());
        let context = subject.open_context(&system, &user).expect("context");
        assert!(matches!(
            context.pinyin_session(),
            Err(AbiError::FacadeMismatch {
                context: Facade::Zhuyin,
                requested: Facade::Pinyin
            })
        ));
    }

    #[test]
    fn a_missing_symbol_fails_only_the_operation_that_needs_it() {
        let mock = without(pinyin_mock(), "pinyin_get_sentence");
        let subject = subject(Facade::Pinyin, &mock);
        assert_eq!(subject.missing_symbols(), ["pinyin_get_sentence"]);
        let (system, user) = dirs();
        let context = subject.open_context(&system, &user).expect("context");
        let mut session = context.pinyin_session().expect("session");
        // Everything that does not need it still works.
        assert_eq!(session.parse_full_pinyin(b"ni").expect("parse"), 2);
        assert!(session.guess_candidates(0, SORT).expect("guess"));
        assert_eq!(session.candidate_count().expect("count"), 3);
        // The operation that needs it says exactly what is missing.
        assert!(matches!(
            session.sentence(0),
            Err(AbiError::SymbolMissing {
                name: "pinyin_get_sentence"
            })
        ));
        assert!(matches!(
            session.observe(b"ni", SORT, 1),
            Err(AbiError::SymbolMissing {
                name: "pinyin_get_sentence"
            })
        ));
    }

    #[test]
    fn without_g_free_a_sentence_is_never_requested() {
        let mock = without(pinyin_mock(), "g_free");
        let subject = subject(Facade::Pinyin, &mock);
        let (system, user) = dirs();
        let context = subject.open_context(&system, &user).expect("context");
        let mut session = context.pinyin_session().expect("session");
        session.parse_full_pinyin(b"ni").expect("parse");
        session.guess_sentence().expect("guess");
        assert!(matches!(
            session.sentence(0),
            Err(AbiError::SymbolMissing { name: "g_free" })
        ));
        // The library was not asked, so nothing was handed over to leak.
        assert_eq!(read(&STRINGS_HANDED_OVER), 0);
    }

    #[test]
    fn a_revision_without_the_nbest_accessor_still_lists_candidates() {
        let mock = without(pinyin_mock(), "pinyin_get_candidate_nbest_index");
        let subject = subject(Facade::Pinyin, &mock);
        assert_eq!(
            subject.missing_symbols(),
            ["pinyin_get_candidate_nbest_index"]
        );
        let (system, user) = dirs();
        let context = subject.open_context(&system, &user).expect("context");
        let mut session = context.pinyin_session().expect("session");
        let seen = session.observe(b"nihao", SORT, 3).expect("observation");
        assert_eq!(seen.candidates.len(), 3);
        assert!(seen.candidates.iter().all(|c| c.nbest_index.is_none()));
    }

    #[test]
    fn a_library_without_init_cannot_open_a_context_but_still_opens() {
        let mock = without(pinyin_mock(), "pinyin_init");
        let subject = subject(Facade::Pinyin, &mock);
        let (system, user) = dirs();
        assert!(matches!(
            subject.open_context(&system, &user),
            Err(AbiError::SymbolMissing {
                name: "pinyin_init"
            })
        ));
    }

    #[test]
    fn refusals_are_reported_with_their_inputs() {
        let subject = subject(Facade::Pinyin, &pinyin_mock());
        let user = PathBuf::from("user");
        assert!(matches!(
            subject.open_context(Path::new("fail"), &user),
            Err(AbiError::ContextInitFailed { .. })
        ));
        assert!(matches!(
            subject.open_context(Path::new("a\0b"), &user),
            Err(AbiError::InteriorNul { what: "path" })
        ));
        let (system, user) = dirs();
        let context = subject.open_context(&system, &user).expect("context");
        let mut session = context.pinyin_session().expect("session");
        assert!(matches!(
            session.parse_full_pinyin(b"n\0i"),
            Err(AbiError::InteriorNul { what: "input" })
        ));
    }

    #[test]
    fn a_context_that_could_not_be_released_is_never_created() {
        let (system, user) = dirs();
        for (facade, mock, fini) in [
            (Facade::Pinyin, pinyin_mock(), "pinyin_fini"),
            (Facade::Zhuyin, zhuyin_mock(), "zhuyin_fini"),
        ] {
            let subject = subject(facade, &without(mock, fini));
            assert_eq!(subject.missing_symbols(), [fini]);
            assert!(matches!(
                subject.open_context(&system, &user),
                Err(AbiError::SymbolMissing { name }) if name == fini
            ));
            // `*_init` was never called, so no context exists to leak.
            assert_eq!(read(&INITS), 0, "{facade:?}");
            assert_eq!(read(&CONTEXTS_FINISHED), 0, "{facade:?}");
        }
    }

    #[test]
    fn an_instance_that_could_not_be_freed_is_never_allocated() {
        let (system, user) = dirs();
        for (facade, mock, free) in [
            (Facade::Pinyin, pinyin_mock(), "pinyin_free_instance"),
            (Facade::Zhuyin, zhuyin_mock(), "zhuyin_free_instance"),
        ] {
            let subject = subject(facade, &without(mock, free));
            let context = subject.open_context(&system, &user).expect("context");
            let refused = match facade {
                Facade::Pinyin => context.pinyin_session().err(),
                Facade::Zhuyin => context.zhuyin_session().err(),
            };
            assert!(matches!(
                refused,
                Some(AbiError::SymbolMissing { name }) if name == free
            ));
            // `*_alloc_instance` was never called.
            assert_eq!(read(&ALLOCS), 0, "{facade:?}");
        }
    }

    #[test]
    fn a_missing_allocator_creates_nothing_either() {
        let (system, user) = dirs();
        let subject = subject(
            Facade::Pinyin,
            &without(pinyin_mock(), "pinyin_alloc_instance"),
        );
        let context = subject.open_context(&system, &user).expect("context");
        assert!(matches!(
            context.pinyin_session(),
            Err(AbiError::SymbolMissing {
                name: "pinyin_alloc_instance"
            })
        ));
        assert_eq!(read(&ALLOCS), 0);
        drop(context);
        // The context that was created is still released.
        assert_eq!(read(&CONTEXTS_FINISHED), 1);
    }

    #[test]
    fn every_context_created_is_released_even_when_a_session_fails() {
        let (system, user) = dirs();
        let subject = subject(Facade::Pinyin, &pinyin_mock());
        {
            let context = subject.open_context(&system, &user).expect("context");
            let mut session = context.pinyin_session().expect("session");
            assert!(matches!(
                session.parse_full_pinyin(b"n\0i"),
                Err(AbiError::InteriorNul { .. })
            ));
        }
        assert_eq!(read(&INITS), 1);
        assert_eq!(read(&ALLOCS), 1);
        assert_eq!(read(&INSTANCES_FREED), 1);
        assert_eq!(read(&CONTEXTS_FINISHED), 1);
    }

    /// Compile-time check that a type is neither `Send` nor `Sync`: if it were,
    /// the call below would be ambiguous and the test would not compile.
    trait AmbiguousIfSend<A> {
        fn probe() {}
    }
    impl<T: ?Sized> AmbiguousIfSend<()> for T {}
    impl<T: ?Sized + Send> AmbiguousIfSend<u8> for T {}
    trait AmbiguousIfSync<A> {
        fn probe() {}
    }
    impl<T: ?Sized> AmbiguousIfSync<()> for T {}
    impl<T: ?Sized + Sync> AmbiguousIfSync<u8> for T {}

    #[test]
    fn nothing_derived_from_a_subject_can_cross_threads() {
        // libpinyin is not thread-safe (function-static slots, and the pin
        // aborts when several threads each open a context), so the whole chain
        // is `!Send` and `!Sync`.
        <Subject as AmbiguousIfSend<_>>::probe();
        <Subject as AmbiguousIfSync<_>>::probe();
        <Context<'static> as AmbiguousIfSend<_>>::probe();
        <Context<'static> as AmbiguousIfSync<_>>::probe();
        <PinyinSession<'static> as AmbiguousIfSend<_>>::probe();
        <PinyinSession<'static> as AmbiguousIfSync<_>>::probe();
        <ZhuyinSession<'static> as AmbiguousIfSend<_>>::probe();
        <ZhuyinSession<'static> as AmbiguousIfSync<_>>::probe();
    }

    #[test]
    fn a_failed_open_gives_its_slot_back() {
        static SLOTS: Slots = Slots::new();
        let missing = Path::new("definitely-not-a-libpinyin.so");
        // The load fails; the default open must not be left blocked by it.
        assert!(matches!(
            Subject::open_in(&SLOTS, missing, Facade::Pinyin, true),
            Err(AbiError::LibraryLoad { .. })
        ));
        drop(SLOTS.exclusive().expect("slot released after the failure"));
    }

    #[test]
    fn the_default_open_refuses_a_second_subject_before_touching_the_file() {
        static SLOTS: Slots = Slots::new();
        let Some(name) = system_library() else {
            return;
        };
        let first =
            Subject::open_in(&SLOTS, Path::new(name), Facade::Pinyin, true).expect("first subject");
        // The path is irrelevant: the slot is checked first.
        assert!(matches!(
            Subject::open_in(&SLOTS, Path::new("not-even-a-file"), Facade::Pinyin, true),
            Err(AbiError::SubjectAlreadyOpen)
        ));
        // The explicit opt-out still works alongside it.
        let second = Subject::open_in(&SLOTS, Path::new(name), Facade::Zhuyin, false)
            .expect("additional subject");
        drop(first);
        assert!(matches!(
            Subject::open_in(&SLOTS, Path::new(name), Facade::Pinyin, true),
            Err(AbiError::SubjectAlreadyOpen)
        ));
        drop(second);
        drop(Subject::open_in(&SLOTS, Path::new(name), Facade::Pinyin, true).expect("free again"));
    }

    #[test]
    fn the_first_subject_takes_the_only_slot_and_additional_ones_are_explicit() {
        static SLOTS: Slots = Slots::new();
        let first = SLOTS.exclusive().expect("first subject");
        assert!(matches!(
            SLOTS.exclusive(),
            Err(AbiError::SubjectAlreadyOpen)
        ));
        let second = SLOTS.additional();
        drop(first);
        // One is still open, so the default open is still refused.
        assert!(matches!(
            SLOTS.exclusive(),
            Err(AbiError::SubjectAlreadyOpen)
        ));
        drop(second);
        drop(SLOTS.exclusive().expect("free again"));
    }

    /// A library that is certainly present, to exercise the real loader on every
    /// platform: it has none of the subset's symbols.
    fn system_library() -> Option<&'static str> {
        if cfg!(target_os = "linux") {
            Some("libm.so.6")
        } else if cfg!(target_os = "macos") {
            Some("/usr/lib/libSystem.B.dylib")
        } else if cfg!(target_os = "windows") {
            Some("kernel32.dll")
        } else {
            None
        }
    }

    #[test]
    fn the_real_loader_reports_a_library_with_none_of_the_symbols() {
        let Some(name) = system_library() else {
            return;
        };
        for facade in [Facade::Pinyin, Facade::Zhuyin] {
            let subject = Subject::open_additional(name, facade).expect("system library loads");
            assert_eq!(subject.facade(), facade);
            assert_eq!(subject.path(), Path::new(name));
            assert_eq!(subject.missing_symbols().len(), 16, "{facade:?}");
            let prefix = match facade {
                Facade::Pinyin => "pinyin_",
                Facade::Zhuyin => "zhuyin_",
            };
            assert!(
                subject
                    .missing_symbols()
                    .iter()
                    .all(|n| n.starts_with(prefix))
            );
            let (system, user) = dirs();
            assert!(matches!(
                subject.open_context(&system, &user),
                Err(AbiError::SymbolMissing { .. })
            ));
        }
    }

    #[test]
    fn a_file_that_cannot_be_loaded_is_a_reported_failure() {
        let missing = Path::new("definitely-not-a-libpinyin.so");
        assert!(matches!(
            Subject::open_additional(missing, Facade::Pinyin),
            Err(AbiError::LibraryLoad { .. })
        ));
    }
}
