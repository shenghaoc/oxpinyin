//! Implements the libzhuyin C API and ABI.
//!
//! Linux packages provide `libzhuyin.so.15`.
//! Native Rust interfaces are unsupported internal details.
//! Each build selects exactly one database backend. The default is `tkrzw`.
//!
//! ## Further reading
//!
//! - [C header](https://github.com/shenghaoc/oxpinyin/blob/main/zhuyin-capi/zhuyin.h)
//! - [Exported symbols](https://github.com/shenghaoc/oxpinyin/blob/main/zhuyin-capi/libzhuyin.ver)
//! - [C library packaging](https://github.com/shenghaoc/oxpinyin/blob/main/docs/packaging.md)
//! - [Backend selection](https://github.com/shenghaoc/oxpinyin/blob/main/docs/runbooks/backends.md#selecting-a-backend)
//! - [Supported external interfaces](https://github.com/shenghaoc/oxpinyin/blob/main/docs/constitution.md#external-interfaces)
//! - [Upstream pin](https://github.com/shenghaoc/oxpinyin/blob/main/tools/oracle/oracle-pin.txt)
//! - [Failure behaviour](https://github.com/shenghaoc/oxpinyin/blob/main/docs/findings/compatibility-policy.md#failure-behaviour)
//!
//! ## Aborts
//!
//! As in libpinyin, malformed or corrupt profile, library or database data can abort the
//! process. A failed database operation or an asserted read or write of stored data can
//! also abort. Individual function descriptions omit these cases.
//!
//! ## Safety
//!
//! The caller must uphold the C header contract. Each function's `# Safety`
//! section identifies the rules below that apply and gives any additional
//! requirements.
//!
//! - **Handles.** A context pointer must be null or point to a live context
//!   returned by [`context::zhuyin_init`]. [`context::zhuyin_fini`] ends that context's lifetime.
//!   The caller must not use the handle again, even if a later context gets
//!   the same address. An instance pointer is null or a live pointer returned
//!   by [`instance::zhuyin_alloc_instance`]. An iterator pointer is
//!   null or a pointer that [`iterators::zhuyin_begin_add_phrases`] returned and
//!   [`iterators::zhuyin_end_add_phrases`] has yet to free.
//! - **Strings.** A `const char *` argument is null or points to a
//!   NUL-terminated string that stays valid and unchanged for the call.
//! - **Out-parameters.** A pointer the function writes through is null or is
//!   valid and aligned for a write of its pointee type.
//! - **Serial calls.** Serialize calls that use the same context or its
//!   instances or iterators.
//!
//! ### Borrowed objects
//!
//! A candidate pointer is null or a pointer borrowed from a live instance by
//! [`candidates::zhuyin_get_candidate`]. The pointer becomes invalid when the instance's
//! candidate list is rebuilt or cleared, including by [`instance::zhuyin_reset`], or
//! when [`instance::zhuyin_free_instance`] frees the instance.
//!
//! A key pointer is null or points to a [`types::ChewingKey`] valid for reads. A
//! key-rest pointer is null or points to a [`types::ChewingKeyRest`] valid for reads.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![cfg_attr(not(test), deny(clippy::unreachable))]
#![expect(
    unsafe_code,
    reason = "the C ABI crate; every block carries a SAFETY comment (constitution §5)"
)]
#![warn(missing_docs)]

pub mod candidates;
pub mod config;
pub mod context;
pub mod cursor;
pub mod dict;
mod ffi;
pub mod instance;
pub mod iterators;
pub mod keys;
mod live;
pub mod parse;
pub mod phrase;
pub mod sentence;
mod state;
pub mod types;

#[cfg(feature = "tools")]
pub mod tool;

// C publication and conversion tests live in sibling tests.rs files and tests/it.
// The shared test helpers name the exported functions through the library's
// service modules, as the verification tests do.
#[cfg(test)]
extern crate self as oxzhuyin_capi;

#[cfg(test)]
mod test_support;
