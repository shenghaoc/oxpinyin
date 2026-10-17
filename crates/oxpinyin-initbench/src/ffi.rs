//! The only module that loads a shared object and calls through it.
//!
//! A [`Native`] is one libpinyin or libzhuyin opened by path with `libloading`
//! and driven through the [`Driver`](crate::workload::Driver) trait. Everything
//! else in the crate is safe code over that trait.
//!
//! # Safety model
//!
//! * Every symbol of the chosen facade is resolved once, at open, into a typed
//!   function pointer. A library missing any of them is refused with all the
//!   missing names listed ([`BenchError::SymbolsMissing`]); no operation runs on
//!   a partial table.
//! * The types of the pointers are the pinned headers' (`src/pinyin.h`,
//!   `src/zhuyin.h` at 074a2219): the caller's statement that the file is a
//!   libpinyin of the requested facade is the one thing this module cannot
//!   check, as with any `dlopen`.
//! * The library handle is **never unloaded**: libpinyin and GLib keep
//!   process-global state and unloading code whose pointers may still be
//!   reachable buys nothing in a short-lived benchmark process. The cost is one
//!   leaked loader reference per [`Native`].
//! * Raw context and instance handles live only inside [`Native`], are created
//!   only by `init` and `alloc_instance` (NULL is reported, never stored), and
//!   are released by `free_instance` and `fini` in that order, exactly once;
//!   `Drop` releases whatever is still live in the same order. Methods that need
//!   a handle that does not exist return an error instead of calling.
//! * Strings the library lends (`*_get_candidate_string`) point into the
//!   instance and are copied before the next call. The sentence string is
//!   caller-owned (the headers: "should be freed by g_free()"); it is copied and
//!   released with the `g_free` the library's own dependency chain resolves
//!   (a symbol lookup on a library handle searches its dependencies on ELF and
//!   Mach-O). With no `g_free` a sentence read fails *before* the library is
//!   asked, so nothing is leaked.
//! * libpinyin is not thread-safe; [`Native`] is neither `Send` nor `Sync`.
//! * Nothing here catches an abort the library raises (some inputs assert in the
//!   pin). Such inputs are run in a child process by the callers.

use core::ffi::{CStr, c_char, c_int, c_uint, c_void};
use core::marker::PhantomData;
use core::ptr;
use std::ffi::CString;
use std::mem::ManuallyDrop;
use std::path::Path;

use crate::error::BenchError;
use crate::workload::{Bind, Candidate, Driver, Facade, ParseKind, PhraseRow};

type InitFn = unsafe extern "C" fn(*const c_char, *const c_char) -> *mut c_void;
type VoidHandleFn = unsafe extern "C" fn(*mut c_void);
type SetOptionsFn = unsafe extern "C" fn(*mut c_void, u32) -> bool;
type HandleFn = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type HandleBoolFn = unsafe extern "C" fn(*mut c_void) -> bool;
type ParseFn = unsafe extern "C" fn(*mut c_void, *const c_char) -> usize;
type LengthFn = unsafe extern "C" fn(*mut c_void) -> usize;
type PinyinSentenceFn = unsafe extern "C" fn(*mut c_void, u8, *mut *mut c_char) -> bool;
type ZhuyinSentenceFn = unsafe extern "C" fn(*mut c_void, *mut *mut c_char) -> bool;
type PinyinGuessFn = unsafe extern "C" fn(*mut c_void, usize, c_uint) -> bool;
type ZhuyinGuessFn = unsafe extern "C" fn(*mut c_void, usize) -> bool;
type CountFn = unsafe extern "C" fn(*mut c_void, *mut c_uint) -> bool;
type CandidateFn = unsafe extern "C" fn(*mut c_void, c_uint, *mut *mut c_void) -> bool;
type CandidateTypeFn = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_int) -> bool;
type CandidateStringFn = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut *const c_char) -> bool;
type NbestIndexFn = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut u8) -> bool;
type SaveFn = unsafe extern "C" fn(*mut c_void) -> bool;
type LoadLibraryFn = unsafe extern "C" fn(*mut c_void, u8) -> bool;
type BeginAddFn = unsafe extern "C" fn(*mut c_void, u8) -> *mut c_void;
type AddPhraseFn = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, i32) -> bool;
type BeginGetFn = unsafe extern "C" fn(*mut c_void, c_uint) -> *mut c_void;
type HasNextFn = unsafe extern "C" fn(*mut c_void) -> bool;
type GetNextFn =
    unsafe extern "C" fn(*mut c_void, *mut *mut c_char, *mut *mut c_char, *mut i32) -> bool;
type GFreeFn = unsafe extern "C" fn(*mut c_void);

/// Resolves a symbol by name to a raw address: the seam between the real
/// loader and the mock used in tests.
pub trait SymbolSource {
    /// The address of `name` (no trailing NUL), if the object exports it.
    fn lookup(&self, name: &str) -> Option<*const c_void>;
}

#[cfg(unix)]
type RawLibrary = libloading::os::unix::Library;
#[cfg(not(unix))]
type RawLibrary = libloading::Library;

struct Loader(ManuallyDrop<RawLibrary>);

