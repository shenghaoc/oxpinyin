//! A standalone benchmark of initialisation and startup: how long a libpinyin
//! or libzhuyin takes to become usable, and what it holds in memory while it
//! does, compared between the pinned libpinyin and oxpinyin.
//!
//! **Correctness before performance.** A timing is reported only for a cell whose
//! subjects first produced identical complete observations
//! ([`observe`]); otherwise the cell is NOT COMPARABLE and no ratio is printed.
//!
//! The crate depends on no other oxpinyin crate and on nothing outside this
//! workspace's lockfile: it loads each library by path with `libloading`
//! ([`ffi`], the only module with `unsafe`) and drives it through the
//! [`workload::Driver`] trait. One fresh child process per sample; the pin and an
//! oxpinyin build are never loaded into one process.
//!
//! What a figure here is, and is not: these are startup figures from a Rust
//! driver; they are not comparable with the C instruments' (`bisect --perf`,
//! `system-import-profile`), which are untouched.
#![deny(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![warn(missing_docs)]

pub mod child;
pub mod digest;
mod error;
pub mod fixtures;
// The one module that loads a shared object and calls through it. Every unsafe
// block in it is justified; see its module documentation.
#[allow(unsafe_code)]
pub mod ffi;
pub mod instrument;
pub mod memory;
pub mod observe;
pub mod orchestrate;
pub mod profile;
pub mod report;
pub mod sample;
pub mod stats;
pub mod workload;

pub use error::BenchError;
