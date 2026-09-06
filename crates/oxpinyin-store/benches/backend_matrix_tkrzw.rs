//! `backend_matrix` for the tkrzw backend — see `support/mod.rs`.
#![allow(missing_docs)]

#[path = "support/mod.rs"]
mod support;

use oxpinyin_store::TkrzwStore;

fn main() {
    support::run::<TkrzwStore>("tkrzw");
}