impl Loader {
    fn open(path: &Path, bind: Bind) -> Result<Self, BenchError> {
        #[cfg(unix)]
        // SAFETY: loading a shared object runs its initialisers. The caller
        // named this file as the subject on purpose; this is the dlopen
        // contract, restated in the module documentation.
        let result = unsafe {
            let mode = match bind {
                Bind::Now => libloading::os::unix::RTLD_NOW,
                Bind::Lazy => libloading::os::unix::RTLD_LAZY,
            } | libloading::os::unix::RTLD_LOCAL;
            RawLibrary::open(Some(path), mode)
        };
        #[cfg(not(unix))]
        // SAFETY: as above. Binding mode is not selectable through the
        // portable `libloading` API; it is recorded as requested, not applied.
        let result = {
            let _ = bind;
            unsafe { RawLibrary::new(path) }
        };
        result
            .map(|library| Self(ManuallyDrop::new(library)))
            .map_err(|error| BenchError::Library {
                path: path.to_path_buf(),
                reason: error.to_string(),
            })
    }
}

impl SymbolSource for Loader {
    fn lookup(&self, name: &str) -> Option<*const c_void> {
        let c_name = CString::new(name).ok()?;
        // SAFETY: the symbol is read as an opaque address and only turned into
        // a typed function pointer in `Api::resolve`, whose declarations are
        // the pinned headers'.
        let symbol = unsafe { self.0.get::<*const c_void>(c_name.as_bytes_with_nul()) }.ok()?;
        Some(*symbol)
    }
}

/// The resolved function table of one facade.
#[derive(Clone, Copy)]
struct Api {
    init: InitFn,
    fini: VoidHandleFn,
    set_options: SetOptionsFn,
    alloc_instance: HandleFn,
    free_instance: VoidHandleFn,
    reset: HandleBoolFn,
    parse_full: ParseFn,
    parse_chewing: Option<ParseFn>,
    parsed_length: LengthFn,
    guess_sentence: HandleBoolFn,
    get_sentence_pinyin: Option<PinyinSentenceFn>,
    get_sentence_zhuyin: Option<ZhuyinSentenceFn>,
    guess_pinyin: Option<PinyinGuessFn>,
    guess_zhuyin: Option<ZhuyinGuessFn>,
    get_n_candidate: CountFn,
    get_candidate: CandidateFn,
    get_candidate_type: CandidateTypeFn,
    get_candidate_string: CandidateStringFn,
    get_nbest_index: Option<NbestIndexFn>,
    save: SaveFn,
    load_library: LoadLibraryFn,
    begin_add: BeginAddFn,
    add_phrase: AddPhraseFn,
    end_add: VoidHandleFn,
    begin_get: Option<BeginGetFn>,
    has_next: Option<HasNextFn>,
    get_next: Option<GetNextFn>,
    end_get: Option<VoidHandleFn>,
    g_free: Option<GFreeFn>,
}

