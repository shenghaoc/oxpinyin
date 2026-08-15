//! C ABI subset of libpinyin's public API (50 live symbols).
//!
//! Every `#[unsafe(no_mangle)] pub extern "C" fn` matches the signature in
//! `libpinyin/src/pinyin.h` (tag 2.11.91) symbol-for-symbol.
//!
//! ## Panic discipline
//!
//! An unwind across `extern "C"` is undefined behaviour. Every entry
//! point wraps its body in [`ffi::ffi_catch`] which calls
//! [`std::panic::catch_unwind`], returning the sentinel value (false /
//! null / 0) on panic. The engine layer returns `Result` everywhere and
//! should never panic, but `catch_unwind` is the belt-and-suspenders
//! safety net at the ABI boundary.
//!
//! Opaque handles cross the boundary as `*mut T` via `Box::into_raw` /
//! `Box::from_raw`. Every incoming pointer is null-checked before deref.
//! `// SAFETY:` documents each `unsafe` block.
#![allow(unsafe_code)]
#![warn(missing_docs)]

mod ffi;
mod state;
mod types;

mod candidates;
mod config;
mod context;
mod cursor;
mod instance;
mod iterators;
mod parse;
mod sentence;
mod text;
mod user_data;

#[cfg(test)]
mod e2e_tests;
