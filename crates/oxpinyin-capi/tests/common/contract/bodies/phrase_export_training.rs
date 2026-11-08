//! The phrase_export_training family's case bodies: each answers the result document the old case returned.
//!
//! Phrase export and training (inventory row 51, #759, #790, #1031): the export
//! cursor across imports, the live item a training changes, appended system
//! readings and the user library's saved bytes.

use std::ffi::c_char;

use crate::facade;
use crate::fixtures::{
    chunk, craft_hash, gram_bytes, hex_bytes, modify_header, replace_user_bigram, write_user_conf,
};
use crate::readers::{
    c, candidate_at, candidate_text, candidate_type, final_step_sentences, guess_out, import_rows,
    key_out, n_candidate, remember_export, tokens_of, unigram_of,
};
use crate::support::{J, Kit, P, UNTOUCHED, take_text};

/// `_PAST_RANGES`: a token past every library's range.
const PAST_RANGES: u32 = 0x0fff_ffff;

/// The case body for `id`.
pub fn case(k: &mut Kit, id: &str) -> J {
    if let Some(action) = id
        .strip_prefix("issue790-export-")
        .and_then(|rest| rest.strip_suffix("-system"))
    {
        match action {
            "after-train" => return issue790_export(k, false),
            "open-before" => return issue790_export(k, true),
            _ => {}
        }
    }
    match id {
        "issue790-export-zero-and-signed-system" => issue790_bounded_counts(k, (1 << 31) - 69),
        "issue790-export-prefix-overflow-system" => {
            issue790_bounded_counts(k, u32::try_from((1_u64 << 32) - 76).expect("a u32"))
        }
        "appended-system-second-reading-pinyin" | "appended-system-second-reading-zhuyin" => {
            appended_system_second_reading(k)
        }
        "issue790-guess-after-train-system" => guess_after_train(k),
        "issue790-character-offset-after-train-system" => character_offset_after_train(k),
        "issue759-export-open-across-import-system" => export_open_across_import_system(k),
        "issue759-export-open-across-import-user" => export_open_across_import_user(k),
        "user-library-training-bytes-pinyin" | "user-library-training-bytes-zhuyin" => {
            user_library_training_bytes(k)
        }
        other => panic!("no phrase_export_training case {other}"),
    }
}

/// `export_rows`: up to `limit` export rows of `it` (every row for `None`),
/// `has_next` asked before the limit; the iterator stays open.
fn export_rows(it: P, limit: Option<usize>) -> Vec<J> {
    let mut rows = Vec::new();
    while facade::iterator_has_next_phrase(it) && limit.is_none_or(|limit| rows.len() < limit) {
        let (mut phrase, mut reading): (*mut c_char, *mut c_char) =
            (std::ptr::null_mut(), std::ptr::null_mut());
        let mut count = i32::from_ne_bytes(UNTOUCHED.to_ne_bytes());
        let ret =
            facade::iterator_get_next_phrase(it, &raw mut phrase, &raw mut reading, &raw mut count);
        rows.push(J::from((ret, take_text(phrase), take_text(reading), count)));
    }
    rows
}

/// `remember_export(k, library, phrase)` as the old document held it.
fn exported(k: &mut Kit, library: u32, phrase: &str) -> J {
    J::from(remember_export(k, library, phrase))
}

/// `train_selected_top`: chooses the NORMAL candidate `wanted` on the cached
/// instance, guesses and trains n-best result 0; with `guess_before_choose`,
/// first guesses and keeps the final-step sentences as `before-choose`.
fn train_selected_top(k: &mut Kit, wanted: &str, token: u32, guess_before_choose: bool) -> J {
    let inst = k.inst();
    if facade::ZHUYIN {
        assert!(facade::guess_candidates_after_cursor(inst, 0));
    } else {
        // SORT_WITHOUT_SENTENCE would train during choose (pinyin.cpp:2566-2576).
        assert!(facade::guess_candidates(inst, 0, 0x1e));
    }
    for index in 0..n_candidate(inst) {
        let cand = candidate_at(inst, index).expect("candidate");
        let text = candidate_text(inst, cand).expect("candidate text");
        let kind = candidate_type(inst, cand);
        if kind != 2 || text != wanted {
            continue;
        }
        let mut out = J::map();
        if guess_before_choose {
            assert!(facade::guess_sentence(inst));
            out.set("before-choose", final_step_sentences(inst));
        }
        let cursor = facade::choose_candidate(inst, 0, cand);
        assert!(cursor > 0, "normal phrase selection failed");
        out.set("after-choose", unigram_of(k, token));
        let guessed = facade::guess_sentence(inst);
        assert!(guessed, "training needs a guessed n-best result");
        let trained = facade::train_top(inst);
        assert!(trained, "training n-best result 0 failed");
        out.set("chosen", wanted);
        out.set("kind", kind);
        out.set("cursor", cursor);
        out.set("guess", guessed);
        out.set("train", trained);
        return out;
    }
    panic!("missing normal candidate: {wanted}");
}