impl Api {
    /// Resolves the table, listing **every** missing required symbol.
    fn resolve(source: &dyn SymbolSource, facade: Facade) -> Result<Self, BenchError> {
        let prefix = facade.prefix();
        let mut missing: Vec<String> = Vec::new();
        let mut get = |suffix: &str, required: bool| -> Option<*const c_void> {
            let name = format!("{prefix}_{suffix}");
            let found = source.lookup(&name);
            if found.is_none() && required {
                missing.push(name);
            }
            found
        };
        let pinyin = facade == Facade::Pinyin;
        let init = get("init", true);
        let fini = get("fini", true);
        let set_options = get("set_options", true);
        let alloc_instance = get("alloc_instance", true);
        let free_instance = get("free_instance", true);
        let reset = get("reset", true);
        let parse_full = get("parse_more_full_pinyins", true);
        let parse_chewing = get("parse_more_chewings", !pinyin);
        let parsed_length = get("get_parsed_input_length", true);
        let guess_sentence = get("guess_sentence", true);
        let get_sentence = get("get_sentence", true);
        let guess = get(
            if pinyin {
                "guess_candidates"
            } else {
                "guess_candidates_after_cursor"
            },
            true,
        );
        let get_n_candidate = get("get_n_candidate", true);
        let get_candidate = get("get_candidate", true);
        let get_candidate_type = get("get_candidate_type", true);
        let get_candidate_string = get("get_candidate_string", true);
        let get_nbest_index = get("get_candidate_nbest_index", pinyin);
        let save = get("save", true);
        let load_library = get("load_phrase_library", true);
        let begin_add = get("begin_add_phrases", true);
        let add_phrase = get("iterator_add_phrase", true);
        let end_add = get("end_add_phrases", true);
        let begin_get = get("begin_get_phrases", pinyin);
        let has_next = get("iterator_has_next_phrase", pinyin);
        let get_next = get("iterator_get_next_phrase", pinyin);
        let end_get = get("end_get_phrases", pinyin);
        // `g_free` is resolved by name without a prefix and is optional here:
        // its absence only makes sentence reads fail before the library is
        // asked (see the module documentation).
        let g_free = source.lookup("g_free");
        if !missing.is_empty() {
            return Err(BenchError::SymbolsMissing { names: missing });
        }
        // Every `Option` below that is required was just checked non-missing.
        // SAFETY (for every `transmute` in this function): each address was
        // returned by the loader for the exact symbol named, and the target
        // type is that function's declaration in the pinned public header for
        // the facade; a pointer and a function pointer have the same size.
        macro_rules! f {
            ($opt:expr, $ty:ty) => {
                match $opt {
                    // SAFETY: see above.
                    Some(address) => unsafe { core::mem::transmute::<*const c_void, $ty>(address) },
                    None => return Err(BenchError::invalid("a required symbol vanished")),
                }
            };
        }
        macro_rules! maybe {
            ($opt:expr, $ty:ty) => {
                // SAFETY: see above.
                $opt.map(|address| unsafe { core::mem::transmute::<*const c_void, $ty>(address) })
            };
        }
        Ok(Self {
            init: f!(init, InitFn),
            fini: f!(fini, VoidHandleFn),
            set_options: f!(set_options, SetOptionsFn),
            alloc_instance: f!(alloc_instance, HandleFn),
            free_instance: f!(free_instance, VoidHandleFn),
            reset: f!(reset, HandleBoolFn),
            parse_full: f!(parse_full, ParseFn),
            parse_chewing: maybe!(parse_chewing, ParseFn),
            parsed_length: f!(parsed_length, LengthFn),
            guess_sentence: f!(guess_sentence, HandleBoolFn),
            get_sentence_pinyin: if pinyin {
                maybe!(get_sentence, PinyinSentenceFn)
            } else {
                None
            },
            get_sentence_zhuyin: if pinyin {
                None
            } else {
                maybe!(get_sentence, ZhuyinSentenceFn)
            },
            guess_pinyin: if pinyin {
                maybe!(guess, PinyinGuessFn)
            } else {
                None
            },
            guess_zhuyin: if pinyin {
                None
            } else {
                maybe!(guess, ZhuyinGuessFn)
            },
            get_n_candidate: f!(get_n_candidate, CountFn),
            get_candidate: f!(get_candidate, CandidateFn),
            get_candidate_type: f!(get_candidate_type, CandidateTypeFn),
            get_candidate_string: f!(get_candidate_string, CandidateStringFn),
            get_nbest_index: maybe!(get_nbest_index, NbestIndexFn),
            save: f!(save, SaveFn),
            load_library: f!(load_library, LoadLibraryFn),
            begin_add: f!(begin_add, BeginAddFn),
            add_phrase: f!(add_phrase, AddPhraseFn),
            end_add: f!(end_add, VoidHandleFn),
            begin_get: maybe!(begin_get, BeginGetFn),
            has_next: maybe!(has_next, HasNextFn),
            get_next: maybe!(get_next, GetNextFn),
            end_get: maybe!(end_get, VoidHandleFn),
            g_free: maybe!(g_free, GFreeFn),
        })
    }
}

/// One loaded library and the handles created through it.
///
/// Neither `Send` nor `Sync` (see the module documentation).
pub struct Native {
    facade: Facade,
    api: Api,
    context: *mut c_void,
    instance: *mut c_void,
    // Kept alive for the life of the process; never unloaded.
    _loader: Option<Loader>,
    _single_threaded: PhantomData<*mut ()>,
}

fn path_cstring(path: &Path) -> Result<CString, BenchError> {
    let text = path
        .to_str()
        .ok_or_else(|| BenchError::invalid(format!("{} is not UTF-8", path.display())))?;
    CString::new(text).map_err(|_| BenchError::invalid("a path contains a NUL byte"))
}

fn text_cstring(text: &str, what: &str) -> Result<CString, BenchError> {
    CString::new(text).map_err(|_| BenchError::invalid(format!("{what} contains a NUL byte")))
}

/// Copies a library-lent C string.
///
/// # Safety
///
/// `text` must be non-null and point to a NUL-terminated string that stays
/// valid for the duration of the call.
unsafe fn copy_lent(text: *const c_char) -> Result<String, BenchError> {
    // SAFETY: the caller guarantees a valid NUL-terminated string.
    let bytes = unsafe { CStr::from_ptr(text) };
    bytes
        .to_str()
        .map(str::to_owned)
        .map_err(|_| BenchError::invalid("the library returned a string that is not UTF-8"))
}

impl Native {
    /// Opens the shared object at `path` as `facade` and resolves every symbol.
    ///
    /// # Errors
    ///
    /// [`BenchError::Library`] when the loader refuses the file and
    /// [`BenchError::SymbolsMissing`] naming every needed symbol it lacks.
    pub fn open(path: &Path, facade: Facade, bind: Bind) -> Result<Self, BenchError> {
        let loader = Loader::open(path, bind)?;
        let api = Api::resolve(&loader, facade)?;
        Ok(Self::with_api(facade, api, Some(loader)))
    }

    /// Builds over an arbitrary symbol source (the test seam).
    ///
    /// # Errors
    ///
    /// [`BenchError::SymbolsMissing`].
    pub fn from_source(source: &dyn SymbolSource, facade: Facade) -> Result<Self, BenchError> {
        Ok(Self::with_api(facade, Api::resolve(source, facade)?, None))
    }

