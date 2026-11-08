//! The toned_keys_zhuyin_refusals family's case bodies: each answers the result document the old case returned.
//!
//! A tone digit on an initial-only key (batch2 group 12e, #525 row 4) and the
//! zhuyin cursor, key and offset refusals (inventory row 42, #525).

use crate::facade;
use crate::readers::{c, full_inst};
use crate::support::{J, Kit, P, UNTOUCHED};

/// `TONE_INCOMPLETE`: `USE_TONE | PINYIN_INCOMPLETE`.
const TONE_INCOMPLETE: u32 = (1 << 5) | (1 << 3);

/// `NO_PREPENDS`: `SORT_WITHOUT_SENTENCE_CANDIDATE | SORT_WITHOUT_LONGER_CANDIDATE`.
const NO_PREPENDS: u32 = 0x1 | 0x2;

/// `contract_case_helpers.FACADE_TOTAL`: the oracle data's loaded libraries, summed.
const FACADE_TOTAL: u32 = 51_051_831;

/// `toned_initial`: tone and incomplete options, then a fresh instance parsing `text`.
fn toned_initial(k: &mut Kit, text: &str) -> P {
    facade::set_options(k.ctx(), TONE_INCOMPLETE);
    let inst = k.alloc();
    facade::parse_more_full_pinyins(inst, c(text).as_ptr());
    inst
}

/// `toned_initial_guess`: the guess flag and candidate count.
fn toned_initial_guess(k: &mut Kit, text: &str, offset: usize, sort: u32) -> J {
    let inst = toned_initial(k, text);
    let ret = facade::guess_candidates(inst, offset, sort);
    let mut count = 0_u32;
    facade::get_n_candidate(inst, &raw mut count);
    obj! { "ret" => ret, "count" => count }
}

/// `zhuyin_chewing`: a fresh instance with su3cl3 parsed.
fn zhuyin_chewing(k: &mut Kit) -> P {
    let inst = k.alloc();
    facade::parse_more_chewings(inst, c("su3cl3").as_ptr());
    inst
}

/// A seeded offset out-parameter: `"untouched"` or its value.
fn offset_shown(value: usize) -> J {
    if value == UNTOUCHED as usize {
        "untouched".into()
    } else {
        value.into()
    }
}

/// An offset getter by its facade name.
fn offset_getter(name: &str) -> fn(P, usize, *mut usize) -> bool {
    match name {
        "get_left_zhuyin_offset" => facade::get_left_zhuyin_offset,
        "get_right_zhuyin_offset" => facade::get_right_zhuyin_offset,
        other => panic!("no offset getter {other}"),
    }
}

/// A key accessor by its facade name.
fn key_getter(name: &str) -> fn(P, usize, *mut P) -> bool {
    match name {
        "get_zhuyin_key" => facade::get_zhuyin_key,
        "get_zhuyin_key_rest" => facade::get_zhuyin_key_rest,
        other => panic!("no key accessor {other}"),
    }
}

/// A cursor guess by its facade name.
fn guesser(name: &str) -> fn(P, usize) -> bool {
    match name {
        "guess_candidates_after_cursor" => facade::guess_candidates_after_cursor,
        "guess_candidates_before_cursor" => facade::guess_candidates_before_cursor,
        other => panic!("no guess {other}"),
    }
}

/// `offset_call`: the offset getter's flag on a fresh or su3cl3 instance.
fn offset_call(k: &mut Kit, name: &str, fresh: bool, offset: usize) -> J {
    let inst = if fresh { k.alloc() } else { zhuyin_chewing(k) };
    let mut out = UNTOUCHED as usize;
    obj! { "ret" => offset_getter(name)(inst, offset, &raw mut out) }
}

/// `key_call`: the key accessor's flag on a fresh or su3cl3 instance.
fn key_call(k: &mut Kit, name: &str, fresh: bool, offset: usize) -> J {
    let inst = if fresh { k.alloc() } else { zhuyin_chewing(k) };
    let mut out = UNTOUCHED as usize as P;
    obj! { "ret" => key_getter(name)(inst, offset, &raw mut out) }
}

/// `guess_call`: the cursor guess's flag after su3cl3.
fn guess_call(k: &mut Kit, name: &str, offset: usize) -> J {
    let inst = zhuyin_chewing(k);
    obj! { "ret" => guesser(name)(inst, offset) }
}

/// `wrap_total_to_zero`: the delta that wraps the facade total to zero.
fn wrap_total_to_zero(inst: P) -> bool {
    facade::token_add_unigram_frequency(inst, (1 << 24) | 1, FACADE_TOTAL.wrapping_neg())
}

