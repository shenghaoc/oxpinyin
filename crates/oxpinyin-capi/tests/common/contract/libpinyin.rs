//! libpinyin's export iterator in the zhuyin targets
//! ([`crate::support::libpinyin_export`]): libzhuyin has no export iterator, so a
//! saved zhuyin profile is exported through libpinyin, linked as a dev-dependency.
#![allow(dead_code)]

use std::ffi::c_char;

use crate::support::P;

facade_abi! { "pinyin_";
    fn init(systemdir: *const c_char, userdir: *const c_char) -> P;
    fn begin_get_phrases(context: P, index: u32) -> P;
    fn iterator_has_next_phrase(iter: P) -> bool;
    fn iterator_get_next_phrase(iter: P, phrase: *mut *mut c_char, pinyin: *mut *mut c_char, count: *mut i32) -> bool;
    fn end_get_phrases(iter: P);
}
