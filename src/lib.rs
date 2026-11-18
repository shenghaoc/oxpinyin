//! Pinyin and zhuyin parsing, decoding, model contracts and native I/O.
#![deny(unsafe_code)]
#![warn(missing_docs)]
// Constitution §4: keep these non-test denials. A permitted site needs the
// narrowest justified #[expect], citing the pin's abort site or an invariant
// making failure unreachable; see docs/rust-conventions.md.
// An expectation proves lint coverage, not correspondence with the pin.
// #[cfg(test)] modules are exempt; tests/, benches/ and examples/
// are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![cfg_attr(not(test), deny(clippy::unreachable))]

// The fixture doubles name this crate by its package name.
#[cfg(test)]
extern crate self as oxpinyin;

#[forbid(unsafe_code)]
mod chewing_key;
#[forbid(unsafe_code)]
mod chewing_key_data;
#[forbid(unsafe_code)]
pub mod data;
#[forbid(unsafe_code)]
pub mod direct;
#[forbid(unsafe_code)]
pub mod engine;
#[forbid(unsafe_code)]
pub mod facade;
#[cfg(test)]
#[path = "../tests/common/fixture_doubles.rs"]
#[forbid(unsafe_code)]
mod fixture_doubles;
#[forbid(unsafe_code)]
mod full_pinyin_index;
#[forbid(unsafe_code)]
pub mod graph;
pub mod input;
#[forbid(unsafe_code)]
pub mod marshal;
#[forbid(unsafe_code)]
pub mod model;
#[forbid(unsafe_code)]
pub mod options;
#[forbid(unsafe_code)]
mod parser;
#[cfg(test)]
#[path = "../tests/common/parser_rows.rs"]
#[forbid(unsafe_code)]
mod parser_rows;
#[forbid(unsafe_code)]
mod scheme;
#[forbid(unsafe_code)]
pub mod scoring;
#[forbid(unsafe_code)]
pub mod sentence_facts;
#[forbid(unsafe_code)]
pub mod single_gram;
#[forbid(unsafe_code)]
mod static_strings;
pub mod store;
#[cfg(feature = "tools")]
#[forbid(unsafe_code)]
pub mod tool;
#[forbid(unsafe_code)]
pub mod ucs4;
#[forbid(unsafe_code)]
pub mod user;
#[forbid(unsafe_code)]
mod string_tables {
    use crate::options::table_row::{
        ZHUYIN_CORRECT_ETEN26, ZHUYIN_CORRECT_HSU, ZHUYIN_CORRECT_SHUFFLE, ZHUYIN_INCOMPLETE,
    };
    include!(concat!(env!("OUT_DIR"), "/string_tables.rs"));
}
#[forbid(unsafe_code)]
mod syllables;
#[forbid(unsafe_code)]
mod vocab;
#[forbid(unsafe_code)]
mod zhuyin_map;

pub use chewing_key::{CHEWING_ZERO_TONE, ChewingKey, KeyText};
pub use full_pinyin_index::{
    FullPinyinIndex, FullPinyinIndexKey, FullPinyinIndexParse, LUOMA_PINYIN_COUNT,
    SECONDARY_ZHUYIN_COUNT, luoma_pinyin_index, parse_full_pinyin_index_with_options,
    secondary_zhuyin_index,
};
pub use options::OptionBits;
pub use parser::{Completeness, FullPinyinParser};
pub use scheme::{
    DoublePinyinKey, DoublePinyinParse, DoublePinyinParser, DoublePinyinScheme, FullPinyinScheme,
    ZhuyinKey, ZhuyinParse, ZhuyinParser, ZhuyinScheme, parse_zhuyin_direct,
};
pub use syllables::{
    FULL_PINYIN_SYLLABLE_COUNT, INCOMPLETE_PINYIN_KEY_COUNT, INCOMPLETE_PINYIN_KEYS,
    MAX_SYLLABLE_LEN, OPTION_ONLY_COMPLETE_SYLLABLE_COUNT, OPTION_ONLY_COMPLETE_SYLLABLES,
    ZHUYIN_ONLY_COMPLETE_SYLLABLE_COUNT, ZHUYIN_ONLY_COMPLETE_SYLLABLES,
    canonical_complete_syllable, full_pinyin_syllable, full_pinyin_syllables,
    option_alias_canonical,
};
pub use ucs4::{OutwardWords, PhraseText, PhraseTextConcat};
pub use vocab::{PhraseEntry, PhraseToken, SyllableKey, fuzzy_chewing_alternatives};
pub(crate) use vocab::{fuzzy_chewing_finals, fuzzy_chewing_initials};

pub use single_gram::GramFault;
