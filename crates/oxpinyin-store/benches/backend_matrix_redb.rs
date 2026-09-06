//! `backend_matrix` for the redb backend — see `support/mod.rs`.
#![allow(missing_docs)]

#[path = "support/mod.rs"]
mod support;

use oxpinyin_store::RedbStore;

fn main() {
    support::run::<RedbStore>("redb");
}