    fn with_api(facade: Facade, api: Api, loader: Option<Loader>) -> Self {
        Self {
            facade,
            api,
            context: ptr::null_mut(),
            instance: ptr::null_mut(),
            _loader: loader,
            _single_threaded: PhantomData,
        }
    }

    fn need_context(&self, op: &'static str) -> Result<*mut c_void, BenchError> {
        if self.context.is_null() {
            Err(BenchError::invalid(format!("{op} needs a live context")))
        } else {
            Ok(self.context)
        }
    }

    fn need_instance(&self, op: &'static str) -> Result<*mut c_void, BenchError> {
        if self.instance.is_null() {
            Err(BenchError::invalid(format!("{op} needs a live instance")))
        } else {
            Ok(self.instance)
        }
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        // Instance before context, each once, whatever is still live.
        if !self.instance.is_null() {
            // SAFETY: a live instance this value owns, released exactly once.
            unsafe { (self.api.free_instance)(self.instance) };
            self.instance = ptr::null_mut();
        }
        if !self.context.is_null() {
            // SAFETY: a live context this value owns, released exactly once,
            // after the instance that borrowed it.
            unsafe { (self.api.fini)(self.context) };
            self.context = ptr::null_mut();
        }
    }
}

impl Driver for Native {
    fn facade(&self) -> Facade {
        self.facade
    }

    fn init(&mut self, system: &Path, user: &Path) -> Result<bool, BenchError> {
        if !self.context.is_null() {
            return Err(BenchError::invalid(
                "init called with a context already live",
            ));
        }
        let system = path_cstring(system)?;
        let user = path_cstring(user)?;
        // SAFETY: both arguments are valid NUL-terminated strings that outlive
        // the call; the result is NULL or an owned context.
        self.context = unsafe { (self.api.init)(system.as_ptr(), user.as_ptr()) };
        Ok(!self.context.is_null())
    }

    fn set_options(&mut self, bits: u32) -> Result<bool, BenchError> {
        let context = self.need_context("set_options")?;
        // SAFETY: a live context this value owns.
        Ok(unsafe { (self.api.set_options)(context, bits) })
    }

    fn alloc_instance(&mut self) -> Result<bool, BenchError> {
        let context = self.need_context("alloc_instance")?;
        if !self.instance.is_null() {
            return Err(BenchError::invalid(
                "alloc_instance with an instance already live",
            ));
        }
        // SAFETY: a live context this value owns; NULL is stored as "none".
        self.instance = unsafe { (self.api.alloc_instance)(context) };
        Ok(!self.instance.is_null())
    }

    fn reset(&mut self) -> Result<bool, BenchError> {
        let instance = self.need_instance("reset")?;
        // SAFETY: a live instance this value owns.
        Ok(unsafe { (self.api.reset)(instance) })
    }

    fn parse(&mut self, kind: ParseKind, input: &str) -> Result<usize, BenchError> {
        let instance = self.need_instance("parse")?;
        let text = text_cstring(input, "the input")?;
        let function = match kind {
            ParseKind::FullPinyin => self.api.parse_full,
            ParseKind::Chewing => self.api.parse_chewing.ok_or(BenchError::SymbolsMissing {
                names: vec![format!("{}_parse_more_chewings", self.facade.prefix())],
            })?,
        };
        // SAFETY: a live instance; `text` is NUL-terminated and outlives the call.
        Ok(unsafe { function(instance, text.as_ptr()) })
    }

    fn parsed_length(&mut self) -> Result<usize, BenchError> {
        let instance = self.need_instance("get_parsed_input_length")?;
        // SAFETY: a live instance this value owns.
        Ok(unsafe { (self.api.parsed_length)(instance) })
    }

    fn guess_sentence(&mut self) -> Result<bool, BenchError> {
        let instance = self.need_instance("guess_sentence")?;
        // SAFETY: a live instance this value owns.
        Ok(unsafe { (self.api.guess_sentence)(instance) })
    }

    fn sentence(&mut self, index: u8) -> Result<Option<String>, BenchError> {
        let instance = self.need_instance("get_sentence")?;
        let g_free = self.api.g_free.ok_or(BenchError::SymbolsMissing {
            names: vec!["g_free".to_owned()],
        })?;
        let mut out: *mut c_char = ptr::null_mut();
        let ok = match (self.api.get_sentence_pinyin, self.api.get_sentence_zhuyin) {
            // SAFETY: a live instance and a valid out-pointer.
            (Some(get), _) => unsafe { get(instance, index, &raw mut out) },
            // SAFETY: as above (libzhuyin's `get_sentence` takes no index).
            (None, Some(get)) => unsafe { get(instance, &raw mut out) },
            (None, None) => return Err(BenchError::invalid("no get_sentence resolved")),
        };
        if !ok {
            return Ok(None);
        }
        if out.is_null() {
            return Err(BenchError::Call {
                function: "get_sentence (NULL string with success)",
            });
        }
        // SAFETY: the library reported success and a non-null caller-owned
        // NUL-terminated string.
        let copied = unsafe { copy_lent(out) };
        // SAFETY: `out` was allocated by this library's GLib and is released
        // exactly once, after the copy, with the `g_free` of the same GLib.
        unsafe { g_free(out.cast()) };
        copied.map(Some)
    }

