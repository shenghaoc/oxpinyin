//! Python bindings for oxpinyin.
//!
//! The concrete engine assembly lives in `oxpinyin-runtime`, shared with
//! `oxpinyin-capi` so native consumers, the C ABI and this binding cannot
//! silently diverge. With the `bindings` feature this crate re-exposes that
//! assembly through `PyO3` as the `oxpinyin._native` extension module; see
//! `docs/python.md`. [`dump`] carries the corpus driver shared with the
//! Python-side parity tests.
// Constitution §4, mechanically: library builds may not unwrap, expect,
// or panic. Inline #[cfg(test)] modules are exempt (see the allow below
// their declaration); tests/, benches/ and examples/ are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![warn(missing_docs)]

pub mod dump;
// The zhuyin facade state machine the zhuyin binding translates. Pure Rust
// over the same runtime assembly — no Python dependency, so the parity
// driver links it like `dump` does.
pub mod zhuyin;

// The engine lock policy the binding acquires through. Compiled with the
// binding, and under `cfg(test)` regardless, so `cargo test -p
// oxpinyin-python` covers the policy without a Python toolchain in the
// process; skipped entirely otherwise, so it is never dead code.
#[cfg(any(feature = "bindings", test))]
mod lock;

#[cfg(feature = "bindings")]
mod binding;

#[cfg(feature = "bindings")]
mod zhuyin_binding;
