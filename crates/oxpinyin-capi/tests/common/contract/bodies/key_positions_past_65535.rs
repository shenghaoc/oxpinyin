//! Key positions past byte 65535 (#1501): the `guint16` key rest.

use crate::facade;
use crate::readers::c;
use crate::support::{Kit, P};

/// `USE_TONE`.
const OPTIONS: u32 = 1 << 5;
const UNITS: usize = 32769;

/// Double pinyin under `MS2003` (2), or chewing under the standard keyboard (1):
/// `ni3` or `su3`, then 32769 units of `ni` or `su`.
pub fn parsed(k: &mut Kit) -> (usize, usize) {
    let ctx = k.ctx();
    assert!(facade::set_options(ctx, OPTIONS));
    let (head, unit): (&[u8], &[u8]) = if facade::ZHUYIN {
        (b"su3", b"su")
    } else {
        (b"ni3", b"ni")
    };
    let text = [head, &unit.repeat(UNITS)].concat();
    let consumed = if facade::ZHUYIN {
        assert!(facade::set_chewing_scheme(ctx, 1));
        facade::parse_more_chewings(k.inst(), c(&text).as_ptr())
    } else {
        assert!(facade::set_double_pinyin_scheme(ctx, 2));
        facade::parse_more_double_pinyins(k.inst(), c(&text).as_ptr())
    };
    let inst = k.inst();
    (consumed, facade::get_parsed_input_length(inst))
}

/// The key rest at `offset`: whether there is one, and then the key's two bytes
/// (hex), the positions accessor's answer, begin, end, the length accessor's
/// answer and the length.
#[derive(Debug, PartialEq)]
pub struct RestRow {
    pub ret: bool,
    pub detail: Option<(Option<String>, bool, u16, u16, bool, u16)>,
}

pub fn rest_row(k: &mut Kit, offset: usize) -> RestRow {
    let inst = k.inst();
    let mut rest: P = std::ptr::null_mut();
    let ret = if facade::ZHUYIN {
        facade::get_zhuyin_key_rest(inst, offset, &raw mut rest)
    } else {
        facade::get_pinyin_key_rest(inst, offset, &raw mut rest)
    };
    if !ret || rest.is_null() {
        return RestRow { ret, detail: None };
    }
    let mut key: P = std::ptr::null_mut();
    if facade::ZHUYIN {
        facade::get_zhuyin_key(inst, offset, &raw mut key);
    } else {
        facade::get_pinyin_key(inst, offset, &raw mut key);
    }
    let key_hex = (!key.is_null()).then(|| {
        // SAFETY: a non-NULL key points at the facade's two-byte ChewingKey.
        let bytes = unsafe { std::slice::from_raw_parts(key.cast::<u8>(), 2) };
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    });
    let (mut begin, mut end, mut length) = (0xABCD_u16, 0xABCD_u16, 0xABCD_u16);
    let (positions, lengthed) = if facade::ZHUYIN {
        (
            facade::get_zhuyin_key_rest_positions(inst, rest, &raw mut begin, &raw mut end),
            facade::get_zhuyin_key_rest_length(inst, rest, &raw mut length),
        )
    } else {
        (
            facade::get_pinyin_key_rest_positions(inst, rest, &raw mut begin, &raw mut end),
            facade::get_pinyin_key_rest_length(inst, rest, &raw mut length),
        )
    };
    RestRow {
        ret,
        detail: Some((key_hex, positions, begin, end, lengthed, length)),
    }
}

#[derive(Debug, PartialEq)]
pub struct Crossing {
    pub parsed: (usize, usize),
    pub rest_65535: RestRow,
}

/// The key at 65535, whose end wraps.
pub fn crossing(k: &mut Kit) -> Crossing {
    let parsed = parsed(k);
    Crossing {
        parsed,
        rest_65535: rest_row(k, 65535),
    }
}

#[derive(Debug, PartialEq)]
pub struct Past {
    pub rest_65537: RestRow,
}

/// The key at 65537, past the bound.
pub fn past(k: &mut Kit) -> Past {
    parsed(k);
    Past {
        rest_65537: rest_row(k, 65537),
    }
}
