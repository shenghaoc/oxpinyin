//! NULL arguments the zhuyin pin dereferences (#563, register row 41): the pin
//! dies of SIGSEGV after its stderr; oxpinyin answers the declared value.

use std::ffi::c_char;
use std::ptr::null_mut;

use crate::facade;
use crate::readers::{c, candidate_at};
use crate::support::{Kit, P, UNTOUCHED};

#[derive(Debug, PartialEq)]
pub struct Ret {
    pub ret: bool,
}

fn seeded_ptr<T>() -> *mut T {
    UNTOUCHED as usize as *mut T
}

/// The cached instance after `su3cl3`, and its first key rest.
fn first_rest(k: &mut Kit) -> (P, P) {
    let inst = k.inst();
    facade::parse_more_chewings(inst, c("su3cl3").as_ptr());
    let mut rest: P = seeded_ptr();
    assert!(facade::get_zhuyin_key_rest(inst, 0, &raw mut rest) && !rest.is_null());
    (inst, rest)
}

/// The cached instance after `su3cl3`, and its first after-cursor candidate.
fn first_candidate(k: &mut Kit) -> (P, P) {
    let inst = k.inst();
    facade::parse_more_chewings(inst, c("su3cl3").as_ptr());
    assert!(facade::guess_candidates_after_cursor(inst, 0));
    let cand = candidate_at(inst, 0).expect("candidate 0");
    assert!(!cand.is_null());
    (inst, cand)
}

/// A two-byte ChewingKey buffer.
fn key_buffer() -> u16 {
    0
}

pub fn add_phrase_library_16(k: &mut Kit) -> Ret {
    let it = facade::begin_add_phrases(k.ctx(), 16);
    assert!(!it.is_null(), "begin_add_phrases(16) answers a handle");
    let ret = facade::iterator_add_phrase(it, c("你好").as_ptr(), c("ㄋㄧˇ ㄏㄠˇ").as_ptr(), 1);
    facade::end_add_phrases(it);
    Ret { ret }
}

pub fn parse_chewing_null_instance(_k: &mut Kit) -> Ret {
    let mut key = key_buffer();
    Ret {
        ret: facade::parse_chewing(null_mut(), c("su3").as_ptr(), (&raw mut key).cast()),
    }
}

pub fn parse_chewing_null_text(k: &mut Kit) -> Ret {
    let mut key = key_buffer();
    Ret {
        ret: facade::parse_chewing(k.inst(), std::ptr::null(), (&raw mut key).cast()),
    }
}

pub fn parse_chewing_null_key(k: &mut Kit) -> Ret {
    Ret {
        ret: facade::parse_chewing(k.inst(), c("su3").as_ptr(), null_mut()),
    }
}

pub fn in_chewing_null_instance(_k: &mut Kit) -> Ret {
    let mut symbols: *mut *mut c_char = seeded_ptr();
    Ret {
        ret: facade::in_chewing_keyboard(null_mut(), b's' as c_char, &raw mut symbols),
    }
}

pub fn in_chewing_null_symbols(k: &mut Kit) -> Ret {
    Ret {
        ret: facade::in_chewing_keyboard(k.inst(), b's' as c_char, null_mut()),
    }
}

pub fn pinyin_string_null_instance(k: &mut Kit) -> Ret {
    let mut key = key_buffer();
    facade::parse_chewing(k.inst(), c("su3").as_ptr(), (&raw mut key).cast());
    let mut string: *mut c_char = seeded_ptr();
    Ret {
        ret: facade::get_pinyin_string(null_mut(), (&raw mut key).cast(), &raw mut string),
    }
}

pub fn zhuyin_string_null_key(k: &mut Kit) -> Ret {
    let mut string: *mut c_char = seeded_ptr();
    Ret {
        ret: facade::get_zhuyin_string(k.inst(), null_mut(), &raw mut string),
    }
}

pub fn zhuyin_string_null_out(k: &mut Kit) -> Ret {
    let mut key = key_buffer();
    facade::parse_chewing(k.inst(), c("su3").as_ptr(), (&raw mut key).cast());
    Ret {
        ret: facade::get_zhuyin_string(k.inst(), (&raw mut key).cast(), null_mut()),
    }
}

pub fn key_rest_positions_null_rest(k: &mut Kit) -> Ret {
    let (mut begin, mut end) = (0_u16, 0_u16);
    Ret {
        ret: facade::get_zhuyin_key_rest_positions(
            k.inst(),
            null_mut(),
            &raw mut begin,
            &raw mut end,
        ),
    }
}

pub fn key_rest_length_null_rest(k: &mut Kit) -> Ret {
    let (inst, _rest) = first_rest(k);
    let mut length = 0_u16;
    Ret {
        ret: facade::get_zhuyin_key_rest_length(inst, null_mut(), &raw mut length),
    }
}

pub fn key_rest_length_null_length(k: &mut Kit) -> Ret {
    let (inst, rest) = first_rest(k);
    Ret {
        ret: facade::get_zhuyin_key_rest_length(inst, rest, null_mut()),
    }
}

pub fn candidate_type_null_candidate(k: &mut Kit) -> Ret {
    let (inst, _cand) = first_candidate(k);
    let mut kind = 0_i32;
    Ret {
        ret: facade::get_candidate_type(inst, null_mut(), &raw mut kind),
    }
}

pub fn candidate_type_null_out(k: &mut Kit) -> Ret {
    let (inst, cand) = first_candidate(k);
    Ret {
        ret: facade::get_candidate_type(inst, cand, null_mut()),
    }
}

pub fn candidate_string_null_candidate(k: &mut Kit) -> Ret {
    let (inst, _cand) = first_candidate(k);
    let mut text: *const c_char = seeded_ptr::<c_char>();
    Ret {
        ret: facade::get_candidate_string(inst, null_mut(), &raw mut text),
    }
}

pub fn candidate_string_null_out(k: &mut Kit) -> Ret {
    let (inst, cand) = first_candidate(k);
    Ret {
        ret: facade::get_candidate_string(inst, cand, null_mut()),
    }
}

pub fn n_phrase_null_instance(_k: &mut Kit) -> Ret {
    let mut count = 0_u32;
    Ret {
        ret: facade::get_n_phrase(null_mut(), &raw mut count),
    }
}

pub fn n_phrase_null_out(k: &mut Kit) -> Ret {
    Ret {
        ret: facade::get_n_phrase(k.inst(), null_mut()),
    }
}

pub fn phrase_token_null_instance(_k: &mut Kit) -> Ret {
    let mut slot = 0_u32;
    Ret {
        ret: facade::get_phrase_token(null_mut(), 0, &raw mut slot),
    }
}

pub fn phrase_token_null_out(k: &mut Kit) -> Ret {
    Ret {
        ret: facade::get_phrase_token(k.inst(), 0, null_mut()),
    }
}