/// Parses full pinyin `text` into the cached instance, answering the bytes consumed.
fn parse(k: &mut Kit, text: &str) -> usize {
    facade::parse_more_full_pinyins(k.inst(), c(text).as_ptr())
}

/// Frees the cached instance, finalises the cached context and forgets both.
fn close(k: &mut Kit) {
    let inst = k.inst();
    facade::free_instance(inst);
    facade::fini(k.ctx());
    k.forget();
}

/// `issue790_export`: 你's export around a selected training (or none) and the
/// persistence `action` names.
fn issue790_export(k: &mut Kit, open_before: bool) -> J {
    let phrase = "你";
    let token = tokens_of(k, phrase)[0];
    let mut out = obj! { "before" => exported(k, 1, phrase) };
    let iterator = open_before.then(|| facade::begin_get_phrases(k.ctx(), 1));
    out.set("parsed", parse(k, "ni"));
    out.set("training", train_selected_top(k, phrase, token, true));
    if let Some(iterator) = iterator {
        let rows: Vec<J> = export_rows(iterator, None)
            .into_iter()
            .filter(|row| row.items()[1] == J::from(phrase))
            .collect();
        out.set("after", rows);
        facade::end_get_phrases(iterator);
    } else {
        out.set("after", exported(k, 1, phrase));
    }
    out
}
fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("a word"))
}

/// `issue790_readings`: closes the kit and replays a system-item MODIFY of
/// `phrase` (library 1, one character) whose readings become `readings` and
/// whose unigram becomes `unigram` (kept when `None`); answers the token.
fn issue790_readings(
    k: &mut Kit,
    phrase: &str,
    readings: &[(&str, u32)],
    unigram: Option<u32>,
) -> u32 {
    let token = tokens_of(k, phrase)[0];
    assert_eq!(token >> 24, 1);
    let packed: Vec<(u16, u32)> = readings
        .iter()
        .map(|(text, count)| (key_out(k, text.as_bytes()).1, *count))
        .collect();
    let file = std::fs::read(k.data.join("gb_char.bin")).expect("gb_char.bin");
    let payload = &file[8..];
    let (total, index, content) = (word(payload, 0), word(payload, 4), word(payload, 8));
    let slot = usize::try_from(index + (token & 0x00FF_FFFF) * 4).expect("an offset");
    let offset = usize::try_from(content + word(payload, slot)).expect("an offset");
    let (length, nprons) = (payload[offset], usize::from(payload[offset + 1]));
    assert_eq!(length, 1);
    let old = &payload[offset..offset + 10 + nprons * 6];
    let previous = word(old, 2);
    let unigram = unigram.unwrap_or(previous);
    let mut new = vec![1, u8::try_from(packed.len()).expect("a count")];
    new.extend(unigram.to_le_bytes());
    new.extend(&old[6..10]);
    for (key, count) in &packed {
        new.extend(key.to_le_bytes());
        new.extend(count.to_le_bytes());
    }
    let mut record = 3_u32.to_le_bytes().to_vec();
    record.extend((token & 0x00FF_FFFF).to_le_bytes());
    record.extend(u16::try_from(old.len()).expect("a length").to_le_bytes());
    record.extend(u16::try_from(new.len()).expect("a length").to_le_bytes());
    record.extend(old);
    record.extend(&new);
    close(k);
    write_user_conf(k);
    let header = modify_header(0, total, total.wrapping_add(unigram).wrapping_sub(previous));
    std::fs::write(
        k.user.join("gb_char.dbin"),
        chunk(&[header, record].concat()),
    )
    .expect("write the journal");
    token
}

/// `issue790_bounded_counts`: 你 with ni3 at `count` and two zero-count readings,
/// its export around a selected training.
fn issue790_bounded_counts(k: &mut Kit, count: u32) -> J {
    let token = issue790_readings(k, "你", &[("ni3", count), ("ni2", 0), ("na4", 0)], None);
    let mut out = obj! { "before" => exported(k, 1, "你") };
    assert!(parse(k, "ni") > 0);
    out.set("training", train_selected_top(k, "你", token, true));
    out.set("after", exported(k, 1, "你"));
    out
}

