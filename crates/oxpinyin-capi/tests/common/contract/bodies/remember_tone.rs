//! Remembered toned readings and the zhuyin facade's missing API.

use crate::facade;
use crate::readers::{ExportRow, c, remember_export};
use crate::support::Kit;

#[derive(Debug, PartialEq)]
pub struct Apis {
    pub remember_api: bool,
    pub export_api: bool,
}

/// Neither `zhuyin_remember_user_input` nor `zhuyin_begin_get_phrases` is in
/// `libzhuyin.ver`: the symbol table this facade links answers for it.
pub fn api_absent(_k: &mut Kit) -> Apis {
    Apis {
        remember_api: !facade::ZHUYIN,
        export_api: !facade::ZHUYIN,
    }
}

#[derive(Debug, PartialEq)]
pub struct Tones {
    pub remember_api: bool,
    pub export_api: bool,
    pub add: bool,
    pub parse: usize,
    pub guess: bool,
    pub remember: bool,
    pub export: Vec<ExportRow>,
}

/// Imports 你好你好, parses its toned spelling, guesses, remembers it with count
/// three and exports it; frees the instance and finalises the context.
pub fn remember_tones(k: &mut Kit) -> Tones {
    let ctx = k.ctx();
    let it = facade::begin_add_phrases(ctx, 7);
    let add = facade::iterator_add_phrase(
        it,
        c("你好你好").as_ptr(),
        c("ni3'hao3'ni3'hao3").as_ptr(),
        100_000,
    );
    facade::end_add_phrases(it);
    let inst = k.inst();
    let parse = facade::parse_more_full_pinyins(inst, c("ni3hao3ni3hao3").as_ptr());
    let guess = facade::guess_sentence(inst);
    let remember = facade::remember_user_input(inst, c("你好你好").as_ptr(), 3);
    let export = remember_export(k, 7, "你好你好");
    facade::free_instance(inst);
    facade::fini(ctx);
    k.forget();
    Tones {
        remember_api: true,
        export_api: true,
        add,
        parse,
        guess,
        remember,
        export,
    }
}

#[derive(Debug, PartialEq)]
pub struct Review {
    pub parse: usize,
    pub guess: bool,
    pub clear_tone: Option<bool>,
    pub remember: bool,
    pub export: Vec<ExportRow>,
}

/// With `USE_TONE`, parses a toned input (full pinyin, or double pinyin), guesses,
/// clears the options after guessing on the full-pinyin path, remembers with
/// count three and exports.
pub fn remember_review(k: &mut Kit, double: bool) -> Review {
    let ctx = k.ctx();
    facade::set_options(ctx, 0x20);
    let inst = k.inst();
    let (phrase, parse) = if double {
        (
            "你",
            facade::parse_more_double_pinyins(inst, c("ni3").as_ptr()),
        )
    } else {
        (
            "你好",
            facade::parse_more_full_pinyins(inst, c("ni3hao3").as_ptr()),
        )
    };
    let guess = facade::guess_sentence(inst);
    let clear_tone = (!double).then(|| facade::set_options(ctx, 0));
    let remember = facade::remember_user_input(inst, c(phrase).as_ptr(), 3);
    let export = remember_export(k, 7, phrase);
    facade::free_instance(inst);
    facade::fini(ctx);
    k.forget();
    Review {
        parse,
        guess,
        clear_tone,
        remember,
        export,
    }
}
