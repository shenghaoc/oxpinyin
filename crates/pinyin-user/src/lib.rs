//! redb ACID store for user data: learning, frequencies, preferences.
//! The redb major version is pinned. Internal crate — the supported
//! public API is `pinyin-engine`.
//!
//! W6-T1 scope: the integer count [`seed`] arithmetic pinned in
//! `docs/findings/user-store.md` §2, the redb [`UserStore`] schema, and the
//! first [`pinyin_core::UserModel`] implementor. Persistence semantics (T5),
//! the user phrase index and token allocation (T2), Session/capi training
//! wiring (T3) and the decode-time additive merge (T4) are out of scope.
#![warn(missing_docs)]

pub mod seed;

mod model;
mod store;

pub use store::{SENTENCE_START, Token, UserStore, UserStoreError};
