//! The user_stored_words family's case bodies: each answers the result document the old case returned.
//!
//! Inventory row 41 (#1175, #1198): an imported item's stored words replaced
//! with a raw word no import writes, in the journal (library 1) or a user
//! file (5 to 7). Load copies the raw words (`pinyin.cpp:303-318`,
//! `phrase_index.cpp:335-389,439-459`); the token text, the export, the
//! sentence and the candidates show them losslessly, and save copies the
//! stored content (`pinyin.cpp:978-988`).

use crate::bodies::system_phrase_text::{
    candidate_text_rows, phrase_text_bytes, phrase_text_system, phrase_text_word, seeded_text,
    sentence_into,
};
use crate::facade;
use crate::fixtures::{chunk, ucs4_key};
use crate::readers::{c, import_rows, tokens_of, unigram_of};
use crate::support::{J, Kit, UNTOUCHED};

/// Runs the case `id` and answers its result document.
pub fn case(k: &mut Kit, id: &str) -> J {
    let rest = id
        .strip_prefix("user-stored-words/")
        .expect("a user-stored-words case");
    let (_mode, rest) = rest.split_once('-').expect("a mode");
    let (library, variant) = rest.split_once('-').expect("a library");
    user_stored_words(k, library.parse().expect("a library"), variant)
}

/// Frees the cached instance and finalises the cached context; both reopen lazily.
fn close(k: &mut Kit) {
    facade::free_instance(k.inst());
    facade::fini(k.ctx());
    k.forget();
}

/// `user_stored_words`: 甲戊乙 imported into `library` over the private
/// system and saved, its stored words replaced by 甲<word>乙, then observed
/// loaded, saved again (`native`) and reloaded.
fn user_stored_words(k: &mut Kit, library: u8, variant: &str) -> J {
    phrase_text_system(k, "valid");
    let reading = if facade::ZHUYIN {
        "ㄉㄚ ㄉㄚ ㄉㄚ"
    } else {
        "da1'da1'da1"
    };
    assert_eq!(
        import_rows(k, library, &[("甲戊乙", reading, 1000)]),
        [true]
    );
    let token = tokens_of(k, "甲戊乙")
        .into_iter()
        .find(|token| token >> 24 == u32::from(library))
        .expect("the imported token");
    assert!(facade::save(k.ctx()));
    close(k);
    let filename = match library {
        1 => "gb_char.dbin",
        5 => "addon.bin",
        6 => "network.bin",
        7 => "user.bin",
        other => panic!("no library {other}"),
    };
    let path = k.user.join(filename);
    let original = ucs4_key("甲戊乙");
    let replacement: Vec<u8> = [0x7532, phrase_text_word(variant), 0x4e59]
        .iter()
        .flat_map(|word: &u32| word.to_le_bytes())
        .collect();
    let payload = std::fs::read(&path).expect("the saved library")[8..].to_vec();
    let at: Vec<usize> = payload
        .windows(original.len())
        .enumerate()
        .filter(|(_, window)| *window == original.as_slice())
        .map(|(at, _)| at)
        .collect();
    assert_eq!(at.len(), 1, "one imported item to replace");
    let mut replaced = payload;
    replaced.splice(at[0]..at[0] + original.len(), replacement);
    std::fs::write(&path, chunk(&replaced)).expect("replace the stored words");
    let mut out = obj! { "loaded" => observe(k, token, library) };
    // Ending an import batch arms save without changing an item.
    assert!(import_rows(k, 7, &[]).is_empty());
    out.set("saved", facade::save(k.ctx()));
    let native: String = std::fs::read(&path)
        .expect("the saved library")
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    out.set("native", native);
    close(k);
    out.set("reloaded", observe(k, token, library));
    out
}

/// `library`'s export rows (pinyin), text and reading as hex, at most four.
fn export_rows(k: &mut Kit, library: u8) -> Vec<(bool, Option<String>, Option<String>, i32)> {
    let it = facade::begin_get_phrases(k.ctx(), u32::from(library));
    let mut rows = Vec::new();
    while facade::iterator_has_next_phrase(it) {
        assert!(rows.len() < 4, "bounded fixture export");
        let (mut phrase, mut pinyin, mut count) = (seeded_text(), seeded_text(), UNTOUCHED as i32);
        let got =
            facade::iterator_get_next_phrase(it, &raw mut phrase, &raw mut pinyin, &raw mut count);
        rows.push((
            got,
            phrase_text_bytes(phrase, true),
            phrase_text_bytes(pinyin, true),
            count,
        ));
    }
    facade::end_get_phrases(it);
    rows
}

/// `observe`: the token's text and unigram, the export (pinyin), then a
/// da1da1da1ba1 guess: its sentence and candidate list.
fn observe(k: &mut Kit, token: u32, library: u8) -> J {
    let inst = k.inst();
    let (mut length, mut text) = (UNTOUCHED, seeded_text());
    let ret = facade::token_get_phrase(inst, token, &raw mut length, &raw mut text);
    let mut result = obj! {
        "token" => (ret, length, phrase_text_bytes(text, true)),
        "unigram" => unigram_of(k, token),
    };
    if !facade::ZHUYIN {
        result.set("export", export_rows(k, library));
    }
    assert!(facade::set_options(k.ctx(), 1 << 5));
    result.set(
        "parsed",
        facade::parse_more_full_pinyins(inst, c("da1da1da1ba1").as_ptr()),
    );
    result.set("guessed", facade::guess_sentence(inst));
    let mut sentence = seeded_text();
    let read = sentence_into(inst, &mut sentence);
    let listed = if facade::ZHUYIN {
        facade::guess_candidates_after_cursor(inst, 0)
    } else {
        facade::guess_candidates(inst, 0, 0x1e)
    };
    result.set("sentence", (read, phrase_text_bytes(sentence, true)));
    result.set("candidates", (listed, candidate_text_rows(inst)));
    result
}
