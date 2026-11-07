//! A 256th pronunciation on one item (#804): `add_pronunciation` stores the
//! count through a `guint8` (`phrase_index.cpp:28`, `:88`), so it wraps to 0.

use std::collections::BTreeMap;

use crate::facade;
use crate::readers::{c, garray_u16, tokens_of, unigram_of};
use crate::support::{Kit, P, UNTOUCHED};

const WRAP_PHRASES: [(u8, &str); 7] = [
    (1, "龘"),
    (2, "龖"),
    (3, "龗"),
    (4, "鬱"),
    (5, "鑫"),
    (6, "靐"),
    (7, "麤"),
];

/// The candidate one-syllable pinyin readings, initial × final × tone 1-4.
fn pinyin_readings() -> Vec<Vec<u8>> {
    let initials = [
        "b", "p", "m", "f", "d", "t", "n", "l", "g", "k", "h", "j", "q", "x", "zh", "ch", "sh",
        "r", "z", "c", "s", "y", "w", "",
    ];
    let finals = [
        "a", "o", "e", "ai", "ei", "ao", "ou", "an", "en", "ang", "eng", "i", "u", "ia", "ie",
        "iao", "iu", "ian", "in", "iang", "ing", "ua", "uo", "uai", "ui", "uan", "un", "uang",
        "ong",
    ];
    let mut out = Vec::new();
    for initial in initials {
        for fin in finals {
            for tone in ["1", "2", "3", "4"] {
                out.push(format!("{initial}{fin}{tone}").into_bytes());
            }
        }
    }
    out
}

/// The candidate one-syllable zhuyin readings, initial × final × tone.
fn zhuyin_readings() -> Vec<Vec<u8>> {
    let initials = "ㄅㄆㄇㄈㄉㄊㄋㄌㄍㄎㄏㄐㄑㄒㄓㄔㄕㄖㄗㄘㄙ";
    let finals = [
        "ㄚ", "ㄛ", "ㄜ", "ㄞ", "ㄟ", "ㄠ", "ㄡ", "ㄢ", "ㄣ", "ㄤ", "ㄥ", "ㄧ", "ㄨ", "ㄩ", "ㄧㄚ",
        "ㄧㄝ", "ㄧㄠ", "ㄧㄡ", "ㄧㄢ", "ㄧㄣ", "ㄧㄤ", "ㄧㄥ", "ㄨㄚ", "ㄨㄛ", "ㄨㄞ", "ㄨㄟ",
        "ㄨㄢ", "ㄨㄣ", "ㄨㄤ", "ㄨㄥ",
    ];
    let mut out = Vec::new();
    for initial in initials.chars() {
        for fin in finals {
            for tone in ["ˊ", "ˇ", "ˋ", ""] {
                out.push(format!("{initial}{fin}{tone}").into_bytes());
            }
        }
    }
    out
}

/// `_wrap_distinct`: the candidates the facade accepts, found by adding each to
/// a fresh one-character phrase on a scratch profile.
fn distinct(k: &mut Kit, candidates: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    let scratch = k.scratch.join("wrap-X");
    std::fs::create_dir(&scratch).expect("the scratch profile");
    let ctx = k.init(Some(&k.data.clone()), Some(&scratch));
    assert!(!ctx.is_null(), "scratch context");
    let it = facade::begin_add_phrases(ctx, 7);
    let kept: Vec<Vec<u8>> = candidates
        .into_iter()
        .enumerate()
        .filter(|(number, reading)| {
            let text = char::from_u32(0x4E00 + u32::try_from(*number).expect("number"))
                .expect("a character")
                .to_string();
            facade::iterator_add_phrase(it, c(text).as_ptr(), c(reading).as_ptr(), 1)
        })
        .map(|(_, reading)| reading)
        .collect();
    facade::end_add_phrases(it);
    facade::fini(ctx);
    assert!(kept.len() >= 258, "{}", kept.len());
    kept
}

/// One nth-pronunciation read: the answer, the keys' count and, below the
/// counted readings, their bytes in hex.
pub type Nth = (bool, u32, Option<String>);

/// The export summary: the row count, the first and last rows and the SHA-256
/// of the rows as the old reader serialised them.
pub type ExportSummary = (usize, Vec<(String, i32)>, Vec<(String, i32)>, String);

#[derive(Debug, PartialEq)]
pub struct ItemRead {
    pub token: String,
    pub n: (bool, u32),
    /// Left out for the addon and network libraries, whose unigram the shipped
    /// addon table answers.
    pub unigram: Option<(bool, u32)>,
    pub nth: BTreeMap<String, Nth>,
    pub export: Option<ExportSummary>,
}

/// One entry of the case's record.
#[derive(Debug, PartialEq)]
pub enum Entry {
    Item(Option<ItemRead>),
    Count(bool, u32),
    Flag(bool),
    Note(String),
}

/// `json.dumps(rows, ensure_ascii=False)` of `[reading, count]` rows.
fn python_json(rows: &[(String, i32)]) -> String {
    let rows: Vec<String> = rows
        .iter()
        .map(|(reading, count)| format!("[\"{reading}\", {count}]"))
        .collect();
    format!("[{}]", rows.join(", "))
}

