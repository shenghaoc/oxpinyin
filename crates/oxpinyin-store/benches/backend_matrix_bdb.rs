//! `backend_matrix` for the Berkeley DB backend — see `support/mod.rs`.
#![allow(missing_docs)]

#[path = "support/mod.rs"]
mod support;

use oxpinyin_store::BdbStore;

fn main() {
    support::run::<BdbStore>("bdb");
}
