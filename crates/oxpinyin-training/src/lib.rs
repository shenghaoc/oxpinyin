//! Private, nonshipping modules and command-line tools for the training pipeline.
//!
//! Each stage stays in a named module; the binary target names retain the
//! individual stage commands. Backend-sensitive stages share one feature
//! selection so a trainer build cannot mix table formats.
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]

pub mod counter;
pub mod emitter;
pub mod eval;
pub mod lambda;
pub mod segment;
pub mod train;
