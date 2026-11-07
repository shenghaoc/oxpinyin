//! Training, imports and token updates around an unloaded library.

use std::ops::Bound;
use std::path::Path;

use oxpinyin_store::{DefaultStore, RawReadStore as _};

use super::{chunk, hex, journal, le, le16, modify_header, word, write_log};
use crate::facade;
use crate::fixtures::{craft_hash, craft_tree, gram_bytes, items_system};
use crate::readers::{
    c, candidate_at, candidate_text, candidate_type, n_candidate, reopen, unigram_of,
};
use crate::support::{J, Kit, P};

/// The two libraries each owning one of the fixture's two slots: library 1 keeps
/// slot 1, the GBK library slot 2, each at `total`; the bodies stay tiny.
fn split_libraries(k: &Kit, gbk_total: u32) {
    let original = std::fs::read(k.data.join("gb_char.bin")).expect("gb_char.bin")[8..].to_vec();
    for (name, absent, total) in [("gb_char.bin", 2, 100), ("gbk_char.bin", 1, gbk_total)] {
        let mut payload = original.clone();
        payload[..4].copy_from_slice(&le(total));
        payload[17 + absent * 4..21 + absent * 4].fill(0);
        let path = k.data.join(name);
        std::fs::remove_file(&path).expect("unlink a library");
        std::fs::write(&path, chunk(&payload)).expect("write a library");
    }
}

/// Appends `line` to the private table.conf.
fn append_conf(k: &Kit, line: &str) {
    let path = k.data.join("table.conf");
    let text = std::fs::read_to_string(&path).expect("table.conf");
    std::fs::write(&path, text + line).expect("table.conf");
}

/// The GBK row the fixtures add back after `items_system` isolated library 1.
const GBK_ROW: &str =
    "default GBK_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin SYSTEM_FILE\n";

/// The facade's parse call, asserted to consume all of `text`.
fn parse_all(inst: P, text: &str) {
    let parsed = if facade::ZHUYIN {
        facade::parse_more_chewings(inst, c(text).as_ptr())
    } else {
        facade::parse_more_full_pinyins(inst, c(text).as_ptr())
    };
    assert_eq!(parsed, text.len());
}

/// The facade's training of n-best result 0.
fn train(inst: P) -> bool {
    facade::train_top(inst)
}

/// `unloaded_cached_pronunciation`: two one-reading items expose
/// `train_result3`'s stale cached `PhraseItem`. Library two is chosen, unloaded,
/// then a second instance guesses so the context's lookup caches library one's
/// item, and the first instance's still-valid result trains. Pin
/// `phonetic_lookup.h:924` ignores the failed fetch; `:925-927` can consequently
/// train the cached, still-loaded item instead. The save exposes every fixture
/// item's pronunciation count on both facades.
pub(super) fn cached_pronunciation(k: &mut Kit) -> J {
    items_system(
        k,
        &[
            (&[u32::from('甲')], &[0x1081], 100),
            (&[u32::from('乙')], &[0x1081], 100),
        ],
        200,
        true,
    );
    split_libraries(k, 100);
    append_conf(k, GBK_ROW);
    let tokens = [0x0100_0001_u32, 0x0200_0002];
    let phonetic: Vec<u8> = tokens
        .iter()
        .flat_map(|token| [&le(*token)[..], &le16(0x1081), &[0, 0]].concat())
        .collect();
    craft_tree(
        &k.data.join("pinyin_index.bin"),
        &[(b"\x81\x00".to_vec(), phonetic)],
    );
    let phrases: Vec<(Vec<u8>, Vec<u8>)> = ['甲', '乙']
        .iter()
        .zip(tokens)
        .map(|(word, token)| (le(u32::from(*word)).to_vec(), le(token).to_vec()))
        .collect();
    craft_tree(&k.data.join("phrase_index.bin"), &phrases);
    let inst = k.inst();
    let text = if facade::ZHUYIN { "18 " } else { "ba1" };
    parse_all(inst, text);
    if facade::ZHUYIN {
        assert!(facade::guess_candidates_after_cursor(inst, 0));
    } else {
        assert!(facade::guess_candidates(inst, 0, 0x1e));
    }
    let chosen = candidate(inst, "乙", 2);
    assert!(facade::choose_candidate(inst, 0, chosen) > 0);
    assert!(facade::guess_sentence(inst));
    let mut out = obj! {
        "unigrams-before" => tokens.map(|token| unigram_of(k, token)).to_vec(),
        "pronunciations-before" => vec![10, 10],
    };
    out.set("unload", facade::unload_phrase_library(k.ctx(), 2));
    let other = k.alloc();
    parse_all(other, text);
    assert!(facade::guess_sentence(other));
    out.set("train", train(inst));
    out.set(
        "unigrams-after",
        tokens.map(|token| unigram_of(k, token)).to_vec(),
    );
    out.set("save", facade::save(k.ctx()));
    // Every MODIFY payload, including the loaded item's pronunciation. The
    // unloaded library must have no journal written by this save.
    out.set("journals", journals(k, &["gb_char.dbin", "gbk_char.dbin"]));
    facade::free_instance(other);
    out
}

