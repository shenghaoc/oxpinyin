//! `backend_matrix` for the tkrzw backend — see `support/mod.rs`.
#![allow(missing_docs)]
// The gate mirrors this target's `required-features`: cargo already skips
// the bench when the backend is not selected, but rust-analyzer analyzes
// required-features targets regardless and would flag `TkrzwStore`'s import
// as unresolved under every other backend's feature set.
#![cfg(feature = "tkrzw")]

#[path = "support/mod.rs"]
mod support;

use oxpinyin_store::TkrzwStore;

fn main() {
    support::run::<TkrzwStore>("tkrzw");
}
