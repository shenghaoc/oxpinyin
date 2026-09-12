//! `backend_matrix` for the LMDB backend — see `support/mod.rs`.
#![allow(missing_docs)]
// The gate mirrors this target's `required-features`: cargo already skips
// the bench when the backend is not selected, but rust-analyzer analyzes
// required-features targets regardless and would flag `LmdbStore`'s import
// as unresolved under every other backend's feature set.
#![cfg(feature = "lmdb")]

#[path = "support/mod.rs"]
mod support;

use oxpinyin_store::LmdbStore;

fn main() {
    support::run::<LmdbStore>("lmdb");
}
