//! `backend_matrix` for the Kyoto Cabinet backend — see `support/mod.rs`.
#![allow(missing_docs)]

#[path = "support/mod.rs"]
mod support;

use oxpinyin_store::KcStore;

fn main() {
    support::run::<KcStore>("kyotocabinet");
}
