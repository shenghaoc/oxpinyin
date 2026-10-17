//! Generic vocabulary and wire formats for observing the public libpinyin and
//! libzhuyin C ABIs.
//!
//! This crate is the shared foundation of the developer toolchain
//! (`docs/findings/tooling-consolidation-phase1.md`, stage 1). It knows the
//! public libpinyin interface and nothing about oxpinyin: it depends on no
//! oxpinyin crate, names no pin, no oracle prefix and no divergence register,
//! and never decides what counts as success. Those belong to `pinyin-oracle`
//! (the pinned oracle and oxpinyin's verification policy), which builds on this
//! crate and re-exports every item it used to define, so existing consumers are
//! unaffected.
//!
//! What lives here is only what was already generic in `pinyin-oracle`:
//!
//! - [`option`] — the `pinyin_option_t` bits and sort word of the public
//!   headers;
//! - [`observation`] — the candidate, segment and completeness vocabulary one
//!   parse reports;
//! - [`capture`] — the escaping rules and `pinyin-capture-v1` field notation.
//!
//! The crate is pure and portable: it compiles and its tests run on Linux,
//! macOS and Windows with no libpinyin installed.
#![forbid(unsafe_code)]
// Constitution §4, mechanically: library builds may not unwrap, expect, or
// panic. Inline #[cfg(test)] modules are exempt; tests/ are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![warn(missing_docs)]

pub mod capture;
pub mod observation;
pub mod option;
