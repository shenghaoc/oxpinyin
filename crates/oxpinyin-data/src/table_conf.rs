//! The bigram interpolation weight λ of the model's `table.conf`.
//!
//! [`crate::table_info::TableConf::load`] scans `lambda parameter:` with the
//! pin's `fscanf("%f")` (`src/storage/table_info.cpp:220-224`). The scanned
//! `gfloat` is used directly by the language model.

/// λ recorded for the pinned model in `data-formats.md` §3
/// (`lambda parameter:0.312699`), as an `f32`.
pub const PINNED_LAMBDA: f32 = 0.312_699;
