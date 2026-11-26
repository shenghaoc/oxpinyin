//! The loaded subject owns all handles and strings; no product crate is linked.
use crate::{Result, require};
use std::ffi::{CStr, CString, c_char, c_void};
use std::path::Path;

pub(crate) type Handle = *mut c_void;
type Release = extern "C" fn(Handle);

pub(crate) struct Library(libloading::Library);
impl Library {
    pub(crate) fn open(path: &str) -> Result<Self> {
        Ok(Self(crate::platform::load(path)?))
    }
    fn get<T: Copy>(&self, name: &str) -> Result<T> {
        // SAFETY: Only the typed ABI tables below call this private loader; signatures
        // match pinyin.h/zhuyin.h. The Library outlives every table and live handle.
        unsafe { Ok(*self.0.get::<T>(name.as_bytes())?) }
    }
    pub(crate) fn release(&self) -> Result<Release> {
        crate::platform::release(&self.0)
    }
}

macro_rules! table {
    ($name:ident { $($field:ident: ($($arg:ty),*) -> $ret:ty = $suffix:literal),* $(,)? }) => {
        pub(crate) struct $name { $(pub(crate) $field: extern "C" fn($($arg),*) -> $ret,)* }
        impl $name {
            pub(crate) fn load(library: &Library, facade: &str) -> Result<Self> {
                Ok(Self { $($field: library.get(&format!("{facade}_{}", $suffix))?,)* })
            }
        }
    };
}

table!(Core {
    init: (*const c_char, *const c_char) -> Handle = "init",
    fini: (Handle) -> () = "fini",
});
table!(Typing {
    alloc: (Handle) -> Handle = "alloc_instance",
    free: (Handle) -> () = "free_instance",
    options: (Handle, u32) -> bool = "set_options",
    reset: (Handle) -> bool = "reset",
    parse: (Handle, *const c_char) -> usize = "parse_more_full_pinyins",
    count: (Handle, *mut u32) -> bool = "get_n_candidate",
});

pub(crate) enum Guess {
    Pinyin(extern "C" fn(Handle, usize, u32) -> bool),
    Zhuyin(extern "C" fn(Handle, usize) -> bool),
}
impl Guess {
    pub(crate) fn load(library: &Library, facade: &str) -> Result<Self> {
        if facade == "zhuyin" {
            Ok(Self::Zhuyin(
                library.get("zhuyin_guess_candidates_after_cursor")?,
            ))
        } else {
            Ok(Self::Pinyin(library.get("pinyin_guess_candidates")?))
        }
    }
    pub(crate) fn call(&self, instance: Handle) -> bool {
        match self {
            Self::Pinyin(f) => f(instance, 0, 0x1e),
            Self::Zhuyin(f) => f(instance, 0),
        }
    }
}

table!(Sentence {
    guess: (Handle) -> bool = "guess_sentence",
    get: (Handle, u8, *mut *mut c_char) -> bool = "get_sentence",
    candidate: (Handle, u32, *mut Handle) -> bool = "get_candidate",
    kind: (Handle, Handle, *mut i32) -> bool = "get_candidate_type",
    rank: (Handle, Handle, *mut u8) -> bool = "get_candidate_nbest_index",
    text: (Handle, Handle, *mut *const c_char) -> bool = "get_candidate_string",
});
table!(Training {
    parsed: (Handle) -> usize = "get_parsed_input_length",
    train: (Handle, u8) -> bool = "train",
    choose: (Handle, usize, Handle) -> i32 = "choose_candidate",
});
table!(Import {
    begin: (Handle, u8) -> Handle = "begin_add_phrases",
    add: (Handle, *const c_char, *const c_char, i32) -> bool = "iterator_add_phrase",
    end: (Handle) -> () = "end_add_phrases",
    save: (Handle) -> bool = "save",
});
table!(Export {
    load: (Handle, u8) -> bool = "load_phrase_library",
    begin: (Handle, u32) -> Handle = "begin_get_phrases",
    next: (Handle) -> bool = "iterator_has_next_phrase",
    get: (Handle, *mut *mut c_char, *mut *mut c_char, *mut i32) -> bool = "iterator_get_next_phrase",
    end: (Handle) -> () = "end_get_phrases",
});
table!(Bigram {
    begin: (Handle) -> Handle = "begin_get_bigram_phrases",
    next: (Handle) -> bool = "bigram_iterator_has_next_phrase",
    get: (Handle, *mut *mut c_char, *mut *mut c_char, *mut i32) -> bool = "bigram_iterator_get_next_phrase",
    end: (Handle) -> () = "end_get_bigram_phrases",
});
table!(Unigram {
    get: (Handle, u32, *mut u32) -> bool = "token_get_unigram_frequency",
    increment: (Handle, u32, u32) -> bool = "token_add_unigram_frequency",
});

