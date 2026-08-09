//! Hand-written declarations for the pinned libpinyin public ABI.
//!
//! Frozen in `docs/findings/oracle-ffi-seam.md` against
//! `include/libpinyin-2.11.91/pinyin.h` with SHA-256
//! `e1138482d06766163608406fe1083539b21ff8c44ea04f329f3db0c78a312d47`, the
//! header installed by `tools/oracle/build-oracle.sh` and recorded as
//! `header_sha256` in the prefix manifest.
//!
//! # Soundness notes
//!
//! Every function below returns C `bool`, that is `_Bool`, one byte with values
//! 0 and 1. It is **not** GLib's `gboolean`, which is `gint` and four bytes.
//! Rust's `bool` is FFI-compatible with `_Bool`, so `bool` is the correct
//! return type. Declaring these as `c_int` would read undefined upper bits on
//! the SysV AMD64 ABI, where `_Bool` is returned in `AL`. Do not "simplify"
//! these signatures.
//!
//! The five handle types are opaque. Their layout is never assumed, they are
//! never constructed on the Rust side, and they are never dereferenced — the
//! pointers only travel back into libpinyin. Each carries a `PhantomData`
//! marker making it neither `Send`, `Sync`, nor `Unpin`, so a handle cannot be
//! moved across threads by accident.

use core::ffi::{c_char, c_uint, c_void};
use core::marker::{PhantomData, PhantomPinned};

/// Emulates an `extern type`: unsized-ish, never constructed, never read.
macro_rules! opaque_handle {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[repr(C)]
        pub struct $name {
            _data: [u8; 0],
            _marker: PhantomData<(*mut u8, PhantomPinned)>,
        }
    };
}

opaque_handle! {
    /// `pinyin_context_t` — one loaded model, shared by its instances.
    PinyinContext
}
opaque_handle! {
    /// `pinyin_instance_t` — one parse/lookup session over a context.
    PinyinInstance
}
opaque_handle! {
    /// `lookup_candidate_t` — a candidate borrowed from an instance.
    LookupCandidate
}
opaque_handle! {
    /// `ChewingKey` — a parsed syllable key borrowed from an instance.
    ChewingKey
}
opaque_handle! {
    /// `ChewingKeyRest` — byte positions for a key, borrowed from an instance.
    ChewingKeyRest
}

/// `pinyin_option_t`, defined as `guint32` in the pinned `novel_types.h`.
pub type PinyinOption = u32;

unsafe extern "C" {
    /// Creates a context. Returns NULL on failure.
    pub(crate) fn pinyin_init(
        systemdir: *const c_char,
        userdir: *const c_char,
    ) -> *mut PinyinContext;

    /// Destroys a context created by [`pinyin_init`].
    pub(crate) fn pinyin_fini(context: *mut PinyinContext);

    /// Sets the parser option word on a context.
    pub(crate) fn pinyin_set_options(context: *mut PinyinContext, options: PinyinOption) -> bool;

    /// Allocates an instance over a context. Returns NULL on failure.
    pub(crate) fn pinyin_alloc_instance(context: *mut PinyinContext) -> *mut PinyinInstance;

    /// Frees an instance allocated by [`pinyin_alloc_instance`].
    pub(crate) fn pinyin_free_instance(instance: *mut PinyinInstance);

    /// Clears parse and candidate state on an instance, keeping it allocated.
    pub(crate) fn pinyin_reset(instance: *mut PinyinInstance) -> bool;

    /// Parses full pinyin into the instance. Returns bytes accepted.
    pub(crate) fn pinyin_parse_more_full_pinyins(
        instance: *mut PinyinInstance,
        pinyins: *const c_char,
    ) -> usize;

    /// Returns the length of the parsed input prefix.
    pub(crate) fn pinyin_get_parsed_input_length(instance: *mut PinyinInstance) -> usize;

    /// Builds the candidate list at `offset` under `sort_option`.
    pub(crate) fn pinyin_guess_candidates(
        instance: *mut PinyinInstance,
        offset: usize,
        sort_option: c_uint,
    ) -> bool;

    /// Writes the uncapped candidate count.
    pub(crate) fn pinyin_get_n_candidate(instance: *mut PinyinInstance, num: *mut c_uint) -> bool;

    /// Borrows the candidate at `index` from the instance.
    pub(crate) fn pinyin_get_candidate(
        instance: *mut PinyinInstance,
        index: c_uint,
        candidate: *mut *mut LookupCandidate,
    ) -> bool;

    /// Borrows a candidate's UTF-8 string from the instance. Do not free it.
    pub(crate) fn pinyin_get_candidate_string(
        instance: *mut PinyinInstance,
        candidate: *mut LookupCandidate,
        utf8_str: *mut *const c_char,
    ) -> bool;

    /// Borrows the parsed key covering `offset`.
    pub(crate) fn pinyin_get_pinyin_key(
        instance: *mut PinyinInstance,
        offset: usize,
        key: *mut *mut ChewingKey,
    ) -> bool;

    /// Borrows the key-rest covering `offset`.
    pub(crate) fn pinyin_get_pinyin_key_rest(
        instance: *mut PinyinInstance,
        offset: usize,
        key_rest: *mut *mut ChewingKeyRest,
    ) -> bool;

    /// Writes the half-open byte range of a key-rest.
    pub(crate) fn pinyin_get_pinyin_key_rest_positions(
        instance: *mut PinyinInstance,
        key_rest: *mut ChewingKeyRest,
        begin: *mut u16,
        end: *mut u16,
    ) -> bool;

    /// Writes a newly allocated UTF-8 spelling of `key`. Release with [`g_free`].
    pub(crate) fn pinyin_get_pinyin_string(
        instance: *mut PinyinInstance,
        key: *mut ChewingKey,
        utf8_str: *mut *mut c_char,
    ) -> bool;

    /// Whether `key` is an incomplete (initial-only) key.
    pub(crate) fn pinyin_get_pinyin_is_incomplete(
        instance: *mut PinyinInstance,
        key: *mut ChewingKey,
    ) -> bool;

    /// GLib deallocator, for the owned strings libpinyin hands back.
    pub(crate) fn g_free(mem: *mut c_void);
}
