//! Location of the checksum-pinned model20 text export.
//!
//! Re-export of the canonical implementation, which lives in
//! `oxpinyin-testsupport` (`oxpinyin_testsupport::model_cache`) so the
//! capi benches can consume the same constants and helpers through a
//! dev-dependency instead of duplicating them — and without the forbidden
//! `oxpinyin-capi` → `pinyin-oracle` edge. This crate never ships; the
//! re-export keeps `pinyin_oracle::model_cache` and the crate-root
//! re-exports stable for its bins, tests, and benches.

pub use oxpinyin_testsupport::model_cache::*;
