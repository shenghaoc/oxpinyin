//! Safe input conversion and phrase encoding for the two C facades.

mod input;
mod phrase_text;
mod ucs4;

pub use input::{direct_zhuyin_reading, parser_prefix, scalar_text, utf8_to_ucs4};
pub use phrase_text::{phrase_text_bytes, phrase_text_len};
pub(crate) use ucs4::decode;
pub use ucs4::prefix_text;
