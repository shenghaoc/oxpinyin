//! The allocator_register family's case bodies: each answers the result document the old case returned.
//!
//! #681: the eight "unexecuted" string-getter lines of the allocator registers
//! (E3-E10). Each getter's false branch is `0 == key->get_table_index()`
//! (`pinyin.cpp:2707-2762`, `zhuyin.cpp:1732-1760`); a conforming consumer reaches
//! it with the zero key the key getter answers at a separator column. The case
//! records the return and every out-parameter (`untouched` is the sentinel, `null`
//! a NULL write); the control calls the same getter on a real key.

use std::ffi::c_char;

use crate::facade;
use crate::readers::{c, owned_out};
use crate::support::{J, Kit, P, UNTOUCHED};

/// `alloc_register_case`: parse `text` with option word 0x1aa, guess, take the
/// key at `offset` and render it with `which`'s getter (`strings`: initial and final).
fn probe(k: &mut Kit, text: &str, offset: usize, which: &str) -> J {
    let inst = k.inst();
    facade::set_options(k.ctx(), 0x1aa);
    facade::parse_more_full_pinyins(inst, c(text).as_ptr());
    let mut out = obj! { "guess_sentence" => facade::guess_sentence(inst) };
    let mut key = UNTOUCHED as usize as P;
    let key_ret = if facade::ZHUYIN {
        facade::get_zhuyin_key(inst, offset, &raw mut key)
    } else {
        facade::get_pinyin_key(inst, offset, &raw mut key)
    };
    out.set("key_ret", key_ret);
    if !key_ret || key.is_null() {
        return out;
    }
    // SAFETY: the key getter answered a live key, whose first two bytes are its words.
    let bytes = unsafe { std::slice::from_raw_parts(key.cast::<u8>(), 2) };
    out.set(
        "key_bytes",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
    );
    let untouched = || UNTOUCHED as usize as *mut c_char;
    if which == "strings" {
        let (mut initial, mut fin) = (untouched(), untouched());
        out.set(
            "ret",
            facade::get_pinyin_strings(inst, key, &raw mut initial, &raw mut fin),
        );
        out.set("shengmu", owned_out(initial));
        out.set("yunmu", owned_out(fin));
    } else {
        let mut string = untouched();
        let getter: fn(P, P, *mut *mut c_char) -> bool = match which {
            "pinyin" => facade::get_pinyin_string,
            "zhuyin" => facade::get_zhuyin_string,
            "luoma_pinyin" => facade::get_luoma_pinyin_string,
            "secondary_zhuyin" => facade::get_secondary_zhuyin_string,
            other => panic!("no getter {other}"),
        };
        out.set("ret", getter(inst, key, &raw mut string));
        out.set("out", owned_out(string));
    }
    out
}

/// `alloc-register-{mode}_get_{which}-{zero,real}-key`.
pub fn case(k: &mut Kit, id: &str) -> J {
    let (text, zero) = if facade::ZHUYIN {
        ("xi1'", 3)
    } else {
        ("xi'", 2)
    };
    let rest = id
        .strip_prefix(if facade::ZHUYIN {
            "alloc-register-zhuyin_get_"
        } else {
            "alloc-register-pinyin_get_"
        })
        .unwrap_or_else(|| panic!("no case {id}"));
    let (which, offset) = if let Some(which) = rest.strip_suffix("-zero-key") {
        (which, zero)
    } else {
        (
            rest.strip_suffix("-real-key")
                .unwrap_or_else(|| panic!("no case {id}")),
            0,
        )
    };
    probe(k, text, offset, which)
}
