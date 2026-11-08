//! The candidate_removals family's case bodies: each answers the result document the old case returned.
//!
//! Inventory row 32 (#1175, #525): candidate removals and refusals. The pin
//! asserts where a candidate of the wrong type, or a user phrase it cannot
//! remove, reaches a removal or selection call; the controls exercise the rows
//! the guards leave alone.

use std::ops::Bound;
use std::path::Path;

use oxpinyin_store::{DefaultStore, RawReadStore as _};

use crate::facade;
use crate::fixtures::{craft_hash, craft_tree, write_user_conf};
use crate::readers::{
    add_user_phrase, c, candidate_at, candidate_type, full_inst, lookup, n_candidate, row_of,
};
use crate::support::{J, Kit, P};

/// `row_of`, NULL where no row of `kind` exists (ctypes passed `None` as NULL).
fn row(inst: P, kind: i32) -> P {
    row_of(inst, kind).unwrap_or(std::ptr::null_mut())
}

/// `contract_abi.guessed`: a fresh instance with `nihao` parsed, a sentence
/// guess and a candidate guess at byte zero with sort zero.
fn guessed(k: &mut Kit) -> P {
    let inst = k.alloc();
    facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
    facade::guess_sentence(inst);
    facade::guess_candidates(inst, 0, 0);
    inst
}

/// `predicted`: a fresh instance with predictions and punctuation for 我.
fn predicted(k: &mut Kit) -> P {
    let inst = k.alloc();
    facade::guess_predicted_candidates_with_punctuations(inst, c("我").as_ptr());
    inst
}

/// `_user_candidate_rows`: the NORMAL user candidates of the current list.
fn user_candidate_rows(inst: P) -> Vec<P> {
    (0..n_candidate(inst))
        .map(|index| candidate_at(inst, index).unwrap_or(std::ptr::null_mut()))
        .filter(|&cand| candidate_type(inst, cand) == 2 && facade::is_user_candidate(inst, cand))
        .collect()
}

/// `index_file_rows`: a saved tree DBM's rows, sorted.
fn index_rows(path: &Path) -> Vec<(Vec<u8>, Vec<u8>)> {
    let store = DefaultStore::open_read_only(path).expect("the saved index opens");
    let mut rows = Vec::new();
    store
        .range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
            rows.push((key.to_vec(), value.to_vec()));
            Ok(())
        })
        .expect("read the saved index");
    rows.sort();
    rows
}

/// A fresh context on the kit's directories, an instance with `text` parsed and
/// a candidate guess at zero with sort zero; the cached context is not replaced.
fn reopened_guess(k: &Kit, text: &str) -> P {
    let ctx = k.init_default();
    assert!(!ctx.is_null());
    let inst = facade::alloc_instance(ctx);
    facade::parse_more_full_pinyins(inst, c(text).as_ptr());
    facade::guess_candidates(inst, 0, 0);
    inst
}

/// Imports and saves 龘龘, then finalises the cached context.
fn saved_dalong(k: &mut Kit, count: i32) {
    add_user_phrase(k, "龘龘", &[b"da2'da2"], count);
    assert!(facade::save(k.ctx()));
    facade::fini(k.ctx());
}

/// `_guessed_user_phrase`: imports the phrase, guesses `dada` on a fresh
/// instance and answers its user rows.
fn guessed_user_phrase(k: &mut Kit, readings: &[&[u8]]) -> (P, Vec<P>) {
    add_user_phrase(k, "龘龘", readings, 5);
    let inst = k.alloc();
    facade::parse_more_full_pinyins(inst, c("dada").as_ptr());
    facade::guess_candidates(inst, 0, 0);
    let rows = user_candidate_rows(inst);
    assert!(!rows.is_empty());
    (inst, rows)
}

fn remove_twice(k: &mut Kit) -> J {
    let (inst, rows) = guessed_user_phrase(k, &[b"da2'da2"]);
    let first = facade::remove_user_candidate(inst, rows[0]);
    assert!(first, "the first removal did not complete");
    obj! { "first" => first, "ret" => facade::remove_user_candidate(inst, rows[0]) }
}

