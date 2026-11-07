//! The remaining #542 C-string and lifetime cases.

use std::collections::BTreeMap;
use std::ffi::c_char;

use crate::facade;
use crate::readers::{c, candidate_at, full_inst, key_out, n_candidate, owned_out, seeded};
use crate::support::{Kit, UNTOUCHED};

#[derive(Debug, PartialEq)]
pub struct Segment {
    pub ret: bool,
    pub messages: Vec<Option<String>>,
}

/// `phrase_segment` of invalid UTF-8 (`pinyin.cpp:1446-1454`, `zhuyin.cpp:965-973`).
pub fn segment_invalid_utf8(k: &mut Kit) -> Segment {
    let ret = facade::phrase_segment(k.inst(), c(b"\xff\xfe").as_ptr());
    Segment {
        ret,
        messages: k.messages(),
    }
}

#[derive(Debug, PartialEq)]
pub struct TrainSave {
    pub parse: usize,
    pub guess: bool,
    pub train: bool,
    pub save: bool,
    pub files: Vec<String>,
}

/// Trains with no choice, then saves.
pub fn train_no_choose_save(k: &mut Kit) -> TrainSave {
    let inst = k.inst();
    let parse = if facade::ZHUYIN {
        facade::parse_more_chewings(inst, c("su3cl3").as_ptr())
    } else {
        facade::parse_more_full_pinyins(inst, c("nihao").as_ptr())
    };
    let guess = facade::guess_sentence(inst);
    let train = facade::train_top(inst);
    let save = facade::save(k.ctx());
    let mut files: Vec<String> = std::fs::read_dir(&k.user)
        .expect("user directory")
        .map(|entry| {
            entry
                .expect("user entry")
                .file_name()
                .into_string()
                .expect("UTF-8 name")
        })
        .collect();
    files.sort();
    TrainSave {
        parse,
        guess,
        train,
        save,
        files,
    }
}

/// The single-key parse of `n` and `ni3`.
pub fn parse_full_one_values(k: &mut Kit) -> BTreeMap<String, (bool, u16)> {
    ["n", "ni3"]
        .iter()
        .map(|text| ((*text).to_owned(), key_out(k, text.as_bytes())))
        .collect()
}

#[derive(Debug, PartialEq)]
pub struct AliasPrefix {
    pub with_prefix_borrowed_text: String,
    pub with_prefix: bool,
    pub sentences: Vec<(bool, Option<String>)>,
    pub owned_prefix_recovery: bool,
    pub owned_prefix_candidate_count: u32,
}

/// A sentence guess whose prefix is a candidate's own borrowed text, then a
/// prediction on a fresh instance.
pub fn alias_prefix_borrowed(k: &mut Kit) -> AliasPrefix {
    let inst = full_inst(k, b"nihao");
    facade::guess_sentence(inst);
    facade::guess_candidates(inst, 0, 30);
    let cand = candidate_at(inst, 0).expect("candidate 0");
    let mut text: *const c_char = UNTOUCHED as usize as *const c_char;
    assert!(facade::get_candidate_string(inst, cand, &raw mut text));
    let borrowed = crate::support::borrowed_text(text).expect("borrowed text");
    let with_prefix = facade::guess_sentence_with_prefix(inst, text);
    let sentences = (0..3)
        .map(|index| {
            let mut out = seeded();
            let ret = facade::get_sentence_at(inst, index, &raw mut out);
            (ret, owned_out(out))
        })
        .collect();
    let owned = full_inst(k, b"nihao");
    let recovery = facade::guess_predicted_candidates(owned, c("你好").as_ptr());
    AliasPrefix {
        with_prefix_borrowed_text: borrowed,
        with_prefix,
        sentences,
        owned_prefix_recovery: recovery,
        owned_prefix_candidate_count: n_candidate(owned),
    }
}