pub(crate) struct ContextPaths {
    data: CString,
    user: CString,
}
impl ContextPaths {
    pub(crate) fn new(data: &str, user: &Path) -> Result<Self> {
        Ok(Self {
            data: CString::new(data)?,
            user: CString::new(user.to_str().ok_or("user path is not UTF-8")?)?,
        })
    }
}

pub(crate) struct Context<'a> {
    pub(crate) handle: Handle,
    core: &'a Core,
}
impl<'a> Context<'a> {
    pub(crate) fn open(core: &'a Core, paths: &ContextPaths) -> Result<Self> {
        let handle = (core.init)(paths.data.as_ptr(), paths.user.as_ptr());
        require(!handle.is_null(), "fatal: pinyin_init returned NULL")?;
        Ok(Self { handle, core })
    }
}
impl Drop for Context<'_> {
    fn drop(&mut self) {
        (self.core.fini)(self.handle);
    }
}

pub(crate) struct Instance<'a> {
    pub(crate) handle: Handle,
    typing: &'a Typing,
}
impl<'a> Instance<'a> {
    pub(crate) fn open(typing: &'a Typing, ctx: Handle) -> Result<Self> {
        let handle = (typing.alloc)(ctx);
        require(!handle.is_null(), "fatal: alloc_instance returned NULL")?;
        Ok(Self { handle, typing })
    }
}
impl Drop for Instance<'_> {
    fn drop(&mut self) {
        (self.typing.free)(self.handle);
    }
}

pub(crate) struct StringValue {
    pointer: *mut c_char,
    release: Release,
}
impl StringValue {
    pub(crate) fn new(pointer: *mut c_char, release: Release) -> Self {
        Self { pointer, release }
    }
    pub(crate) fn text(&self) -> Result<Option<&str>> {
        text(self.pointer)
    }
}
impl Drop for StringValue {
    fn drop(&mut self) {
        (self.release)(self.pointer.cast());
    }
}

pub(crate) fn text<'a>(pointer: *const c_char) -> Result<Option<&'a str>> {
    if pointer.is_null() {
        return Ok(None);
    }
    // SAFETY: All callers pass subject-owned NUL-terminated strings and finish
    // observing them before reset, mutation, release, iterator advance or teardown.
    Ok(Some(unsafe { CStr::from_ptr(pointer) }.to_str()?))
}

pub(crate) struct UserDir {
    path: std::path::PathBuf,
    _temporary: Option<tempfile::TempDir>,
}
impl UserDir {
    /// The profile that a nonempty `PERF_USER_DIR` names, which is kept; otherwise a private
    /// temporary profile that is removed on drop.
    pub(crate) fn supplied_or_temporary(prefix: &str) -> Result<Self> {
        match std::env::var_os("PERF_USER_DIR").filter(|path| !path.is_empty()) {
            Some(path) => {
                let path = std::path::PathBuf::from(path);
                require(
                    path.is_dir(),
                    "PERF_USER_DIR must name an existing private profile directory",
                )?;
                Ok(Self {
                    path,
                    _temporary: None,
                })
            }
            None => Self::temporary(prefix),
        }
    }

    /// A private temporary profile that is removed on drop; `PERF_USER_DIR` is ignored.
    pub(crate) fn temporary(prefix: &str) -> Result<Self> {
        let root = crate::platform::temp_root();
        require(
            root.as_os_str().len() + prefix.len() + 8 < 4096,
            "fatal: TMPDIR path is too long",
        )?;
        let temporary = tempfile::Builder::new().prefix(prefix).tempdir_in(root)?;
        Ok(Self {
            path: temporary.path().to_path_buf(),
            _temporary: Some(temporary),
        })
    }
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}