    fn guess_candidates(&mut self, offset: usize, sort: u32) -> Result<bool, BenchError> {
        let instance = self.need_instance("guess_candidates")?;
        match (self.api.guess_pinyin, self.api.guess_zhuyin) {
            // SAFETY: a live instance this value owns.
            (Some(guess), _) => Ok(unsafe { guess(instance, offset, sort) }),
            // SAFETY: as above (libzhuyin takes no sort word).
            (None, Some(guess)) => Ok(unsafe { guess(instance, offset) }),
            (None, None) => Err(BenchError::invalid("no guess_candidates resolved")),
        }
    }

    fn candidate_count(&mut self) -> Result<Option<u32>, BenchError> {
        let instance = self.need_instance("get_n_candidate")?;
        let mut count: c_uint = 0;
        // SAFETY: a live instance and a valid out-pointer.
        let ok = unsafe { (self.api.get_n_candidate)(instance, &raw mut count) };
        Ok(ok.then_some(count))
    }

    fn candidate(&mut self, index: u32) -> Result<Option<Candidate>, BenchError> {
        let instance = self.need_instance("get_candidate")?;
        let mut handle: *mut c_void = ptr::null_mut();
        // SAFETY: a live instance and a valid out-pointer.
        if !unsafe { (self.api.get_candidate)(instance, index, &raw mut handle) } {
            return Ok(None);
        }
        if handle.is_null() {
            return Err(BenchError::Call {
                function: "get_candidate (NULL candidate with success)",
            });
        }
        let mut raw_type: c_int = 0;
        // SAFETY: `handle` was just lent by this instance and is not retained.
        if !unsafe { (self.api.get_candidate_type)(instance, handle, &raw mut raw_type) } {
            return Err(BenchError::Call {
                function: "get_candidate_type",
            });
        }
        let mut text: *const c_char = ptr::null();
        // SAFETY: as above; the string is lent and valid until the next call.
        if !unsafe { (self.api.get_candidate_string)(instance, handle, &raw mut text) }
            || text.is_null()
        {
            return Err(BenchError::Call {
                function: "get_candidate_string",
            });
        }
        // SAFETY: the library reported success and a non-null lent string.
        let text = unsafe { copy_lent(text) }?;
        // The accessor asserts the candidate is an n-best one (type 1), so it is
        // asked only of those.
        let nbest = match (self.facade, self.api.get_nbest_index) {
            (Facade::Pinyin, Some(get)) if raw_type == 1 => {
                let mut index: u8 = 0;
                // SAFETY: as above; the type check guards the pin's assertion.
                unsafe { get(instance, handle, &raw mut index) }.then_some(index)
            }
            _ => None,
        };
        Ok(Some(Candidate {
            raw_type,
            text,
            nbest,
        }))
    }

    fn save(&mut self) -> Result<bool, BenchError> {
        let context = self.need_context("save")?;
        // SAFETY: a live context this value owns.
        Ok(unsafe { (self.api.save)(context) })
    }

    fn load_library(&mut self, library: u8) -> Result<bool, BenchError> {
        let context = self.need_context("load_phrase_library")?;
        // SAFETY: a live context this value owns.
        Ok(unsafe { (self.api.load_library)(context, library) })
    }