/// The named user files' bytes as hex, `None` where absent.
fn journals(k: &Kit, names: &[&str]) -> J {
    let mut out = J::map();
    for name in names {
        out.set(*name, journal(k, name));
    }
    out
}

/// `unloaded_candidate`: the current list's first candidate of `wanted` and
/// `kind`; the getters must answer.
fn candidate(inst: P, wanted: &str, kind: i32) -> P {
    let mut rows = Vec::new();
    for index in 0..n_candidate(inst) {
        let cand = candidate_at(inst, index).expect("get_candidate");
        let text = candidate_text(inst, cand).expect("get_candidate_string");
        let actual = candidate_type(inst, cand);
        if text == wanted && actual == kind {
            return cand;
        }
        rows.push((text, actual));
    }
    panic!("no {wanted} of type {kind}: {rows:?}");
}

/// One insertion-ordered dict of the fixture's index rows (Python's `dict`).
#[derive(Default)]
struct Rows(Vec<(Vec<u8>, Vec<u8>)>);

impl Rows {
    fn set_default(&mut self, key: &[u8]) {
        if !self.0.iter().any(|(k, _)| k == key) {
            self.0.push((key.to_vec(), Vec::new()));
        }
    }

    fn set(&mut self, key: &[u8], value: Vec<u8>) {
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some(row) => row.1 = value,
            None => self.0.push((key.to_vec(), value)),
        }
    }
}

/// `unloaded_fixture`: two distinct readings keep ordinary unloaded training
/// free of stale writes. Library one has 甲/da1; the target library (2, or the
/// user library 7) has 乙/ba1, or 甲乙/ba1ba1 for `longer` (prefix selection).
/// Only these two items and empty bigrams load. Answers the two tokens and the
/// target text.
fn fixture(k: &mut Kit, longer: bool, library: u32, overflow: bool) -> ([u32; 2], &'static str) {
    let target = if longer { "甲乙" } else { "乙" };
    let target_keys: &[u16] = if longer { &[0x1081, 0x1081] } else { &[0x1081] };
    let texts = ["甲", target];
    let readings: [&[u16]; 2] = [&[0x1084], target_keys];
    let words: Vec<Vec<u32>> = texts
        .iter()
        .map(|text| text.chars().map(u32::from).collect())
        .collect();
    items_system(
        k,
        &[(&words[0], readings[0], 100), (&words[1], readings[1], 100)],
        200,
        true,
    );
    split_libraries(k, if overflow { 0xffff_fffe } else { 100 });
    if library == 7 {
        append_conf(k, "default USER_DICTIONARY NULL NULL user.bin USER_FILE\n");
        std::fs::copy(k.data.join("gbk_char.bin"), k.user.join("user.bin")).expect("user.bin");
    } else {
        append_conf(k, GBK_ROW);
    }
    let tokens = [0x0100_0001, (library << 24) | 2];
    let (mut phonetic, mut phrases) = (Rows::default(), Rows::default());
    for ((text, keys), token) in texts.iter().zip(readings).zip(tokens) {
        let raw_keys: Vec<u8> = keys.iter().flat_map(|key| key.to_le_bytes()).collect();
        let index: Vec<u8> = keys
            .iter()
            .flat_map(|key| (key & 0xfff).to_le_bytes())
            .collect();
        let raw_text: Vec<u8> = text.chars().flat_map(|ch| le(u32::from(ch))).collect();
        for end in (2..index.len()).step_by(2) {
            phonetic.set_default(&index[..end]);
        }
        let mut item = [&le(token)[..], &raw_keys].concat();
        item.resize(item.len().div_ceil(4) * 4, 0);
        phonetic.set(&index, item);
        for end in (4..raw_text.len()).step_by(4) {
            phrases.set_default(&raw_text[..end]);
        }
        phrases.set(&raw_text, le(token).to_vec());
    }
    for (name, rows) in [
        ("pinyin_index.bin", phonetic),
        ("phrase_index.bin", phrases),
    ] {
        if library == 7 {
            // User-file tokens belong to the mutable user indexes, as import
            // writes them, rather than a synthetic shipped-system row.
            let (loaded, rest) = rows.0.split_first().expect("a loaded row");
            craft_tree(&k.data.join(name), std::slice::from_ref(loaded));
            craft_tree(&k.user.join(format!("user_{name}")), rest);
        } else {
            craft_tree(&k.data.join(name), &rows.0);
        }
    }
    (tokens, target)
}

