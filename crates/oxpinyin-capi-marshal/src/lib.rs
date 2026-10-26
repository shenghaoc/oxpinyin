//! Declarative marshalling macros shared by the two C-ABI facades
//! (`oxpinyin-capi` → `libpinyin.so.15`, `oxpinyin-zhuyin-capi` →
//! `libzhuyin.so.15`).
//!
//! The two facades are near-twins: ~73% of the zhuyin crate's marshalling
//! source is line-for-line identical to the pinyin crate's once the
//! `pinyin`/`zhuyin` token is folded. The genuinely mechanical,
//! doc-light boilerplate — opaque-handle pointer casts, and the
//! `char **`-out sentence writer — is stamped from these macros instead of
//! being written out by hand twice.
//!
//! ## Why the unsafe marshalling is macros, not functions
//!
//! Every helper stamped here is itself an `unsafe` edge (a raw-pointer
//! deref, `Box::from_raw`/`into_raw`, a `*out = ..` write). The
//! constitution's `unsafe` allowlist reaches only the C-ABI crates
//! (`capi`/`oracle`), and this crate must not carry compiled `unsafe` of
//! its own — so it is `#![forbid(unsafe_code)]`. A `macro_rules!` body is
//! only a token tree in the crate that *defines* it; the `unsafe` is
//! type-checked, borrow-checked and lint-checked in the crate that
//! *expands* it. Each generated `unsafe` block therefore lands in the
//! allowlisted capi crate, carrying its `// SAFETY:` comment, and this
//! crate stays unsafe-free. This is the same boundary the per-facade
//! `ffi.rs` files document: the allocator edge is not shared, and neither
//! is any compiled `unsafe` — only the *shape* of the unsafe marshalling.
//!
//! The safe [`input`] and [`phrase_text`] modules compile here: they convert
//! borrowed input to Rust values and encode [`oxpinyin_core::PhraseText`]
//! as the pin's C-facing bytes. Only pointer borrowing and output allocation
//! remain unsafe edges in the adapters.
//!
//! No exported C symbol is generated here: the `#[unsafe(no_mangle)]`
//! entry points keep their hand-written bodies in each crate, so the
//! `libpinyin.ver` (79) / `libzhuyin.ver` (52) export gates and the C++
//! header smoke test see an unchanged surface. These macros stamp only the
//! crate-internal Rust helpers those entry points call.
#![forbid(unsafe_code)]
#![warn(missing_docs)]
// Constitution §4, mechanically: library builds may not unwrap, expect,
// or panic. Inline #[cfg(test)] modules are exempt (see the allow below
// their declaration); tests/, benches/ and examples/ are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]

