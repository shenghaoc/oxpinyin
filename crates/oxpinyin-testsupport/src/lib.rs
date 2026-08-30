//! Shared test support for the oxpinyin workspace.
//!
//! Test doubles that several crates' test suites need live here, as an
//! ordinary dependency of test code — never of shipping code. The crate has
//! one implementation rule: every item is exercised through the same
//! `oxpinyin-core` seams the shipping implementations use, so a test that
//! swaps this crate in exercises the real trait surface.
//!
//! The crate is a workspace member so `cargo test --workspace` always
//! compiles and runs its own tests; being a member costs nothing in a
//! shipping graph because no shipping crate depends on it.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod differential;
pub mod fixture;
pub mod model_cache;

pub use differential::{
    Manifest, PinDir, fnv1a64, locate_bin, locate_data, parse_estimate_stdout, parse_manifest,
};
pub use fixture::{FixtureDictionary, FixtureError, FixtureLanguageModel};
