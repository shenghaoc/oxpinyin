//! The committed `fixtures/w3` contexts and instances the instance, parse
//! and cursor tests share.

use crate::{ContextCore, InstanceCore, PINYIN_DEFAULT_OPTION_WORD, ZHUYIN_DEFAULT_OPTION_WORD};
use oxpinyin_user::UserConfLaw;
use std::path::PathBuf;

/// The committed fixture directory the compiled backend opens.
pub(crate) fn fixture_dir() -> PathBuf {
    let extension = oxpinyin_data::DEFAULT_STORE_EXT;
    fixture_root().join(extension)
}

pub(crate) fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("w3")
}

/// Opens the committed fixture set the compiled backend reads.
pub(crate) fn open_context(option_word: oxpinyin_core::OptionBits) -> ContextCore {
    ContextCore::open(&fixture_dir(), None, option_word, UserConfLaw::Pinyin)
        .expect("committed backend fixtures")
}

pub(crate) fn pinyin_instance() -> (ContextCore, InstanceCore) {
    let context = open_context(PINYIN_DEFAULT_OPTION_WORD);
    let instance = context.alloc_instance();
    (context, instance)
}

pub(crate) fn zhuyin_instance() -> (ContextCore, InstanceCore) {
    let context = open_context(ZHUYIN_DEFAULT_OPTION_WORD);
    let instance = context.alloc_instance();
    (context, instance)
}