fn missing_phrase_table(k: &mut Kit) -> J {
    saved_dalong(k, 5);
    let index = k.user.join("user_phrase_index.bin");
    std::fs::rename(&index, k.user.join("user_phrase_index.bin.gone")).expect("move the index");
    let inst = reopened_guess(k, "dada");
    let rows = user_candidate_rows(inst);
    assert!(!rows.is_empty());
    obj! { "ret" => facade::remove_user_candidate(inst, rows[0]) }
}

/// A non-empty two-key row whose keys are both incomplete (initials only).
fn incomplete(key: &[u8], value: &[u8]) -> bool {
    !value.is_empty()
        && key.len() == 4
        && [0, 2]
            .iter()
            .all(|&at| u16::from_le_bytes([key[at], key[at + 1]]) <= 31)
}

fn one_projection(k: &mut Kit) -> J {
    saved_dalong(k, 5);
    let path = k.user.join("user_pinyin_index.bin");
    let rows = index_rows(&path);
    assert_eq!(
        rows.iter()
            .filter(|(key, value)| incomplete(key, value))
            .count(),
        1
    );
    let kept: Vec<_> = rows
        .into_iter()
        .filter(|(key, value)| !incomplete(key, value))
        .collect();
    craft_tree(&path, &kept);
    let inst = reopened_guess(k, "dada");
    let rows = user_candidate_rows(inst);
    assert!(!rows.is_empty());
    obj! { "ret" => facade::remove_user_candidate(inst, rows[0]) }
}

fn suggestion_without_marker(k: &mut Kit) -> J {
    saved_dalong(k, 1_000_000_000);
    let path = k.user.join("user_pinyin_index.bin");
    let rows = index_rows(&path);
    assert!(rows.iter().any(|(key, _)| key.len() == 2));
    assert!(
        rows.iter()
            .any(|(key, value)| key.len() == 4 && !value.is_empty())
    );
    let kept: Vec<_> = rows.into_iter().filter(|(key, _)| key.len() != 2).collect();
    craft_tree(&path, &kept);
    let ctx = k.init_default();
    assert!(!ctx.is_null());
    let inst = facade::alloc_instance(ctx);
    assert!(facade::parse_more_full_pinyins(inst, c("da").as_ptr()) != 0);
    facade::guess_candidates(inst, 0, 0);
    let mut found = 0_u32;
    for index in 0..n_candidate(inst) {
        let cand = candidate_at(inst, index).unwrap_or(std::ptr::null_mut());
        let mut text: *const std::ffi::c_char = std::ptr::null();
        if facade::get_candidate_string(inst, cand, &raw mut text) {
            found += u32::from(crate::support::borrowed_text(text).as_deref() == Some("龘龘"));
        }
    }
    obj! { "user_candidates" => found }
}

/// `_removable_user_bigram_context`: a reopened context whose user bigram holds
/// `rows_for(token)` of the saved 龘龘's token; its instance and user row.
fn removable_user_bigram(
    k: &mut Kit,
    rows_for: impl FnOnce(u32) -> Vec<(Vec<u8>, Vec<u8>)>,
) -> (P, P) {
    add_user_phrase(k, "龘龘", &[b"da2'da2"], 5);
    assert!(facade::save(k.ctx()));
    let probe = k.alloc();
    let token = lookup(probe, "龘龘".as_bytes())[0];
    facade::free_instance(probe);
    facade::fini(k.ctx());
    craft_hash(&k.user.join("user_bigram.db"), &rows_for(token));
    write_user_conf(k);
    let ctx = k.init_default();
    assert!(!ctx.is_null(), "the reopen failed");
    let inst = facade::alloc_instance(ctx);
    facade::parse_more_full_pinyins(inst, c("dada").as_ptr());
    assert!(facade::guess_candidates(inst, 0, 0));
    let candidates = user_candidate_rows(inst);
    assert!(!candidates.is_empty(), "no user candidate");
    (inst, candidates[0])
}

fn bigram_removal(k: &mut Kit, id: &str) -> J {
    let (inst, cand) = removable_user_bigram(k, |token| match id {
        "abort-remove-user-candidate-short-bigram-value" => {
            vec![(b"\x01\x00\x00\x00".to_vec(), b"\x07\x00\x00".to_vec())]
        }
        "abort-remove-user-candidate-non-token-bigram-key" => {
            vec![(b"\x00\x01".to_vec(), b"\x07\x00\x00\x00".to_vec())]
        }
        _ => {
            let value = [
                &7_u32.to_le_bytes()[..],
                &token.to_le_bytes(),
                &3_u32.to_le_bytes(),
            ]
            .concat();
            vec![(b"\x01\x00\x00\x00".to_vec(), value)]
        }
    });
    obj! { "ret" => facade::remove_user_candidate(inst, cand) }
}

