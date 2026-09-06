//! `backend_matrix` for the LMDB backend — see `support/mod.rs`.
#![allow(missing_docs)]

#[path = "support/mod.rs"]
mod support;

use oxpinyin_store::LmdbStore;

fn main() {
    support::run::<LmdbStore>("lmdb");
}
