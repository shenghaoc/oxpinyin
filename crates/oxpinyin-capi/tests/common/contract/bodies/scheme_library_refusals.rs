//! The scheme_library_refusals family's case bodies: each answers the result document the old case returned.
//!
//! #525: out-of-range schemes and library indexes, and get-next on an empty
//! export iterator, refused on both facades; #681 (row 62): get-next on a fresh
//! bigram iterator, which the pin asserts on (`pinyin.cpp:902`). The in-range
//! neighbours answer without a warning.

use std::ffi::c_char;

use crate::facade;
use crate::support::{J, Kit, P, UNTOUCHED};

/// One context call: the export and its argument.
#[derive(Clone, Copy)]
enum Call {
    /// A scheme setter and its `int` scheme.
    Scheme(fn(P, i32) -> bool, i32),
    /// A library loader or unloader and its `guint8` index.
    Library(fn(P, u8) -> bool, u8),
}

impl Call {
    fn run(self, ctx: P) -> bool {
        match self {
            Call::Scheme(call, scheme) => call(ctx, scheme),
            Call::Library(call, index) => call(ctx, index),
        }
    }
}

/// `register_pinyin`'s `abort-<name>` calls.
fn pinyin_call(name: &str) -> Option<Call> {
    Some(match name {
        "set-full-pinyin-scheme-0" => Call::Scheme(facade::set_full_pinyin_scheme, 0),
        "set-full-pinyin-scheme-4" => Call::Scheme(facade::set_full_pinyin_scheme, 4),
        "set-double-pinyin-scheme-30" => Call::Scheme(facade::set_double_pinyin_scheme, 30),
        "set-zhuyin-scheme-0" => Call::Scheme(facade::set_zhuyin_scheme, 0),
        "set-zhuyin-scheme-10" => Call::Scheme(facade::set_zhuyin_scheme, 10),
        "set-zhuyin-scheme-7" => Call::Scheme(facade::set_zhuyin_scheme, 7),
        "load-phrase-library-0" => Call::Library(facade::load_phrase_library, 0),
        "load-phrase-library-8" => Call::Library(facade::load_phrase_library, 8),
        "unload-phrase-library-16" => Call::Library(facade::unload_phrase_library, 16),
        "unload-addon-phrase-library-16" => Call::Library(facade::unload_addon_phrase_library, 16),
        _ => return None,
    })
}

/// `register_zhuyin`'s `abort-zhuyin-<name>` calls.
fn zhuyin_call(name: &str) -> Option<Call> {
    Some(match name {
        "chewing-scheme-0" => Call::Scheme(facade::set_chewing_scheme, 0),
        "chewing-scheme-7" => Call::Scheme(facade::set_chewing_scheme, 7),
        "chewing-scheme-10" => Call::Scheme(facade::set_chewing_scheme, 10),
        "chewing-scheme-minus-1" => Call::Scheme(facade::set_chewing_scheme, -1),
        "full-pinyin-scheme-0" => Call::Scheme(facade::set_full_pinyin_scheme, 0),
        "full-pinyin-scheme-4" => Call::Scheme(facade::set_full_pinyin_scheme, 4),
        "load-phrase-library-0" => Call::Library(facade::load_phrase_library, 0),
        "load-phrase-library-8" => Call::Library(facade::load_phrase_library, 8),
        "unload-phrase-library-16" => Call::Library(facade::unload_phrase_library, 16),
        "unload-phrase-library-255" => Call::Library(facade::unload_phrase_library, 255),
        _ => return None,
    })
}

/// `abort-iterator-get-next-phrase`: get-next on an empty library-7 iterator.
fn iterator_get_next(k: &mut Kit) -> J {
    let it = facade::begin_get_phrases(k.ctx(), 7);
    let (mut phrase, mut pinyin, mut count): (*mut c_char, *mut c_char, i32) =
        (std::ptr::null_mut(), std::ptr::null_mut(), 0);
    let has_next = facade::iterator_has_next_phrase(it);
    let ret =
        facade::iterator_get_next_phrase(it, &raw mut phrase, &raw mut pinyin, &raw mut count);
    obj! { "has next" => has_next, "ret" => ret }
}

/// `abort-bigram-iterator-get-next-phrase`: get-next on a fresh bigram iterator.
fn bigram_get_next(k: &mut Kit) -> J {
    let it = facade::begin_get_bigram_phrases(k.ctx());
    let mut phrase = UNTOUCHED as usize as *mut c_char;
    let mut pinyin = UNTOUCHED as usize as *mut c_char;
    let mut count = UNTOUCHED as i32;
    let has_next = facade::bigram_iterator_has_next_phrase(it);
    let ret = facade::bigram_iterator_get_next_phrase(
        it,
        &raw mut phrase,
        &raw mut pinyin,
        &raw mut count,
    );
    obj! {
        "has next" => has_next,
        "ret" => ret,
        "untouched phrase" => phrase as usize == UNTOUCHED as usize,
        "untouched pinyin" => pinyin as usize == UNTOUCHED as usize,
        "untouched count" => count == UNTOUCHED as i32,
    }
}

/// `scheme-and-library-neighbours`: every neighbour in order on one context.
fn neighbours(k: &mut Kit) -> J {
    let calls = [
        ("full 1", Call::Scheme(facade::set_full_pinyin_scheme, 1)),
        ("full 3", Call::Scheme(facade::set_full_pinyin_scheme, 3)),
        (
            "double 6",
            Call::Scheme(facade::set_double_pinyin_scheme, 6),
        ),
        (
            "double 99",
            Call::Scheme(facade::set_double_pinyin_scheme, 99),
        ),
        ("zhuyin 1", Call::Scheme(facade::set_zhuyin_scheme, 1)),
        ("zhuyin 9", Call::Scheme(facade::set_zhuyin_scheme, 9)),
        ("load 1", Call::Library(facade::load_phrase_library, 1)),
        ("load 7", Call::Library(facade::load_phrase_library, 7)),
        ("load 16", Call::Library(facade::load_phrase_library, 16)),
        ("load 255", Call::Library(facade::load_phrase_library, 255)),
        ("unload 1", Call::Library(facade::unload_phrase_library, 1)),
        (
            "unload 15",
            Call::Library(facade::unload_phrase_library, 15),
        ),
        (
            "unload addon 15",
            Call::Library(facade::unload_addon_phrase_library, 15),
        ),
        (
            "load addon 16",
            Call::Library(facade::load_addon_phrase_library, 16),
        ),
    ];
    let mut out = J::map();
    for (label, call) in calls {
        out.set(label, call.run(k.ctx()));
    }
    out
}

/// The family's cases by ID.
pub fn case(k: &mut Kit, id: &str) -> J {
    match id {
        "abort-iterator-get-next-phrase" => return iterator_get_next(k),
        "abort-bigram-iterator-get-next-phrase" => return bigram_get_next(k),
        "scheme-and-library-neighbours" => return neighbours(k),
        _ => {}
    }
    let call = if facade::ZHUYIN {
        id.strip_prefix("abort-zhuyin-").and_then(zhuyin_call)
    } else {
        id.strip_prefix("abort-").and_then(pinyin_call)
    }
    .unwrap_or_else(|| panic!("{id}: not this facade's case"));
    obj! { "ret" => call.run(k.ctx()) }
}
