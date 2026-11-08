//! The C symbols the contract cases call, declared once for both facades.
//!
//! `contract-diff.py` reached each facade through `dlsym` of `<mode>_<name>`
//! (`contract_kit.Kit.fn`). Each facade's target does the same here: it invokes
//! [`facade_abi!`] with its prefix, which declares every symbol below under
//! `#[link_name = "<prefix><name>"]` and a plain wrapper of the same name, so
//! one case body serves both facades. The prototypes are `pinyin.h` and
//! `zhuyin.h` at pin 074a2219; every pointer argument is passed through as the
//! case gives it, NULL included.
#![allow(dead_code)]

/// Declares `$( fn name(args) -> ret; )*` as C symbols `<prefix>name`, with a
/// same-named wrapper for each.
macro_rules! facade_abi {
    ($prefix:literal; $( fn $name:ident ( $($arg:ident : $ty:ty),* $(,)? ) $(-> $ret:ty)? ; )*) => {
        mod raw {
            #[allow(unused_imports)]
            use super::*;
            unsafe extern "C" {
                $(
                    #[allow(dead_code)]
                    #[link_name = concat!($prefix, stringify!($name))]
                    pub fn $name($($arg: $ty),*) $(-> $ret)?;
                )*
            }
        }
        $(
            #[allow(dead_code, clippy::too_many_arguments)]
            pub fn $name($($arg: $ty),*) $(-> $ret)? {
                // SAFETY: the symbol has the facade header's prototype; the
                // arguments are what the frozen case passes, NULL and dangling
                // values included, as the old harness passed them through ctypes.
                unsafe { raw::$name($($arg),*) }
            }
        )*
    };
}

/// The symbols both facades export with one prototype.
macro_rules! shared_abi {
    ($prefix:literal $(; $($extra:tt)*)?) => {
        facade_abi! { $prefix;
            fn init(systemdir: *const c_char, userdir: *const c_char) -> P;
            fn fini(context: P);
            fn save(context: P) -> bool;
            fn alloc_instance(context: P) -> P;
            fn free_instance(instance: P);
            fn set_options(context: P, options: u32) -> bool;
            fn guess_sentence(instance: P) -> bool;
            fn guess_sentence_with_prefix(instance: P, prefix: *const c_char) -> bool;
            fn parse_more_full_pinyins(instance: P, pinyins: *const c_char) -> usize;
            fn parse_full_pinyin(instance: P, onepinyin: *const c_char, onekey: P) -> bool;
            fn parse_more_chewings(instance: P, chewings: *const c_char) -> usize;
            fn parse_chewing(instance: P, onechewing: *const c_char, onekey: P) -> bool;
            fn get_parsed_input_length(instance: P) -> usize;
            fn begin_add_phrases(context: P, index: u8) -> P;
            fn iterator_add_phrase(iter: P, phrase: *const c_char, pinyin: *const c_char, count: i32) -> bool;
            fn end_add_phrases(iter: P);
            fn get_n_candidate(instance: P, num: *mut u32) -> bool;
            fn get_candidate(instance: P, index: u32, candidate: *mut P) -> bool;
            fn get_candidate_type(instance: P, candidate: P, kind: *mut i32) -> bool;
            fn get_candidate_string(instance: P, candidate: P, utf8_str: *mut *const c_char) -> bool;
            fn choose_candidate(instance: P, offset: usize, candidate: P) -> i32;
            fn token_get_nth_pronunciation(instance: P, token: u32, nth: u32, keys: P) -> bool;
            fn token_get_n_pronunciation(instance: P, token: u32, num: *mut u32) -> bool;
            fn token_get_unigram_frequency(instance: P, token: u32, freq: *mut u32) -> bool;
            fn token_add_unigram_frequency(instance: P, token: u32, delta: u32) -> bool;
            fn token_get_phrase(instance: P, token: u32, len: *mut u32, utf8_str: *mut *mut c_char) -> bool;
            fn lookup_tokens(instance: P, phrase: *const c_char, tokenarray: P) -> bool;
            fn phrase_segment(instance: P, sentence: *const c_char) -> bool;
            fn get_n_phrase(instance: P, num: *mut u32) -> bool;
            fn get_phrase_token(instance: P, index: u32, token: *mut u32) -> bool;
            fn in_chewing_keyboard(instance: P, key: c_char, symbols: *mut *mut *mut c_char) -> bool;
            fn get_zhuyin_string(instance: P, key: P, utf8_str: *mut *mut c_char) -> bool;
            fn get_pinyin_string(instance: P, key: P, utf8_str: *mut *mut c_char) -> bool;
            fn mask_out(context: P, mask: u32, value: u32) -> bool;
            fn load_phrase_library(context: P, index: u8) -> bool;
            fn unload_phrase_library(context: P, index: u8) -> bool;
            fn clear_constraint(instance: P, offset: usize) -> bool;
            fn set_full_pinyin_scheme(context: P, scheme: i32) -> bool;
            $($($extra)*)?
        }
    };
}

/// Declares symbols the other facade exports and this one does not: a body
/// shared by both facades names them, and only that facade's cases reach them
/// (the old kit raised `AttributeError` for a missing symbol).
macro_rules! absent_abi {
    ($facade:literal; $( fn $name:ident ( $($arg:ident : $ty:ty),* $(,)? ) $(-> $ret:ty)? ; )*) => {
        $(
            #[allow(dead_code, unused_variables, clippy::too_many_arguments)]
            pub fn $name($($arg: $ty),*) $(-> $ret)? {
                unreachable!(concat!("the ", $facade, " facade exports no ", stringify!($name)))
            }
        )*
    };
}