/// `toned-initial-neighbours`: a toned complete key and an untoned initial.
fn toned_initial_neighbours(k: &mut Kit) -> J {
    let mut out = J::map();
    facade::set_options(k.ctx(), TONE_INCOMPLETE);
    for text in ["ni4", "n", "nin", "ni4hao"] {
        let inst = k.alloc();
        let n = facade::parse_more_full_pinyins(inst, c(text).as_ptr());
        let sentence = facade::guess_sentence(inst);
        let candidates = facade::guess_candidates(inst, 0, 0);
        out.set(text, (n, sentence, candidates));
    }
    let inst = k.alloc();
    for text in ["n", "ni4", "ni"] {
        let mut key = 0_u16;
        let parsed = facade::parse_full_pinyin(inst, c(text).as_ptr(), (&raw mut key).cast());
        let value = key;
        let incomplete = facade::get_pinyin_is_incomplete(inst, (&raw mut key).cast());
        out.set(format!("incomplete {text}"), (parsed, value, incomplete));
    }
    out
}

/// `zhuyin-cursor-neighbours`: the zhuyin sites' graceful neighbours.
fn zhuyin_cursor_neighbours(k: &mut Kit) -> J {
    let inst = zhuyin_chewing(k);
    let mut res = UNTOUCHED as usize;
    let ret = facade::get_zhuyin_offset(inst, 99, &raw mut res);
    let mut out = obj! { "offset 99" => (ret, offset_shown(res)) };
    for (label, name, offset) in [
        ("left 0", "get_left_zhuyin_offset", 0),
        ("left 3", "get_left_zhuyin_offset", 3),
        ("right 0", "get_right_zhuyin_offset", 0),
        ("right 3", "get_right_zhuyin_offset", 3),
    ] {
        let mut res = UNTOUCHED as usize;
        let ret = offset_getter(name)(inst, offset, &raw mut res);
        out.set(label, (ret, offset_shown(res)));
    }
    for (label, name, offset) in [
        ("key 0", "get_zhuyin_key", 0),
        ("key 99", "get_zhuyin_key", 99),
        ("rest 3", "get_zhuyin_key_rest", 3),
    ] {
        let mut ptr = UNTOUCHED as usize as P;
        out.set(label, key_getter(name)(inst, offset, &raw mut ptr));
    }
    let mut left = 0_usize;
    out.set(
        "empty left",
        facade::get_left_zhuyin_offset(k.alloc(), 0, &raw mut left),
    );
    out.set(
        "empty guess",
        facade::guess_candidates_after_cursor(k.alloc(), 0),
    );
    out.set("guess 0", facade::guess_candidates_after_cursor(inst, 0));
    out.set(
        "guess end before",
        facade::guess_candidates_before_cursor(inst, 6),
    );
    out.set(
        "guess end after",
        facade::guess_candidates_after_cursor(inst, 6),
    );
    out
}

/// `zhuyin-scheme-and-library-neighbours`: scheme setters and library bounds.
fn scheme_and_library_neighbours(k: &mut Kit) -> J {
    let ctx = k.ctx();
    let mut out = J::map();
    for (label, full, arg) in [
        ("chewing 1", false, 1),
        ("chewing 6", false, 6),
        ("chewing 8", false, 8),
        ("chewing 9", false, 9),
        ("full 1", true, 1),
        ("full 3", true, 3),
    ] {
        let ret = if full {
            facade::set_full_pinyin_scheme(ctx, arg)
        } else {
            facade::set_chewing_scheme(ctx, arg)
        };
        out.set(label, ret);
    }
    for (label, load, arg) in [
        ("load 1", true, 1),
        ("load 7", true, 7),
        ("load 16", true, 16),
        ("load 255", true, 255),
        ("unload 1", false, 1),
    ] {
        let ret = if load {
            facade::load_phrase_library(ctx, arg)
        } else {
            facade::unload_phrase_library(ctx, arg)
        };
        out.set(label, ret);
    }
    out
}

/// The pinyin cases of the first registration.
fn pinyin_case(k: &mut Kit, id: &str) -> Option<J> {
    Some(match id {
        "abort-is-incomplete-toned-initial" => {
            facade::set_options(k.ctx(), TONE_INCOMPLETE);
            let inst = k.alloc();
            let mut key = 0_u16;
            facade::parse_full_pinyin(inst, c("n4").as_ptr(), (&raw mut key).cast());
            obj! { "ret" => facade::get_pinyin_is_incomplete(inst, (&raw mut key).cast()) }
        }
        "abort-guess-candidates-toned-initial-before-offset" => {
            let inst = toned_initial(k, "n4ni");
            obj! { "ret" => facade::guess_candidates(inst, 2, 0) }
        }
        "abort-guess-candidates-toned-initial" => {
            let inst = toned_initial(k, "n4");
            obj! { "ret" => facade::guess_candidates(inst, 0, 0) }
        }
        "abort-guess-candidates-toned-initial-no-prepends" => {
            toned_initial_guess(k, "n4", 0, NO_PREPENDS)
        }
        "guess-candidates-toned-initial-past-key-no-prepends" => {
            toned_initial_guess(k, "n4", 2, NO_PREPENDS)
        }
        "guess-candidates-toned-initial-before-offset-no-prepends" => {
            toned_initial_guess(k, "n4ni", 2, NO_PREPENDS)
        }
        "abort-guess-sentence-toned-initial" => {
            let inst = toned_initial(k, "n4");
            obj! { "ret" => facade::guess_sentence(inst) }
        }
        "abort-guess-sentence-toned-initial-after-key" => {
            let inst = toned_initial(k, "nihaon4");
            obj! { "ret" => facade::guess_sentence(inst) }
        }
        "toned-initial-neighbours" => toned_initial_neighbours(k),
        "abort-get-right-pinyin-offset-empty-matrix" => {
            let mut out = UNTOUCHED as usize;
            obj! { "ret" => facade::get_right_pinyin_offset(k.alloc(), 0, &raw mut out) }
        }
        "abort-guess-candidates-past-matrix" => {
            obj! { "ret" => facade::guess_candidates(full_inst(k, b"nihao"), 99, 0) }
        }
        _ => return None,
    })
}