/// `choose_without_sentence`: chooses a NORMAL `nihao` row at `offset` from a
/// list guessed at zero under SORT_WITHOUT_SENTENCE_CANDIDATE.
fn choose_without_sentence(k: &mut Kit, offset: usize) -> J {
    let inst = full_inst(k, b"nihao");
    facade::guess_candidates(inst, 0, 0x1);
    let normal = row_of(inst, 2).expect("no NORMAL row");
    obj! { "ret" => facade::choose_candidate(inst, offset, normal) }
}

fn neighbours(k: &mut Kit) -> J {
    let mut out = J::map();
    let inst = guessed(k);
    let mut nbest: u8 = 0xAB;
    let ret = facade::get_candidate_nbest_index(inst, row(inst, 1), &raw mut nbest);
    out.set("nbest of sentence row", (ret, nbest));
    let user: Vec<bool> = [1, 2]
        .iter()
        .map(|&kind| facade::is_user_candidate(inst, row(inst, kind)))
        .collect();
    out.set("is user", user);
    out.set(
        "choose normal",
        facade::choose_candidate(inst, 0, row(inst, 2)),
    );
    let pred = predicted(k);
    out.set(
        "choose predicted prefix",
        facade::choose_predicted_candidate(pred, row(pred, 5)),
    );
    out.set(
        "choose predicted punctuation",
        facade::choose_predicted_candidate(pred, row(pred, 8)),
    );
    out
}

fn nbest_normal_row(k: &mut Kit) -> J {
    let inst = guessed(k);
    let mut out: u8 = 0xAB;
    let ret = facade::get_candidate_nbest_index(inst, row(inst, 2), &raw mut out);
    obj! { "ret" => ret, "untouched index" => out == 0xAB }
}

/// Runs the case `id`.
pub fn case(k: &mut Kit, id: &str) -> J {
    match id {
        "abort-get-candidate-nbest-index-normal-row" => nbest_normal_row(k),
        "abort-remove-user-candidate-normal-system-token" => {
            let inst = guessed(k);
            obj! { "ret" => facade::remove_user_candidate(inst, row(inst, 2)) }
        }
        "abort-remove-user-candidate-sentence-row" => {
            let inst = guessed(k);
            obj! { "ret" => facade::remove_user_candidate(inst, row(inst, 1)) }
        }
        "abort-remove-user-candidate-twice" => remove_twice(k),
        "abort-remove-user-candidate-missing-phrase-table" => missing_phrase_table(k),
        "abort-remove-user-candidate-multi-pron" => {
            let (inst, rows) = guessed_user_phrase(k, &[b"da2'da2", b"da1'da1"]);
            obj! { "ret" => facade::remove_user_candidate(inst, rows[0]) }
        }
        "abort-remove-user-candidate-one-projection" => one_projection(k),
        "user-index-suggestion-without-prefix-marker" => suggestion_without_marker(k),
        "abort-remove-user-candidate-short-bigram-value"
        | "abort-remove-user-candidate-non-token-bigram-key"
        | "abort-remove-user-candidate-residual-bigram-gram" => bigram_removal(k, id),
        "abort-choose-candidate-predicted-prefix-row" => {
            let inst = predicted(k);
            obj! { "ret" => facade::choose_candidate(inst, 0, row(inst, 5)) }
        }
        "abort-choose-predicted-candidate-normal-row" => {
            let inst = guessed(k);
            obj! { "ret" => facade::choose_predicted_candidate(inst, row(inst, 2)) }
        }
        "abort-choose-predicted-candidate-sentence-row" => {
            let inst = guessed(k);
            obj! { "ret" => facade::choose_predicted_candidate(inst, row(inst, 1)) }
        }
        "abort-choose-candidate-without-sentence-offset-1" => choose_without_sentence(k, 1),
        "choose-candidate-without-sentence-offset-0" => choose_without_sentence(k, 0),
        "candidate-type-neighbours" => neighbours(k),
        other => panic!("no case {other}"),
    }
}
