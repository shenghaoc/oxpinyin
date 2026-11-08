//! Duplicate-text candidates before the cursor and LONGER against NORMAL rows
//! (#913), and addon promotion's token reuse (#762).

use std::collections::BTreeMap;
use std::ops::Bound;

use oxpinyin_store::{DefaultStore, RawReadStore as _};

use crate::facade;
use crate::fixtures::{gram_bytes, items_system, replace_user_bigram};
pub use crate::readers::GuessRows;
use crate::readers::{c, candidate_at, candidate_rows, guess_rows, lookup, row_of, tokens_of};
use crate::support::{CELL, Kit};

#[derive(Debug, PartialEq)]
pub struct BeforeCursor {
    pub parsed: usize,
    pub guessed: bool,
    pub ret: bool,
    pub rows: Vec<Option<(i32, String)>>,
    pub chosen: i32,
}

/// Imports 甲 and 乙, plants user bigrams, then lists the zhuyin facade's
/// before-cursor candidates and chooses 乙 (`zhuyin.cpp:1408-1415`, `:1630`).
pub fn before_cursor_listing(k: &mut Kit, dynamic: bool) -> BeforeCursor {
    let it = facade::begin_add_phrases(k.ctx(), 7);
    assert!(!it.is_null());
    for (text, reading) in [("甲", "ㄋㄧㄢˊ"), ("乙", "ㄏㄠˇ")] {
        assert!(facade::iterator_add_phrase(
            it,
            c(text).as_ptr(),
            c(reading).as_ptr(),
            1_000_000
        ));
    }
    facade::end_add_phrases(it);
    let first = *tokens_of(k, "甲").last().expect("甲");
    let second = *tokens_of(k, "乙").last().expect("乙");
    assert!(facade::save(k.ctx()));
    replace_user_bigram(
        k,
        &[
            (1, gram_bytes(1_000_000, &[(first, 1_000_000)])),
            (first, gram_bytes(1_000_000, &[(second, 1_000_000)])),
        ],
    );
    assert!(facade::set_options(
        k.ctx(),
        32 | if dynamic { 512 } else { 0 }
    ));
    let inst = k.inst();
    assert_eq!(
        facade::parse_more_full_pinyins(inst, c("nianhao").as_ptr()),
        7
    );
    let guessed = facade::guess_sentence(inst);
    assert!(guessed);
    let parsed = facade::parse_more_full_pinyins(inst, c("ni''hao").as_ptr());
    assert_eq!(parsed, 7);
    let ret = facade::guess_candidates_before_cursor(inst, parsed);
    let rows = candidate_rows(inst);
    let index = rows
        .iter()
        .position(|row| row.as_ref().is_some_and(|(_, text)| text == "乙"))
        .expect("the imported before-cursor row is missing");
    let candidate = candidate_at(inst, u32::try_from(index).expect("index")).expect("candidate");
    let chosen = facade::choose_candidate(inst, parsed, candidate);
    BeforeCursor {
        parsed,
        guessed,
        ret,
        rows,
        chosen,
    }
}

#[derive(Debug, PartialEq)]
pub struct Longer {
    pub parsed: usize,
    pub at_end: GuessRows,
    pub without_sentence: GuessRows,
    pub guess_sentence: bool,
    pub with_sentence: GuessRows,
    pub sentence_suppressed: GuessRows,
}

/// A system library holding two 甲乙 readings, then the pinyin facade's lists
/// for `xian` with the LONGER row (sort word 1) or without it (3)
/// (`pinyin.cpp:2130-2137`, `:2292-2293`).
pub fn longer_listing(k: &mut Kit, dynamic: bool, longer: bool) -> Longer {
    items_system(
        k,
        &[
            (&[30002, 20057], &[4148, 4480], 100),
            (&[30002, 20057], &[4532, 4139], 100),
        ],
        200,
        true,
    );
    replace_user_bigram(k, &[(1, gram_bytes(1, &[(0x0100_0001, 1)]))]);
    assert!(facade::set_options(
        k.ctx(),
        394 | if dynamic { 512 } else { 0 }
    ));
    let inst = k.inst();
    let parsed = facade::parse_more_full_pinyins(inst, c("xian").as_ptr());
    assert_eq!(parsed, 4);
    let sort = if longer { 1 } else { 3 };
    let at_end = guess_rows(inst, parsed, sort);
    let without_sentence = guess_rows(inst, 0, sort);
    let guess_sentence = facade::guess_sentence(inst);
    let with_sentence = guess_rows(inst, 0, sort & !1);
    let sentence_suppressed = guess_rows(inst, 0, sort);
    assert!(guess_sentence);
    Longer {
        parsed,
        at_end,
        without_sentence,
        guess_sentence,
        with_sentence,
        sentence_suppressed,
    }
}

/// One step of an addon promotion sequence.
#[derive(Debug, PartialEq)]
pub enum Step {
    Promote(String, i32),
    Tokens(BTreeMap<String, Vec<String>>),
    Save(bool),
    Mask(bool),
}

#[derive(Debug, PartialEq)]
pub struct Promotion {
    pub steps: Vec<Step>,
    pub files: BTreeMap<String, UserFileView>,
}