/// The cached instance's `token_get_n_pronunciation` of `token`.
fn pronunciations(k: &mut Kit, token: u32) -> (bool, u32) {
    let mut value = UNTOUCHED;
    let ok = facade::token_get_n_pronunciation(k.inst(), token, &raw mut value);
    (ok, value)
}

/// `appended_system_second_reading`: two readings imported for one new system
/// token, then saved and reopened.
fn appended_system_second_reading(k: &mut Kit) -> J {
    let phrase = "龘龘";
    let readings = if facade::ZHUYIN {
        ["ㄉㄚˇ ㄉㄚˇ", "ㄉㄚˊ ㄉㄚˊ"]
    } else {
        ["da3'da3", "da2'da2"]
    };
    let first = import_rows(k, 1, &[(phrase, readings[0], 5)]);
    assert_eq!(first, [true]);
    let tokens: Vec<u32> = tokens_of(k, phrase)
        .into_iter()
        .filter(|token| token >> 24 == 1)
        .collect();
    assert_eq!(tokens.len(), 1);
    let token = tokens[0];
    let after_first = pronunciations(k, token);
    assert_eq!(after_first, (true, 1));
    let second = import_rows(k, 1, &[(phrase, readings[1], 7)]);
    assert_eq!(second, [true]);
    let after_second = pronunciations(k, token);
    assert_eq!(after_second, (true, 2));
    let saved = facade::save(k.ctx());
    assert!(saved);
    close(k);
    let reopened = pronunciations(k, token);
    assert_eq!(reopened, (true, 2));
    obj! {
        "first" => first,
        "tokens" => tokens,
        "after-first" => after_first,
        "second" => second,
        "after-second" => after_second,
        "saved" => saved,
        "reopened" => reopened,
    }
}

/// `_replace_system_start_row`: closes the kit and opens it next on a copy of
/// the system tables whose bigram holds `value` under the sentence start.
fn replace_system_start_row(k: &mut Kit, value: Vec<u8>) {
    close(k);
    let copied = k.scratch.join("sys-X");
    std::fs::create_dir(&copied).expect("the system copy");
    for entry in std::fs::read_dir(&k.data).expect("system data") {
        let entry = entry.expect("system entry");
        crate::fixtures::writable_copy(&entry.path(), &copied.join(entry.file_name()));
    }
    k.data = copied;
    craft_hash(&k.data.join("bigram.db"), &[(vec![1, 0, 0, 0], value)]);
    write_user_conf(k);
}

/// `issue790-guess-after-train-system`: ni guesses before and after training a
/// low-mass reading of 尼 over rewritten start rows.
fn guess_after_train(k: &mut Kit) -> J {
    let token = issue790_readings(k, "尼", &[("ni2", 1), ("na2", 99)], Some(200_000));
    replace_system_start_row(k, gram_bytes(1, &[(PAST_RANGES, 1)]));
    replace_user_bigram(k, &[(1, gram_bytes(1 << 28, &[(PAST_RANGES, 1 << 28)]))]);
    let mut out = obj! { "parsed" => parse(k, "ni") };
    let mut training = train_selected_top(k, "尼", token, true);
    let J::M(map) = &mut training else {
        unreachable!("an object")
    };
    let before = map.remove("before-choose").expect("before-choose");
    out.set("training", training);
    out.set("before", before);
    out.set("clear", facade::clear_constraint(k.inst(), 0));
    out.set("after", guess_out(k, b"ni"));
    out
}

/// The cached instance's parse of `na` and `get_character_offset` of 哪 at byte one.
fn character_offset(k: &mut Kit) -> J {
    assert!(parse(k, "na") > 0);
    let mut value = UNTOUCHED as usize;
    let ret = facade::get_character_offset(k.inst(), c("哪").as_ptr(), 1, &raw mut value);
    J::from((ret, value))
}

/// `issue790-character-offset-after-train-system`: 哪's character offset before
/// and after training its initially zero-count na reading.
fn character_offset_after_train(k: &mut Kit) -> J {
    let token = issue790_readings(k, "哪", &[("na3", 0), ("nei3", 100)], None);
    assert!(facade::set_options(k.ctx(), 0x18a));
    let mut out = obj! { "before" => character_offset(k) };
    assert!(parse(k, "n") > 0);
    out.set("training", train_selected_top(k, "哪", token, true));
    out.set("clear", facade::clear_constraint(k.inst(), 0));
    out.set("after", character_offset(k));
    out
}