/// `unloaded_parse`: ba1, ba1da1 (`mixed`) or da1 (`loaded`) on the facade's
/// standard keyboard.
fn unloaded_parse(inst: P, mixed: bool, loaded: bool) {
    let text = match (facade::ZHUYIN, loaded, mixed) {
        (false, true, _) => "da1",
        (false, false, true) => "ba1da1",
        (false, false, false) => "ba1",
        (true, true, _) => "28 ",
        (true, false, true) => "18 28 ",
        (true, false, false) => "18 ",
    };
    parse_all(inst, text);
}

/// `_system_items` of gbk_char.bin: its total and slot 2's item bytes.
fn gbk_slot_two(k: &Kit) -> (u32, Vec<u8>) {
    let bytes = std::fs::read(k.data.join("gbk_char.bin")).expect("gbk_char.bin");
    let payload = &bytes[8..];
    let (total, index, content) = (word(payload, 0), word(payload, 4), word(payload, 8));
    let at = |value: u32| usize::try_from(value).expect("an offset");
    let offset = word(payload, at(index + 2 * 4));
    assert!(offset != 0, "slot 2 holds an item");
    let start = at(content + offset);
    let (length, prons) = (usize::from(payload[start]), usize::from(payload[start + 1]));
    let size = 6 + 4 * length + prons * (2 * length + 4);
    (total, payload[start..start + size].to_vec())
}

/// `_log_record` of a MODIFY (3) or REMOVE (2) of `token`.
fn log_record(kind: u32, token: u32, old: &[u8], new: &[u8]) -> Vec<u8> {
    let head = [&le(kind)[..], &le(token)].concat();
    if kind == 2 {
        return [&head[..], &le16(old.len()), old].concat();
    }
    [&head[..], &le16(old.len()), &le16(new.len()), old, new].concat()
}

/// `index_file_rows(path, kind='hash', allow_empty=True)`: a saved user bigram's
/// rows as sorted hex pairs, `[]` when the file is absent.
fn bigram_rows(path: &Path) -> J {
    if !path.exists() {
        return J::L(Vec::new());
    }
    let store = DefaultStore::open_user_bigram(path).expect("the saved bigram opens");
    let mut rows = Vec::new();
    store
        .range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
            rows.push((hex(key), hex(value)));
            Ok(())
        })
        .expect("read the saved bigram");
    rows.sort();
    J::from(rows)
}

