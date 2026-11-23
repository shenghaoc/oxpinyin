//! Native I/O and adapters for the pin-compatible command-line tools.

#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![cfg_attr(not(test), deny(clippy::unreachable))]
#![cfg_attr(not(test), deny(clippy::multiple_unsafe_ops_per_block))]

#[expect(
    unsafe_code,
    reason = "native utility I/O at the C runtime and GLib boundary"
)]
pub mod input;
#[expect(
    unsafe_code,
    reason = "the REPL driver calls the Pinyin ABI with its own handles"
)]
pub mod pinyin;
#[expect(
    unsafe_code,
    reason = "the REPL driver calls the Zhuyin ABI with its own handles"
)]
pub mod zhuyin;

#[forbid(unsafe_code)]
pub mod data;
#[forbid(unsafe_code)]
pub mod flexible_ngram;
#[forbid(unsafe_code)]
pub mod lookup;
#[forbid(unsafe_code)]
pub mod parser;

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../src/store/death_test_support.rs"]
mod death_test;