/// `export_open_across_import_system`: a system export cursor kept open while
/// another reading is imported for its next token.
fn export_open_across_import_system(k: &mut Kit) -> J {
    let inst = k.inst();
    let counts: Vec<u32> = [0x0100_0001, 0x0100_0002]
        .into_iter()
        .map(|token| {
            let mut count = UNTOUCHED;
            assert!(facade::token_get_n_pronunciation(
                inst,
                token,
                &raw mut count
            ));
            count
        })
        .collect();
    let (mut length, mut text) = (UNTOUCHED, UNTOUCHED as usize as *mut c_char);
    assert!(facade::token_get_phrase(
        inst,
        0x0100_0002,
        &raw mut length,
        &raw mut text
    ));
    let phrase = take_text(text).expect("the phrase");
    // SAFETY: a fresh GArray of two-byte elements; it stays allocated, as the old case left it.
    let keys = unsafe { glib_sys::g_array_new(0, 0, 2) };
    assert!(facade::token_get_nth_pronunciation(
        inst,
        0x0100_0002,
        0,
        keys.cast()
    ));
    let syllables: Vec<String> = crate::readers::garray_u16(keys)
        .into_iter()
        .map(|mut key| {
            let mut rendered = UNTOUCHED as usize as *mut c_char;
            assert!(facade::get_pinyin_string(
                inst,
                (&raw mut key).cast(),
                &raw mut rendered
            ));
            take_text(rendered).expect("a syllable")
        })
        .collect();
    let reading = syllables
        .iter()
        .map(|syllable| format!("{syllable}1"))
        .collect::<Vec<_>>()
        .join("'");
    let it = facade::begin_get_phrases(k.ctx(), 1);
    let mut out = obj! { "counts" => counts.clone(), "phrase" => phrase.clone(), "reading" => reading.clone() };
    out.set("first", export_rows(it, Some(counts[0] as usize)));
    out.set("added", import_rows(k, 1, &[(&phrase, &reading, 5)]));
    out.set("second", export_rows(it, Some(counts[1] as usize + 1)));
    facade::end_get_phrases(it);
    close(k);
    out
}

/// `export_open_across_import_user`: a live user export cursor across the
/// import of a new token and a new reading.
fn export_open_across_import_user(k: &mut Kit) -> J {
    let mut out = obj! { "seed" => import_rows(k, 7, &[("你好", "ni'hao", 5)]) };
    let it = facade::begin_get_phrases(k.ctx(), 7);
    out.set("has_next", facade::iterator_has_next_phrase(it));
    out.set(
        "added",
        import_rows(k, 7, &[("好你", "hao'ni", 7), ("你好", "ni2'hao2", 3)]),
    );
    out.set("rows", export_rows(it, None));
    out.set("has_next_after", facade::iterator_has_next_phrase(it));
    facade::end_get_phrases(it);
    facade::fini(k.ctx());
    k.forget();
    out
}

/// `user_library_training_bytes`: the whole `user.bin` chunk after import,
/// training and mask-out.
fn user_library_training_bytes(k: &mut Kit) -> J {
    let phrase = "龘龘";
    let readings = if facade::ZHUYIN {
        ["ㄉㄚˇ ㄉㄚˇ", "ㄉㄚˊ ㄉㄚˊ"]
    } else {
        ["da3'da3", "da2'da2"]
    };
    let rows: Vec<(&str, &str, i32)> = readings
        .iter()
        .map(|reading| (phrase, *reading, 5))
        .collect();
    let mut out = obj! { "imported" => import_rows(k, 7, &rows) };
    let token = tokens_of(k, phrase)
        .into_iter()
        .find(|token| token >> 24 == 7)
        .expect("a user token");
    let save = |k: &mut Kit, out: &mut J, tag: &str| {
        out.set(format!("{tag}-saved"), facade::save(k.ctx()));
        let bytes = std::fs::read(k.user.join("user.bin")).expect("user.bin");
        out.set(format!("{tag}-bytes"), hex_bytes(&bytes));
    };
    save(k, &mut out, "import");
    // USE_TONE without FORCE_TONE admits the zero-tone wildcard on both facades.
    assert!(facade::set_options(k.ctx(), 1 << 5));
    assert_eq!(parse(k, "dada"), 4);
    out.set("trained", train_selected_top(k, phrase, token, false));
    save(k, &mut out, "train");
    out.set("masked", facade::mask_out(k.ctx(), 0xffff_ffff, token));
    assert!(import_rows(k, 7, &[]).is_empty());
    save(k, &mut out, "mask");
    out
}
