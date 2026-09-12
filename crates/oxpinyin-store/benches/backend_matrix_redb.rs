//! `backend_matrix` for the redb backend — see `support/mod.rs`.
#![allow(missing_docs)]
// The gate mirrors this target's `required-features`: cargo already skips
// the bench when the backend is not selected, but rust-analyzer analyzes
// required-features targets regardless and would flag `RedbStore`'s import
// as unresolved under every other backend's feature set.
#![cfg(feature = "redb")]

#[path = "support/mod.rs"]
mod support;

use oxpinyin_store::RedbStore;

fn main() {
    support::run::<RedbStore>("redb");
}
