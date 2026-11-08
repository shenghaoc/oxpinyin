//! The user phrase table's token arrays under `mask_out`, removal and reload
//! (#759, #785, #799, #1418).

use std::ops::Bound;

use oxpinyin_store::{DefaultStore, RawReadStore as _};

use crate::facade;
use crate::fixtures::{craft_tree, ucs4_key, write_user_conf};
pub use crate::readers::Predicted;
use crate::readers::{
    add_user_phrase, c, candidate_at, candidate_type, export_phrases, lookup, n_candidate, reopen,
    row58_rows, tokens_of,
};
use crate::support::Kit;

/// `TOKEN_REUSE_PHRASES`: rare two-character phrases and their readings.
const PHRASES: [(&str, &str); 4] = [
    ("龘龘", "da2'da2"),
    ("龖龖", "ta4'ta4"),
    ("靐靐", "bing3'bing3"),
    ("鱻鱻", "xian1'xian1"),
];

/// `phrase_table_lookup`: `lookup_tokens` of `text` on the cached instance.
fn table_lookup(k: &mut Kit, text: &str) -> (bool, Vec<u32>) {
    let inst = k.inst();
    // SAFETY: a fresh GArray of four-byte elements, freed below.
    let array = unsafe { glib_sys::g_array_new(0, 0, 4) };
    let ret = facade::lookup_tokens(inst, c(text).as_ptr(), array.cast());
    let tokens = crate::readers::garray_u32(array);
    // SAFETY: the array was allocated above and is freed once.
    unsafe { glib_sys::g_array_free(array, 1) };
    (ret, tokens)
}

/// `index_file_rows`: a saved tree DBM's rows as sorted hex pairs, or the
/// failure's kind where the file does not open.
fn index_rows(path: &std::path::Path) -> Vec<(String, String)> {
    let hex = |bytes: &[u8]| {
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let store = DefaultStore::open_read_only(path).expect("the saved index opens");
    let mut rows = Vec::new();
    store
        .range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
            rows.push((hex(key), hex(value)));
            Ok(())
        })
        .expect("read the saved index");
    rows.sort();
    rows
}

pub type Export = Vec<(bool, Option<String>, Option<String>, i32)>;

#[derive(Debug, PartialEq)]
pub struct Masked {
    pub loaded: Vec<(bool, Vec<u32>)>,
    pub export_before: Export,
    pub masked: bool,
    pub trained: bool,
    pub saved: bool,
    pub stored: Vec<(String, String)>,
    pub export_after: Export,
    pub reopened: Vec<(bool, Vec<u32>)>,
    pub export_reopened: Export,
}

/// Saves four phrases, replaces the user phrase index with a long record (three
/// tokens, `duplicates` repeating the first) and a short one, masks out a token
/// (the middle one, or one matching none), trains and saves
/// (`phrase_large_table3.cpp:102-116`, `memory_chunk.h:290-295`).
pub fn mask_records(k: &mut Kit, duplicates: bool, remove_middle: bool) -> Masked {
    for (text, reading) in PHRASES {
        add_user_phrase(k, text, &[reading.as_bytes()], 100);
    }
    let tokens: Vec<u32> = PHRASES
        .iter()
        .map(|(text, _)| table_lookup(k, text).1[0])
        .collect();
    assert!(facade::save(k.ctx()));
    k.close();
    let mut keys: Vec<Vec<u8>> = PHRASES.iter().map(|(text, _)| ucs4_key(text)).collect();
    keys.sort();
    let long: Vec<u32> = if duplicates {
        vec![tokens[0], tokens[0], tokens[1]]
    } else {
        let mut long = tokens[..3].to_vec();
        long.sort_unstable();
        long
    };
    let rows = [
        (
            keys[0].clone(),
            long.iter()
                .flat_map(|token| token.to_le_bytes())
                .collect::<Vec<u8>>(),
        ),
        (keys[1].clone(), tokens[3].to_le_bytes().to_vec()),
    ];
    let path = k.user.join("user_phrase_index.bin");
    craft_tree(&path, &rows);
    write_user_conf(k);
    k.ctx();
    let texts: Vec<String> = rows.iter().map(|(key, _)| decode_ucs4(key)).collect();
    let loaded = texts.iter().map(|text| table_lookup(k, text)).collect();
    let export_before = export_phrases(k);
    let masked_token = if remove_middle {
        tokens[1]
    } else {
        117_506_046
    };
    let masked = facade::mask_out(k.ctx(), 0xffff_ffff, masked_token);
    assert!(facade::set_options(k.ctx(), 1 << 5));
    assert_eq!(
        facade::parse_more_full_pinyins(k.inst(), c("wo").as_ptr()),
        2
    );
    assert!(facade::guess_sentence(k.inst()));
    let trained = facade::train_top(k.inst());
    let saved = facade::save(k.ctx());
    let stored = index_rows(&path);
    let export_after = export_phrases(k);
    reopen(k);
    let reopened = texts.iter().map(|text| table_lookup(k, text)).collect();
    let export_reopened = export_phrases(k);
    Masked {
        loaded,
        export_before,
        masked,
        trained,
        saved,
        stored,
        export_after,
        reopened,
        export_reopened,
    }
}

fn decode_ucs4(key: &[u8]) -> String {
    let (words, rest) = key.as_chunks::<4>();
    assert!(rest.is_empty(), "a UCS-4 key");
    words
        .iter()
        .map(|word| char::from_u32(u32::from_le_bytes(*word)).expect("a scalar"))
        .collect()
}