/// `unloaded_operation`: unload mutations and controls; the save observes every
/// fixture item. Ordinary training caches 甲/da1 before training 乙/ba1, so the
/// pin's stale pronunciation call matches no reading. Raw journal payloads
/// cover all pronunciation fields, including loaded items, on both facades.
pub(super) fn operation(k: &mut Kit, variant: &str) -> J {
    let library = if variant == "train-user" { 7 } else { 2 };
    let longer = variant.starts_with("longer") || variant.starts_with("predicted-prefix");
    let overflow = variant.ends_with("overflow");
    let (tokens, target) = fixture(k, longer, library, overflow);
    let index = u8::try_from(library).expect("a library index");
    let (total, item) = gbk_slot_two(k);
    if variant == "train-journal" {
        let mut changed = item.clone();
        let end = changed.len();
        changed[end - 4..].copy_from_slice(&le(17));
        write_log(
            k,
            &[
                modify_header(0, total, total),
                log_record(3, 2, &item, &changed),
            ]
            .concat(),
            "gbk_char.dbin",
        );
    }
    let predicted = variant.starts_with("predicted");
    if variant.starts_with("predicted-bigram") {
        let predecessor = if variant.ends_with("predecessor") {
            tokens[1]
        } else {
            tokens[0]
        };
        craft_hash(
            &k.user.join("user_bigram.db"),
            &[(
                le(predecessor).to_vec(),
                gram_bytes(100, &[(tokens[1], 100)]),
            )],
        );
    }
    let inst = k.inst();
    let unigrams = |k: &mut Kit| tokens.map(|token| unigram_of(k, token)).to_vec();
    let mut out = obj! { "tokens" => tokens.to_vec(), "before" => unigrams(k) };
    let mut chosen = std::ptr::null_mut();
    let mut iterator = std::ptr::null_mut();
    let trains = variant.starts_with("train");
    let choosing = variant.starts_with("longer") || variant.starts_with("without");
    if trains || choosing {
        unloaded_parse(inst, variant == "train-mixed", false);
        if facade::ZHUYIN {
            assert!(facade::guess_candidates_after_cursor(inst, 0));
        } else {
            let sort = if longer {
                0x1c
            } else if variant.starts_with("without") {
                0x1f
            } else {
                0x1e
            };
            assert!(facade::guess_candidates(inst, 0, sort));
        }
        chosen = candidate(inst, target, if longer { 7 } else { 2 });
        if trains {
            assert!(facade::choose_candidate(inst, 0, chosen) > 0);
            assert!(facade::guess_sentence(inst));
        }
    } else if predicted {
        let prefix = if variant.ends_with("predecessor") {
            "乙"
        } else {
            "甲"
        };
        assert!(facade::guess_predicted_candidates(inst, c(prefix).as_ptr()));
        chosen = candidate(
            inst,
            if longer { "乙" } else { target },
            if longer { 5 } else { 4 },
        );
    } else if variant == "import" {
        iterator = facade::begin_add_phrases(k.ctx(), index);
        assert!(!iterator.is_null());
    }
    if variant == "train-before" {
        out.set("earlier-train", train(inst));
    }
    if !overflow {
        out.set("unload", facade::unload_phrase_library(k.ctx(), index));
        out.set(
            "repeat-unload",
            facade::unload_phrase_library(k.ctx(), index),
        );
    }
    let missing = variant.ends_with("missing");
    if missing {
        write_log(
            k,
            &[modify_header(0, total, 0), log_record(2, 2, &item, &[])].concat(),
            "gbk_char.dbin",
        );
        out.set(
            "reload-missing",
            facade::load_phrase_library(k.ctx(), index),
        );
    }
    if trains {
        // Leave a live, different-reading cache. No undefined pin read.
        let other = k.alloc();
        unloaded_parse(other, false, true);
        assert!(facade::guess_sentence(other));
        let first = train(inst);
        out.set("train", (first, train(inst)));
        facade::free_instance(other);
    } else if predicted {
        out.set("choose", facade::choose_predicted_candidate(inst, chosen));
    } else if choosing {
        out.set("choose", facade::choose_candidate(inst, 0, chosen));
    } else if variant == "import" {
        let reading = if facade::ZHUYIN { "ㄅㄚ" } else { "ba1" };
        out.set(
            "add",
            facade::iterator_add_phrase(iterator, c("乙").as_ptr(), c(reading).as_ptr(), 7),
        );
        facade::end_add_phrases(iterator);
    } else {
        out.set(
            "add",
            facade::token_add_unigram_frequency(inst, tokens[1], 7),
        );
    }
    out.set("after", unigrams(k));
    out.set("save", facade::save(k.ctx()));
    if predicted {
        // Prediction does not mark the context modified. A separate loaded
        // training operation then makes the in-memory predicted bigram visible
        // on disk, without rewriting it or reusing the prediction snapshot.
        let flush = k.alloc();
        unloaded_parse(flush, false, true);
        assert!(facade::guess_candidates(flush, 0, 0x1e));
        let row = candidate(flush, "甲", 2);
        assert!(facade::choose_candidate(flush, 0, row) > 0);
        assert!(facade::guess_sentence(flush));
        assert!(train(flush));
        out.set("flush-save", facade::save(k.ctx()));
        facade::free_instance(flush);
    }
    out.set(
        "journals",
        journals(k, &["gb_char.dbin", "gbk_char.dbin", "user.bin"]),
    );
    let gram = k.user.join("user_bigram.db");
    out.set("bigram", bigram_rows(&gram));
    if !missing && !overflow {
        out.set("reload", facade::load_phrase_library(k.ctx(), index));
        out.set("restored", unigrams(k));
        out.set(
            "loaded-add",
            facade::token_add_unigram_frequency(inst, tokens[1], 7),
        );
        out.set("loaded-after", unigram_of(k, tokens[1]));
    }
    reopen(k);
    out.set("reopened", unigrams(k));
    out.set("reopened-bigram", bigram_rows(&gram));
    out
}
