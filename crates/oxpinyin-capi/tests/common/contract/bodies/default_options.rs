//! The default and explicit option words of the full-pinyin parsers (#532).

use std::collections::BTreeMap;

use crate::facade;
use crate::readers::{consumed, key_out};
use crate::support::Kit;

const PARSE: [&str; 6] = ["su3cl3", "ni2hao", "n", "zzz", "nihao", "ni3"];
const EXPLICIT: [&str; 5] = ["su3cl3", "ni2hao", "n", "zzz", "nihao"];

/// Each spelling's consumed bytes on a fresh instance, under `pinyin_init`'s word.
pub fn default_parse(k: &mut Kit) -> BTreeMap<String, usize> {
    PARSE
        .iter()
        .map(|text| {
            (
                (*text).to_owned(),
                consumed(k, facade::parse_more_full_pinyins, text.as_bytes()),
            )
        })
        .collect()
}

/// The single-key parse of each spelling on the cached instance.
pub fn default_single_key(k: &mut Kit) -> BTreeMap<String, (bool, u16)> {
    ["n", "ni3", "ni", "a1", "zhang", "zzz"]
        .iter()
        .map(|text| ((*text).to_owned(), key_out(k, text.as_bytes())))
        .collect()
}

/// The consumed bytes after `set_options(options)`.
pub fn explicit_parse(k: &mut Kit, options: u32) -> BTreeMap<String, usize> {
    let ctx = k.ctx();
    facade::set_options(ctx, options);
    EXPLICIT
        .iter()
        .map(|text| {
            (
                (*text).to_owned(),
                consumed(k, facade::parse_more_full_pinyins, text.as_bytes()),
            )
        })
        .collect()
}