#[derive(Debug, PartialEq)]
pub struct Roundtrip {
    pub absent: (bool, Vec<u32>),
    pub added: Vec<bool>,
    pub found: (bool, Vec<u32>),
    pub added_again: Vec<bool>,
    pub found_again: (bool, Vec<u32>),
    pub saved: bool,
    pub reopened: (bool, Vec<u32>),
    pub removed: bool,
    pub after_remove: (bool, Vec<u32>),
    pub readded: Vec<bool>,
    pub after_readd: (bool, Vec<u32>),
    pub saved_again: bool,
    pub final_: (bool, Vec<u32>),
    pub export: Export,
}

/// Adds, re-adds, saves, removes and re-adds 龘龘 through the phrase table
/// (`phrase_large_table3.cpp:55-87`).
pub fn roundtrip(k: &mut Kit) -> Roundtrip {
    let (text, reading): (&str, &[u8]) = ("龘龘", b"da2'da2");
    let absent = table_lookup(k, text);
    let added = add_user_phrase(k, text, &[reading], 100);
    let found = table_lookup(k, text);
    let added_again = add_user_phrase(k, text, &[reading], 1);
    let found_again = table_lookup(k, text);
    let saved = facade::save(k.ctx());
    reopen(k);
    let reopened = table_lookup(k, text);
    let inst = k.inst();
    assert_eq!(facade::parse_more_full_pinyins(inst, c("dada").as_ptr()), 4);
    assert!(facade::guess_candidates(inst, 0, 0));
    let user_rows: Vec<_> = (0..n_candidate(inst))
        .filter_map(|index| candidate_at(inst, index))
        .filter(|&cand| candidate_type(inst, cand) == 2 && facade::is_user_candidate(inst, cand))
        .collect();
    assert!(!user_rows.is_empty());
    let removed = facade::remove_user_candidate(inst, user_rows[0]);
    let after_remove = table_lookup(k, text);
    let readded = add_user_phrase(k, text, &[reading], 100);
    let after_readd = table_lookup(k, text);
    let saved_again = facade::save(k.ctx());
    reopen(k);
    let final_ = table_lookup(k, text);
    let export = export_phrases(k);
    Roundtrip {
        absent,
        added,
        found,
        added_again,
        found_again,
        saved,
        reopened,
        removed,
        after_remove,
        readded,
        after_readd,
        saved_again,
        final_,
        export,
    }
}

#[derive(Debug, PartialEq)]
pub struct NeverLoaded {
    pub lookup: (bool, Vec<u32>),
    pub prediction: Predicted,
}

/// A user phrase index naming library 15, which never loads
/// (`phrase_index.h:762-772`, `phrase_large_table3.cpp:40-42`).
pub fn never_loaded(k: &mut Kit) -> NeverLoaded {
    let value = ((15_u32 << 24) | 1).to_le_bytes().to_vec();
    let prefix = ucs4_key("你");
    craft_tree(
        &k.user.join("user_phrase_index.bin"),
        &[
            (prefix.clone(), Vec::new()),
            ([prefix, ucs4_key("龘")].concat(), value.clone()),
            (ucs4_key("龘龘"), value),
        ],
    );
    write_user_conf(k);
    let lookup = table_lookup(k, "龘龘");
    NeverLoaded {
        lookup,
        prediction: row58_rows(k, "你"),
    }
}

/// Persists 龘龘, then replaces its index key with 你 and a raw suffix the
/// pin never decodes (0x110000), and predicts after 你
/// (`phrase_large_table3_tkrzwdb.cpp:31-43`, `pinyin.cpp:2385-2397`).
pub fn raw_key_prediction(k: &mut Kit) -> Predicted {
    add_user_phrase(k, "龘龘", &[b"da2'da2"], 100);
    let token = tokens_of(k, "龘龘")
        .into_iter()
        .find(|token| token >> 24 == 7)
        .expect("a library-7 token");
    assert!(facade::save(k.ctx()));
    k.close();
    let prefix = ucs4_key("你");
    craft_tree(
        &k.user.join("user_phrase_index.bin"),
        &[
            (prefix.clone(), Vec::new()),
            (
                [prefix, 0x0011_0000_u32.to_le_bytes().to_vec()].concat(),
                token.to_le_bytes().to_vec(),
            ),
        ],
    );
    write_user_conf(k);
    row58_rows(k, "你")
}

#[derive(Debug, PartialEq)]
pub struct SavedIndex {
    pub added: Vec<Vec<bool>>,
    pub saved: bool,
    pub user_pinyin_index: Vec<(String, String)>,
    pub user_phrase_index: Vec<(String, String)>,
    pub reopened: Vec<(bool, Vec<u32>)>,
    pub export: Export,
}

/// Saves two phrases and reads both user index trees in the backend's format.
pub fn saved_index_format(k: &mut Kit) -> SavedIndex {
    let phrases: [(&str, &[&[u8]]); 2] = [
        ("龘龘", &[b"da2'da2", b"da4'da4"]),
        ("靐靐", &[b"bing4'bing4"]),
    ];
    let added = phrases
        .iter()
        .map(|(text, readings)| add_user_phrase(k, text, readings, 100))
        .collect();
    let saved = facade::save(k.ctx());
    let user_pinyin_index = index_rows(&k.user.join("user_pinyin_index.bin"));
    let user_phrase_index = index_rows(&k.user.join("user_phrase_index.bin"));
    reopen(k);
    let reopened = phrases
        .iter()
        .map(|(text, _)| table_lookup(k, text))
        .collect();
    let export = export_phrases(k);
    SavedIndex {
        added,
        saved,
        user_pinyin_index,
        user_phrase_index,
        reopened,
        export,
    }
}

/// `lookup` is the raw-byte reader; kept for the bodies that pass non-UTF-8 text.
#[allow(dead_code)]
fn raw_lookup(k: &mut Kit, text: &[u8]) -> Vec<u32> {
    lookup(k.inst(), text)
}