/// The zhuyin cases that are not one of the call tables.
fn zhuyin_case(k: &mut Kit, id: &str) -> J {
    match id {
        "abort-zhuyin-get-character-offset-past-matrix" => {
            let mut out = UNTOUCHED as usize;
            let inst = zhuyin_chewing(k);
            obj! { "ret" => facade::get_character_offset(inst, c("你好").as_ptr(), 99, &raw mut out) }
        }
        "zhuyin-get-zhuyin-key-size-max-empty-matrix" => {
            key_call(k, "get_zhuyin_key", true, usize::MAX)
        }
        "zhuyin-get-zhuyin-key-rest-size-max-empty-matrix" => {
            key_call(k, "get_zhuyin_key_rest", true, usize::MAX)
        }
        "abort-zhuyin-parse-full-pinyin-apostrophe" => {
            let seed = (UNTOUCHED & 0xFFFF) as u16;
            let mut key = seed;
            let ret = facade::parse_full_pinyin(k.inst(), c("n'i").as_ptr(), (&raw mut key).cast());
            obj! { "ret" => ret, "untouched key" => key == seed }
        }
        "abort-zhuyin-guess-candidates-total-zero" => {
            let inst = zhuyin_chewing(k);
            wrap_total_to_zero(inst);
            obj! { "ret" => facade::guess_candidates_after_cursor(inst, 0) }
        }
        "zhuyin-cursor-neighbours" => zhuyin_cursor_neighbours(k),
        "zhuyin-scheme-and-library-neighbours" => scheme_and_library_neighbours(k),
        other => panic!("no case {other}"),
    }
}

/// The registered cases.
pub fn case(k: &mut Kit, id: &str) -> J {
    if let Some(out) = pinyin_case(k, id) {
        return out;
    }
    if let Some(rest) = id.strip_prefix("zhuyin-total-zero-ranks-nothing-") {
        let (name, offset) = match rest {
            "after-cursor" => ("guess_candidates_after_cursor", 6),
            "before-cursor" => ("guess_candidates_before_cursor", 0),
            other => panic!("no zero-total case {other}"),
        };
        let inst = zhuyin_chewing(k);
        wrap_total_to_zero(inst);
        let mut count = 0_u32;
        let ret = guesser(name)(inst, offset);
        facade::get_n_candidate(inst, &raw mut count);
        return obj! { "ret" => ret, "rows" => count };
    }
    let call = match id {
        "abort-zhuyin-get-left-zhuyin-offset-past-matrix" => {
            ("offset", "get_left_zhuyin_offset", false, 99)
        }
        "abort-zhuyin-get-right-zhuyin-offset-past-matrix" => {
            ("offset", "get_right_zhuyin_offset", false, 99)
        }
        "abort-zhuyin-get-right-zhuyin-offset-empty-matrix" => {
            ("offset", "get_right_zhuyin_offset", true, 0)
        }
        "abort-zhuyin-guess-candidates-after-cursor-past-matrix" => {
            ("guess", "guess_candidates_after_cursor", false, 99)
        }
        "abort-zhuyin-guess-candidates-before-cursor-past-matrix" => {
            ("guess", "guess_candidates_before_cursor", false, 99)
        }
        "abort-zhuyin-guess-candidates-after-cursor-reserved-slot-plus-one" => {
            ("guess", "guess_candidates_after_cursor", false, 7)
        }
        "abort-zhuyin-guess-candidates-before-cursor-reserved-slot-plus-one" => {
            ("guess", "guess_candidates_before_cursor", false, 7)
        }
        "abort-zhuyin-get-zhuyin-key-empty-matrix" => ("key", "get_zhuyin_key", true, 0),
        "abort-zhuyin-get-zhuyin-key-rest-empty-matrix" => ("key", "get_zhuyin_key_rest", true, 0),
        _ => return zhuyin_case(k, id),
    };
    match call {
        ("offset", name, fresh, offset) => offset_call(k, name, fresh, offset),
        ("guess", name, _, offset) => guess_call(k, name, offset),
        (_, name, fresh, offset) => key_call(k, name, fresh, offset),
    }
}