/// `token_reuse_files`' view of one user file.
#[derive(Debug, PartialEq)]
pub enum UserFileView {
    Rows(Vec<(String, String)>),
    NonEmpty(bool),
    Hex(String),
}

const ADDON_PROMOTE_INPUTS: [(&str, &str); 4] = [
    ("shuxue", "书学"),
    ("wuji", "武技"),
    ("wusheng", "武生"),
    ("wuxi", "武戏"),
];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `zero_item_padding`: a pinyin index value with every odd-length item's two
/// padding bytes zeroed (`pinyin_phrase3.h:180-196`).
fn zero_item_padding(key_hex: &str, value_hex: &str) -> String {
    let length = key_hex.len() / 4;
    if length.is_multiple_of(2) || value_hex.is_empty() {
        return value_hex.to_owned();
    }
    let mut value: Vec<u8> = (0..value_hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&value_hex[at..at + 2], 16).expect("hex"))
        .collect();
    let stride = 4 + 2 * length + 2;
    let complete = value.len() / stride * stride;
    for item in value[..complete].chunks_exact_mut(stride) {
        item[stride - 2..].fill(0);
    }
    hex(&value)
}

/// `token_reuse_files`: every user file, by the kind the old reader read it as.
fn token_reuse_files(k: &Kit) -> BTreeMap<String, UserFileView> {
    let mut files = BTreeMap::new();
    let mut names: Vec<String> = std::fs::read_dir(&k.user)
        .expect("user directory")
        .map(|entry| {
            entry
                .expect("user entry")
                .file_name()
                .into_string()
                .expect("UTF-8 name")
        })
        .collect();
    names.sort();
    for name in names {
        let path = k.user.join(&name);
        if !path.is_file() || name.ends_with(".tmp") {
            continue;
        }
        let raw = std::fs::read(&path).expect("user file");
        let view = if name == "user_phrase_index.bin" || name == "user_pinyin_index.bin" {
            let store = DefaultStore::open_read_only(&path).expect("a user index");
            let mut rows = Vec::new();
            store
                .range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
                    rows.push((hex(key), hex(value)));
                    Ok(())
                })
                .expect("read a user index");
            rows.sort();
            if name == "user_pinyin_index.bin" {
                rows = rows
                    .into_iter()
                    .map(|(key, value)| {
                        let value = zero_item_padding(&key, &value);
                        (key, value)
                    })
                    .collect();
            }
            UserFileView::Rows(rows)
        } else if CELL != "bdb" && name == "user_bigram.db" {
            UserFileView::NonEmpty(!raw.is_empty())
        } else if ["user.bin", "addon.bin", "network.bin"].contains(&name.as_str()) {
            let mut masked = raw.clone();
            masked[4..8].fill(0);
            UserFileView::Hex(hex(&masked))
        } else {
            UserFileView::Hex(hex(&raw))
        };
        files.insert(name, view);
    }
    files
}

/// Promotes addon rows (`pinyin.cpp:2542-2545`), saving and optionally masking
/// out a promoted token before the last one (`phrase_index.cpp:636-643`, `:677-690`).
pub fn addon_promote(k: &mut Kit, mask_value: Option<u32>) -> Promotion {
    let ctx = k.ctx();
    for index in [4, 5] {
        assert!(facade::load_addon_phrase_library(ctx, index));
    }
    let mut steps = Vec::new();
    let mut texts: Vec<&str> = Vec::new();
    let promote = |steps: &mut Vec<Step>, pinyin: &str| {
        let inst = facade::alloc_instance(ctx);
        facade::parse_more_full_pinyins(inst, c(pinyin).as_ptr());
        facade::guess_candidates(inst, 0, 0);
        let row = row_of(inst, 6).unwrap_or_else(|| panic!("no addon candidate for {pinyin}"));
        steps.push(Step::Promote(
            pinyin.to_owned(),
            facade::choose_candidate(inst, 0, row),
        ));
        facade::free_instance(inst);
    };
    let tokens = |texts: &[&str]| {
        let inst = facade::alloc_instance(ctx);
        let found = texts
            .iter()
            .map(|text| {
                (
                    (*text).to_owned(),
                    lookup(inst, text.as_bytes())
                        .iter()
                        .map(|token| format!("{token:#x}"))
                        .collect(),
                )
            })
            .collect();
        facade::free_instance(inst);
        Step::Tokens(found)
    };
    for (pinyin, text) in &ADDON_PROMOTE_INPUTS[..3] {
        promote(&mut steps, pinyin);
        texts.push(text);
        steps.push(tokens(&texts));
    }
    steps.push(Step::Save(facade::save(ctx)));
    if let Some(value) = mask_value {
        steps.push(Step::Mask(facade::mask_out(ctx, 0xffff_ffff, value)));
        steps.push(tokens(&texts));
    }
    promote(&mut steps, ADDON_PROMOTE_INPUTS[3].0);
    texts.push(ADDON_PROMOTE_INPUTS[3].1);
    steps.push(tokens(&texts));
    steps.push(Step::Save(facade::save(ctx)));
    Promotion {
        steps,
        files: token_reuse_files(k),
    }
}