fn sha256_hex(text: &str) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The export rows of `text` in `library`: `[reading, count]` in export order.
fn export_rows(k: &mut Kit, library: u8, text: &str) -> Vec<(String, i32)> {
    let it = facade::begin_get_phrases(k.ctx(), u32::from(library));
    let mut listed = Vec::new();
    while facade::iterator_has_next_phrase(it) {
        let (mut phrase, mut reading, mut n) = (std::ptr::null_mut(), std::ptr::null_mut(), 0_i32);
        facade::iterator_get_next_phrase(it, &raw mut phrase, &raw mut reading, &raw mut n);
        let (phrase, reading) = (
            crate::support::take_text(phrase),
            crate::support::take_text(reading),
        );
        if phrase.as_deref() == Some(text) {
            listed.push((reading.unwrap_or_default(), n));
        }
    }
    facade::end_get_phrases(it);
    listed
}

/// `_wrap_item_reads`: the item `text` in `library` as the C API shows it.
fn item_reads(k: &mut Kit, library: u8, text: &str) -> Entry {
    let Some(token) = tokens_of(k, text)
        .into_iter()
        .find(|token| token >> 24 == u32::from(library))
    else {
        return Entry::Item(None);
    };
    let inst = k.inst();
    let mut count = UNTOUCHED;
    let ret = facade::token_get_n_pronunciation(inst, token, &raw mut count);
    let unigram = unigram_of(k, token);
    let mut nth = BTreeMap::new();
    for index in [0_u32, 1, 254, 255] {
        // SAFETY: a fresh GArray of two-byte keys, left allocated as the case did.
        let keys: P = unsafe { glib_sys::g_array_new(0, 0, 2) }.cast();
        let answer = facade::token_get_nth_pronunciation(inst, token, index, keys);
        let words = garray_u16(keys.cast());
        let len = u32::try_from(words.len()).expect("keys");
        let content = (index < count).then(|| {
            words
                .iter()
                .flat_map(|word| word.to_le_bytes())
                .map(|byte| format!("{byte:02x}"))
                .collect()
        });
        nth.insert(format!("nth {index}"), (answer, len, content));
    }
    let export = (!facade::ZHUYIN).then(|| {
        let listed = export_rows(k, library, text);
        (
            listed.len(),
            listed.first().cloned().into_iter().collect(),
            listed.last().cloned().into_iter().collect(),
            sha256_hex(&python_json(&listed)),
        )
    });
    Entry::Item(Some(ItemRead {
        token: format!("{token:#x}"),
        n: (ret, count),
        unigram: (![5, 6].contains(&library)).then_some(unigram),
        nth,
        export,
    }))
}

/// Fills libraries 1-7 to 255 readings, adds the 256th, reads the item live and
/// after `end_add_phrases`, adds the 257th in a new iterator, saves, reopens and
/// reads every library again.
pub fn wrap_256th(k: &mut Kit) -> BTreeMap<String, Entry> {
    let candidates = if facade::ZHUYIN {
        zhuyin_readings()
    } else {
        pinyin_readings()
    };
    let readings = distinct(k, candidates);
    let mut out = BTreeMap::new();
    for (library, text) in WRAP_PHRASES {
        let label = format!("lib {library}");
        let mut pending = readings.iter();
        let iterator = facade::begin_add_phrases(k.ctx(), library);
        if iterator.is_null() {
            out.insert(label, Entry::Note("no iterator".to_owned()));
            continue;
        }
        let mut step = |k: &mut Kit, iterator: P, frequency: i32| {
            let reading = pending.next().expect("enough readings");
            let ret = facade::iterator_add_phrase(
                iterator,
                c(text).as_ptr(),
                c(reading).as_ptr(),
                frequency,
            );
            let token = tokens_of(k, text)
                .into_iter()
                .find(|token| token >> 24 == u32::from(library));
            let mut count = UNTOUCHED;
            if let Some(token) = token {
                facade::token_get_n_pronunciation(k.inst(), token, &raw mut count);
            }
            (ret, if token.is_some() { count } else { 0 })
        };
        let (ret, mut count) = step(k, iterator, 11);
        if !ret {
            out.insert(label, Entry::Note("first add refused".to_owned()));
            facade::end_add_phrases(iterator);
            continue;
        }
        while count < 255 {
            count = step(k, iterator, 1).1;
        }
        out.insert(format!("{label} at 255"), item_reads(k, library, text));
        let ret = loop {
            let before = count;
            let (ret, now) = step(k, iterator, 1);
            count = now;
            if count != before || !ret {
                break ret;
            }
        };
        out.insert(format!("{label} 256th"), Entry::Count(ret, count));
        out.insert(
            format!("{label} after 256th live"),
            item_reads(k, library, text),
        );
        facade::end_add_phrases(iterator);
        out.insert(
            format!("{label} after end_add"),
            item_reads(k, library, text),
        );
        let iterator = facade::begin_add_phrases(k.ctx(), library);
        out.insert(
            format!("{label} 257th add"),
            Entry::Flag(step(k, iterator, 9).0),
        );
        out.insert(
            format!("{label} after 257th live"),
            item_reads(k, library, text),
        );
        facade::end_add_phrases(iterator);
        out.insert(
            format!("{label} after 257th end_add"),
            item_reads(k, library, text),
        );
    }
    out.insert("save".to_owned(), Entry::Flag(facade::save(k.ctx())));
    facade::fini(k.ctx());
    k.forget();
    for (library, text) in WRAP_PHRASES {
        out.insert(
            format!("lib {library} after reload"),
            item_reads(k, library, text),
        );
    }
    out
}