/// Stamps the opaque-handle marshalling helpers behind a facade's three C
/// handle types: the context, the instance, and the borrowed lookup
/// candidate.
///
/// Each facade exposes zero-sized `#[repr(C)]` marker types across the C
/// ABI (`pinyin_context_t *` etc.) whose pointers actually address a
/// heap-allocated backing struct (`CapiContext`/`CapiInstance`/
/// `CapiCandidate`). This macro centralises the eight cast/box helpers that
/// bridge the two representations so each call site stays readable and the
/// two facades cannot drift:
///
/// - `context_ref` / `context_mut` — `*mut Marker` → `&`/`&mut Backing`
/// - `instance_ref` / `instance_mut` — likewise for the instance handle
/// - `candidate_ref` — `*mut CandMarker` → `&CandBacking`
/// - `candidate_ptr` — `&CandBacking` → `*mut CandMarker`
/// - `box_context` / `box_instance` — `Backing` → `*mut Marker` (leaks a
///   `Box` for the caller to reclaim through its destructor entry point)
///
/// `$vis` is applied to every generated item, so a facade whose modules
/// reach these through `crate::state::*` passes `pub` and one that keeps
/// them crate-internal passes `pub(crate)`.
///
/// # Example
///
/// ```ignore
/// oxpinyin_capi_marshal::opaque_handle_casts! {
///     vis: pub,
///     context: PinyinContext => CapiContext,
///     instance: PinyinInstance => CapiInstance,
///     candidate: LookupCandidate => CapiCandidate,
/// }
/// ```
#[macro_export]
macro_rules! opaque_handle_casts {
    (
        vis: $vis:vis,
        context: $ctx_marker:ty => $ctx_backing:ty,
        instance: $inst_marker:ty => $inst_backing:ty,
        candidate: $cand_marker:ty => $cand_backing:ty $(,)?
    ) => {
        /// Casts a context handle pointer to a shared backing reference.
        ///
        /// # Safety
        ///
        /// `ptr` must be non-null and produced by
        /// `box_context` (`Box::into_raw(Box::new(<backing> { .. }))`). The
        /// returned reference must not outlive that `Box` (i.e. must not be
        /// used after the `fini` entry point reconstructs and drops it) and
        /// must not be stored in a longer-lived location.
        $vis unsafe fn context_ref<'a>(ptr: *mut $ctx_marker) -> &'a $ctx_backing {
            // SAFETY: Caller guarantees the pointer is valid for the chosen lifetime.
            unsafe { &*(ptr.cast::<$ctx_backing>()) }
        }

        /// Casts a context handle pointer to a unique backing reference.
        ///
        /// # Safety
        ///
        /// `ptr` must be non-null and produced by `box_context`. No other
        /// reference to the same context may exist, and the returned
        /// reference must not outlive the backing `Box` (must not be used
        /// after the `fini` entry point reconstructs and drops it) or be
        /// stored in a longer-lived location.
        $vis unsafe fn context_mut<'a>(ptr: *mut $ctx_marker) -> &'a mut $ctx_backing {
            // SAFETY: Caller guarantees the pointer is valid and unique for the chosen lifetime.
            unsafe { &mut *(ptr.cast::<$ctx_backing>()) }
        }

        /// Casts an instance handle pointer to a shared backing reference.
        ///
        /// # Safety
        ///
        /// `ptr` must be non-null and produced by `box_instance`. The
        /// returned reference must not outlive that `Box` (must not be used
        /// after the `free_instance` entry point reconstructs and drops it)
        /// and must not be stored in a longer-lived location.
        $vis unsafe fn instance_ref<'a>(ptr: *mut $inst_marker) -> &'a $inst_backing {
            // SAFETY: Caller guarantees the pointer is valid for the chosen lifetime.
            unsafe { &*(ptr.cast::<$inst_backing>()) }
        }

        /// Casts an instance handle pointer to a unique backing reference.
        ///
        /// # Safety
        ///
        /// `ptr` must be non-null and produced by `box_instance`. No other
        /// reference to the same instance may exist, and the returned
        /// reference must not outlive the backing `Box` (must not be used
        /// after the `free_instance` entry point reconstructs and drops it)
        /// or be stored in a longer-lived location.
        $vis unsafe fn instance_mut<'a>(ptr: *mut $inst_marker) -> &'a mut $inst_backing {
            // SAFETY: Caller guarantees the pointer is valid and unique for the chosen lifetime.
            unsafe { &mut *(ptr.cast::<$inst_backing>()) }
        }

        /// Converts a backing context into a handle pointer for return to C.
        $vis fn box_context(ctx: $ctx_backing) -> *mut $ctx_marker {
            ::std::boxed::Box::into_raw(::std::boxed::Box::new(ctx)).cast()
        }

        /// Converts a backing instance into a handle pointer for return to C.
        $vis fn box_instance(inst: $inst_backing) -> *mut $inst_marker {
            ::std::boxed::Box::into_raw(::std::boxed::Box::new(inst)).cast()
        }

        /// Casts a lookup-candidate handle pointer back to its backing.
        ///
        /// # Safety
        ///
        /// `ptr` must be non-null and point into an active backing
        /// `candidates` vec (produced by `candidate_ptr`).
        $vis unsafe fn candidate_ref<'a>(ptr: *mut $cand_marker) -> &'a $cand_backing {
            // SAFETY: Caller guarantees the pointer is valid for the chosen lifetime.
            unsafe { &*(ptr.cast::<$cand_backing>()) }
        }

        /// Returns a lookup-candidate handle pointer to a backing candidate.
        $vis const fn candidate_ptr(cand: &$cand_backing) -> *mut $cand_marker {
            (cand as *const $cand_backing as *mut $cand_backing).cast()
        }
    };
}

pub mod input;
pub mod phrase_text;
pub mod ucs4;

/// Stamps a facade's byte-oriented `write_owned_sentence_bytes` helper: the
/// `char **`-out sentence writer for UTF-8 and the pin's historical encoding
/// of non-scalar phrase words. The facade passes its `malloc_cstr_with`
/// allocator, which rejects interior NUL bytes.
///
/// # Example
///
/// ```ignore
/// oxpinyin_capi_marshal::write_owned_sentence_bytes!(crate::ffi::malloc_cstr_with);
/// ```
#[macro_export]
macro_rules! write_owned_sentence_bytes {
    ($malloc_cstr_with:path) => {
        /// Writes `text` through the caller-owned out-param: `false` on an
        /// empty text, interior NUL or allocation failure, with the out-param
        /// nulled on every failure path.
        fn write_owned_sentence_bytes(
            text: &[u8],
            sentence: *mut *mut ::std::os::raw::c_char,
        ) -> bool {
            if text.is_empty() {
                if !sentence.is_null() {
                    // SAFETY: Caller null-checks the out-param.
                    unsafe {
                        *sentence = ::std::ptr::null_mut();
                    }
                }
                return false;
            }
            if !sentence.is_null() {
                let owned = $malloc_cstr_with(text.len(), |out| {
                    out.copy_from_slice(text);
                    text.len()
                });
                // SAFETY: Null-checked above.
                unsafe {
                    *sentence = owned;
                }
                if owned.is_null() {
                    return false;
                }
            }
            true
        }
    };
}
