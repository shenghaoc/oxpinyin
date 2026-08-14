//! C ABI subset of libpinyin's public API (50 live symbols).
//!
//! Every `#[unsafe(no_mangle)] pub extern "C" fn` matches the signature in
//! `libpinyin/src/pinyin.h` (tag 2.11.91) symbol-for-symbol. Bodies are
//! stubs returning error/null until wired to real `Session` state.
//!
//! Opaque handles cross the boundary as `*mut T` via `Box::into_raw` /
//! `Box::from_raw`. Every incoming pointer is null-checked before deref.
//! `// SAFETY:` documents each `unsafe` block.
#![allow(unsafe_code)]
#![warn(missing_docs)]

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
