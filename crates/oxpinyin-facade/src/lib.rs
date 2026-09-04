//! The facade-orchestration layer shared by the pinyin and zhuyin C-ABI
//! facades.
//!
//! Both `oxpinyin-capi` and `oxpinyin-zhuyin-capi` drive the same
//! `oxpinyin-runtime` session assembly through the same laws — the
//! parse-mode state machine (`begin_parse`'s continuation rule, the three
//! batch-parse seams, the stored-parse originals), the candidate-snapshot
//! machinery's shared halves, the cursor/offset matrix laws, and the
//! prefix/sentence seam — which existed twice, once per crate, each copy
//! cited against the same upstream pin. This crate holds those laws once:
//! each facade keeps only its C marshalling and the laws that genuinely
//! differ (the candidate-type enums, the guess re-anchor policies, the
//! choose end-offset chains, the sentence-row display law).
//!
//! What this crate does **not** do is unify per-facade parity decisions.
//! Where the two facades' pins diverge — FORCE_TONE forwarding on the
//! chewing seam, the `PINYIN_CORRECT_ALL` mask on the one-key full-pinyin
//! probe — the shared law is parameterized and each facade passes its own
//! arm, so a divergence stays greppable instead of buried.
//!
//! No C types cross this boundary: the crates above this one own the
//! `#[repr(C)]` shapes, the pointer casts, and the CString snapshots.

// Constitution §4, mechanically: library builds may not unwrap, expect,
// or panic. Inline #[cfg(test)] modules are exempt (see the allow below
// their declaration); tests/, benches/ and examples/ are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod context;
mod cursor;
mod instance;
mod offsets;
mod parse;
mod predict;

pub use context::{ContextCore, LiveOptions};
pub use cursor::{KeyAt, SpanSource};
pub use instance::InstanceCore;
pub use offsets::{
    double_original_offset, double_session_offset, full_original_offset, full_session_offset,
    zhuyin_lookup_session_offset, zhuyin_original_offset, zhuyin_session_offset,
};
pub use parse::{ToneForwarding, double_scheme, full_scheme, zhuyin_scheme};
pub use predict::compute_prefixes;

/// The option word `pinyin_init` seeds (`PINYIN_INCOMPLETE`, and nothing
/// else) — the pinyin facade's distinguishing default.
pub const PINYIN_DEFAULT_OPTION_WORD: u32 = oxpinyin_core::PINYIN_INCOMPLETE;

/// The option word `zhuyin_init` seeds (`USE_TONE | FORCE_TONE`,
/// `zhuyin.cpp:272` at the pin) — the zhuyin facade's distinguishing
/// default: incomplete OFF, unlike `pinyin_init`.
pub const ZHUYIN_DEFAULT_OPTION_WORD: u32 = oxpinyin_core::USE_TONE | oxpinyin_core::FORCE_TONE;