    fn add_phrases(&mut self, library: u8, rows: &[PhraseRow]) -> Result<Vec<bool>, BenchError> {
        let context = self.need_context("begin_add_phrases")?;
        // SAFETY: a live context this value owns; NULL is checked below.
        let iterator = unsafe { (self.api.begin_add)(context, library) };
        if iterator.is_null() {
            return Err(BenchError::Call {
                function: "begin_add_phrases",
            });
        }
        let mut accepted = Vec::with_capacity(rows.len());
        let mut failure = None;
        for row in rows {
            match (
                text_cstring(&row.phrase, "a phrase"),
                text_cstring(&row.pinyin, "a pinyin"),
            ) {
                (Ok(phrase), Ok(pinyin)) => {
                    // SAFETY: a live iterator; both strings are NUL-terminated
                    // and outlive the call.
                    accepted.push(unsafe {
                        (self.api.add_phrase)(iterator, phrase.as_ptr(), pinyin.as_ptr(), row.count)
                    });
                }
                (Err(error), _) | (_, Err(error)) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        // SAFETY: the iterator `begin_add` returned, ended exactly once, also on
        // the error path.
        unsafe { (self.api.end_add)(iterator) };
        failure.map_or(Ok(accepted), Err)
    }

    fn export_phrases(&mut self, library: u32) -> Result<Vec<PhraseRow>, BenchError> {
        let context = self.need_context("begin_get_phrases")?;
        let (Some(begin), Some(has_next), Some(get_next), Some(end), Some(g_free)) = (
            self.api.begin_get,
            self.api.has_next,
            self.api.get_next,
            self.api.end_get,
            self.api.g_free,
        ) else {
            return Err(BenchError::Unsupported(
                "phrase export is only wired for the pinyin facade with g_free".to_owned(),
            ));
        };
        // SAFETY: a live context this value owns; NULL is checked below.
        let iterator = unsafe { begin(context, library) };
        if iterator.is_null() {
            return Err(BenchError::Call {
                function: "begin_get_phrases",
            });
        }
        let mut rows = Vec::new();
        let mut failure = None;
        // SAFETY: a live iterator for every call in the loop.
        while unsafe { has_next(iterator) } {
            let mut phrase: *mut c_char = ptr::null_mut();
            let mut pinyin: *mut c_char = ptr::null_mut();
            let mut count: i32 = -1;
            // SAFETY: a live iterator and valid out-pointers.
            let ok =
                unsafe { get_next(iterator, &raw mut phrase, &raw mut pinyin, &raw mut count) };
            if !ok || phrase.is_null() || pinyin.is_null() {
                failure = Some(BenchError::Call {
                    function: "iterator_get_next_phrase",
                });
                break;
            }
            // SAFETY: both are non-null caller-owned NUL-terminated strings.
            let copied =
                unsafe { copy_lent(phrase).and_then(|p| copy_lent(pinyin).map(|y| (p, y))) };
            // SAFETY: both were allocated by this library's GLib and are
            // released exactly once, after the copy.
            unsafe {
                g_free(phrase.cast());
                g_free(pinyin.cast());
            }
            match copied {
                Ok((phrase, pinyin)) => rows.push(PhraseRow {
                    phrase,
                    pinyin,
                    count,
                }),
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        // SAFETY: the iterator `begin` returned, ended exactly once.
        unsafe { end(iterator) };
        failure.map_or(Ok(rows), Err)
    }

    fn free_instance(&mut self) -> Result<(), BenchError> {
        let instance = self.need_instance("free_instance")?;
        // SAFETY: a live instance this value owns, released exactly once.
        unsafe { (self.api.free_instance)(instance) };
        self.instance = ptr::null_mut();
        Ok(())
    }

    fn fini(&mut self) -> Result<(), BenchError> {
        let context = self.need_context("fini")?;
        if !self.instance.is_null() {
            return Err(BenchError::invalid("fini with an instance still live"));
        }
        // SAFETY: a live context this value owns, released exactly once.
        unsafe { (self.api.fini)(context) };
        self.context = ptr::null_mut();
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, unsafe_code)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    thread_local! {
        static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        static ALLOC_NULL: RefCell<bool> = const { RefCell::new(false) };
    }

    fn log(entry: &str) {
        LOG.with(|l| l.borrow_mut().push(entry.to_owned()));
    }

    fn take_log() -> Vec<String> {
        LOG.with(|l| std::mem::take(&mut *l.borrow_mut()))
    }

    unsafe extern "C" fn m_init(_: *const c_char, _: *const c_char) -> *mut c_void {
        log("init");
        0x10 as *mut c_void
    }
    unsafe extern "C" fn m_fini(_: *mut c_void) {
        log("fini");
    }
    unsafe extern "C" fn m_set_options(_: *mut c_void, bits: u32) -> bool {
        log(&format!("options={bits:#x}"));
        true
    }
    unsafe extern "C" fn m_alloc(_: *mut c_void) -> *mut c_void {
        log("alloc");
        if ALLOC_NULL.with(|n| *n.borrow()) {
            ptr::null_mut()
        } else {
            0x20 as *mut c_void
        }
    }
    unsafe extern "C" fn m_free(_: *mut c_void) {
        log("free");
    }
    unsafe extern "C" fn m_true(_: *mut c_void) -> bool {
        true
    }
    unsafe extern "C" fn m_parse(_: *mut c_void, s: *const c_char) -> usize {
        // SAFETY: the test passes a valid C string.
        unsafe { CStr::from_ptr(s) }.to_bytes().len()
    }
    unsafe extern "C" fn m_len(_: *mut c_void) -> usize {
        5
    }
    unsafe extern "C" fn m_get_sentence(_: *mut c_void, _: u8, out: *mut *mut c_char) -> bool {
        // The test GLib: a leaked C string released by m_g_free below.
        let owned = CString::new("你好").unwrap().into_raw();
        // SAFETY: `out` is a valid out-pointer from the caller.
        unsafe { *out = owned };
        true
    }
    unsafe extern "C" fn m_g_free(p: *mut c_void) {
        log("g_free");
        // SAFETY: only ever called with the pointer `into_raw` produced above.
        drop(unsafe { CString::from_raw(p.cast()) });
    }
    unsafe extern "C" fn m_guess(_: *mut c_void, _: usize, _: c_uint) -> bool {
        true
    }
    unsafe extern "C" fn m_count(_: *mut c_void, n: *mut c_uint) -> bool {
        // SAFETY: valid out-pointer.
        unsafe { *n = 2 };
        true
    }
    unsafe extern "C" fn m_cand(_: *mut c_void, _: c_uint, out: *mut *mut c_void) -> bool {
        // SAFETY: valid out-pointer.
        unsafe { *out = 0x30 as *mut c_void };
        true
    }
    unsafe extern "C" fn m_type(_: *mut c_void, _: *mut c_void, t: *mut c_int) -> bool {
        // SAFETY: valid out-pointer.
        unsafe { *t = 1 };
        true
    }
    static TEXT: &CStr = c"你";
    unsafe extern "C" fn m_string(_: *mut c_void, _: *mut c_void, s: *mut *const c_char) -> bool {
        // SAFETY: valid out-pointer to a static string.
        unsafe { *s = TEXT.as_ptr() };
        true
    }
    unsafe extern "C" fn m_nbest(_: *mut c_void, _: *mut c_void, i: *mut u8) -> bool {
        log("nbest");
        // SAFETY: valid out-pointer.
        unsafe { *i = 2 };
        true
    }
    unsafe extern "C" fn m_load(_: *mut c_void, _: u8) -> bool {
        true
    }
    unsafe extern "C" fn m_begin_add(_: *mut c_void, _: u8) -> *mut c_void {
        0x40 as *mut c_void
    }
    unsafe extern "C" fn m_add(_: *mut c_void, _: *const c_char, _: *const c_char, c: i32) -> bool {
        c >= 0
    }
    unsafe extern "C" fn m_end_add(_: *mut c_void) {
        log("end_add");
    }

    struct Mock(HashMap<&'static str, *const c_void>);

    impl SymbolSource for Mock {
        fn lookup(&self, name: &str) -> Option<*const c_void> {
            self.0.get(name).copied()
        }
    }

    fn mock(facade: Facade) -> Mock {
        let p = facade.prefix();
        let mut m: HashMap<&'static str, *const c_void> = HashMap::new();
        let mut put = |suffix: &str, address: *const c_void| {
            m.insert(Box::leak(format!("{p}_{suffix}").into_boxed_str()), address);
        };
        put("init", m_init as *const c_void);
        put("fini", m_fini as *const c_void);
        put("set_options", m_set_options as *const c_void);
        put("alloc_instance", m_alloc as *const c_void);
        put("free_instance", m_free as *const c_void);
        put("reset", m_true as *const c_void);
        put("parse_more_full_pinyins", m_parse as *const c_void);
        put("parse_more_chewings", m_parse as *const c_void);
        put("get_parsed_input_length", m_len as *const c_void);
        put("guess_sentence", m_true as *const c_void);
        put("get_sentence", m_get_sentence as *const c_void);
        put(
            if facade == Facade::Pinyin {
                "guess_candidates"
            } else {
                "guess_candidates_after_cursor"
            },
            m_guess as *const c_void,
        );
        put("get_n_candidate", m_count as *const c_void);
        put("get_candidate", m_cand as *const c_void);
        put("get_candidate_type", m_type as *const c_void);
        put("get_candidate_string", m_string as *const c_void);
        put("get_candidate_nbest_index", m_nbest as *const c_void);
        put("save", m_true as *const c_void);
        put("load_phrase_library", m_load as *const c_void);
        put("begin_add_phrases", m_begin_add as *const c_void);
        put("iterator_add_phrase", m_add as *const c_void);
        put("end_add_phrases", m_end_add as *const c_void);
        for name in [
            "begin_get_phrases",
            "iterator_has_next_phrase",
            "iterator_get_next_phrase",
            "end_get_phrases",
        ] {
            put(name, m_true as *const c_void);
        }
        let mut hm = Mock(m);
        hm.0.insert("g_free", m_g_free as *const c_void);
        hm
    }

    #[test]
    fn a_complete_table_resolves_for_both_facades() {
        Native::from_source(&mock(Facade::Pinyin), Facade::Pinyin).unwrap();
        Native::from_source(&mock(Facade::Zhuyin), Facade::Zhuyin).unwrap();
    }

    #[test]
    fn every_missing_symbol_is_named_and_nothing_runs() {
        let mut m = mock(Facade::Pinyin);
        m.0.remove("pinyin_init");
        m.0.remove("pinyin_save");
        take_log();
        let Err(BenchError::SymbolsMissing { names }) = Native::from_source(&m, Facade::Pinyin)
        else {
            panic!("a partial table was accepted");
        };
        assert_eq!(names, ["pinyin_init", "pinyin_save"]);
        assert!(take_log().is_empty());
    }

    #[test]
    fn a_pinyin_table_is_not_a_zhuyin_table() {
        // The same mock under the other prefix lacks every zhuyin name.
        assert!(matches!(
            Native::from_source(&mock(Facade::Pinyin), Facade::Zhuyin),
            Err(BenchError::SymbolsMissing { .. })
        ));
    }

    #[test]
    fn lifecycle_releases_instance_then_context_exactly_once() {
        let mut n = Native::from_source(&mock(Facade::Pinyin), Facade::Pinyin).unwrap();
        take_log();
        assert!(n.init(Path::new("/s"), Path::new("/u")).unwrap());
        assert!(n.set_options(0x18a).unwrap());
        assert!(n.alloc_instance().unwrap());
        n.free_instance().unwrap();
        n.fini().unwrap();
        drop(n);
        assert_eq!(
            take_log(),
            ["init", "options=0x18a", "alloc", "free", "fini"]
        );
    }

    #[test]
    fn drop_releases_what_is_still_live_in_order() {
        let mut n = Native::from_source(&mock(Facade::Pinyin), Facade::Pinyin).unwrap();
        take_log();
        n.init(Path::new("/s"), Path::new("/u")).unwrap();
        n.alloc_instance().unwrap();
        drop(n);
        assert_eq!(take_log(), ["init", "alloc", "free", "fini"]);
    }

    #[test]
    fn operations_without_their_handle_are_errors_not_calls() {
        let mut n = Native::from_source(&mock(Facade::Pinyin), Facade::Pinyin).unwrap();
        take_log();
        assert!(n.set_options(1).is_err());
        assert!(n.alloc_instance().is_err());
        assert!(n.parse(ParseKind::FullPinyin, "ni").is_err());
        assert!(n.fini().is_err());
        n.init(Path::new("/s"), Path::new("/u")).unwrap();
        assert!(
            n.init(Path::new("/s"), Path::new("/u")).is_err(),
            "double init"
        );
        n.alloc_instance().unwrap();
        assert!(n.fini().is_err(), "fini while an instance is live");
        assert_eq!(take_log(), ["init", "alloc"]);
    }

    #[test]
    fn a_null_instance_is_reported_not_stored() {
        let mut n = Native::from_source(&mock(Facade::Pinyin), Facade::Pinyin).unwrap();
        n.init(Path::new("/s"), Path::new("/u")).unwrap();
        ALLOC_NULL.with(|v| *v.borrow_mut() = true);
        assert!(!n.alloc_instance().unwrap());
        ALLOC_NULL.with(|v| *v.borrow_mut() = false);
        take_log();
        drop(n);
        assert_eq!(
            take_log(),
            ["fini"],
            "no instance was created, so none is freed"
        );
    }

    #[test]
    fn the_sentence_is_copied_then_released_with_the_librarys_g_free() {
        let mut n = Native::from_source(&mock(Facade::Pinyin), Facade::Pinyin).unwrap();
        n.init(Path::new("/s"), Path::new("/u")).unwrap();
        n.alloc_instance().unwrap();
        take_log();
        assert_eq!(n.sentence(0).unwrap().as_deref(), Some("你好"));
        assert_eq!(take_log(), ["g_free"]);
    }

    #[test]
    fn without_g_free_a_sentence_read_fails_before_the_library_is_asked() {
        let mut m = mock(Facade::Pinyin);
        m.0.remove("g_free");
        let mut n = Native::from_source(&m, Facade::Pinyin).unwrap();
        n.init(Path::new("/s"), Path::new("/u")).unwrap();
        n.alloc_instance().unwrap();
        assert!(matches!(
            n.sentence(0),
            Err(BenchError::SymbolsMissing { .. })
        ));
    }

    #[test]
    fn candidates_are_copied_and_nbest_is_asked_only_of_type_one() {
        let mut n = Native::from_source(&mock(Facade::Pinyin), Facade::Pinyin).unwrap();
        n.init(Path::new("/s"), Path::new("/u")).unwrap();
        n.alloc_instance().unwrap();
        take_log();
        let c = n.candidate(0).unwrap().unwrap();
        assert_eq!((c.raw_type, c.text.as_str(), c.nbest), (1, "你", Some(2)));
        assert_eq!(take_log(), ["nbest"]);
        // The zhuyin facade has no n-best accessor to call.
        let mut z = Native::from_source(&mock(Facade::Zhuyin), Facade::Zhuyin).unwrap();
        z.init(Path::new("/s"), Path::new("/u")).unwrap();
        z.alloc_instance().unwrap();
        take_log();
        assert_eq!(z.candidate(0).unwrap().unwrap().nbest, None);
        assert!(take_log().is_empty());
    }

    #[test]
    fn interior_nul_in_input_or_path_is_an_error() {
        let mut n = Native::from_source(&mock(Facade::Pinyin), Facade::Pinyin).unwrap();
        assert!(n.init(Path::new("/a\0b"), Path::new("/u")).is_err());
        n.init(Path::new("/s"), Path::new("/u")).unwrap();
        n.alloc_instance().unwrap();
        assert!(n.parse(ParseKind::FullPinyin, "n\0i").is_err());
    }

    #[test]
    fn add_phrases_always_ends_its_iterator() {
        let mut n = Native::from_source(&mock(Facade::Pinyin), Facade::Pinyin).unwrap();
        n.init(Path::new("/s"), Path::new("/u")).unwrap();
        take_log();
        let rows = vec![
            PhraseRow {
                phrase: "测试".into(),
                pinyin: "ce4'shi4".into(),
                count: 5,
            },
            PhraseRow {
                phrase: "坏".into(),
                pinyin: "huai4".into(),
                count: -1,
            },
        ];
        assert_eq!(n.add_phrases(1, &rows).unwrap(), [true, false]);
        assert_eq!(take_log(), ["end_add"]);
        let bad = vec![PhraseRow {
            phrase: "a\0".into(),
            pinyin: "a".into(),
            count: 1,
        }];
        assert!(n.add_phrases(1, &bad).is_err());
        assert_eq!(
            take_log(),
            ["end_add"],
            "the iterator is ended on the error path too"
        );
    }
}
