//! Removed items: the search paths over a journal REMOVE's phrase, its
//! candidate list and the prediction walk over it (#1231).

use std::collections::BTreeSet;

use super::{
    Library, close, guess_j, item_text, phrase_text, remove_record, system_items, token_phrase_of,
    unigram_j,
};
use crate::facade;
use crate::fixtures::{modify_header, write_log, write_user_conf};
use crate::readers::{
    c, candidate_rows, garray_u16, guess_rows, owned_out, seeded, zhuyin_guess_rows,
};
use crate::support::{J, Kit, P, UNTOUCHED, borrowed_text, take_text};

/// The raw pronunciation key sequences of `item`, by its character count and
/// entry stride (`PhraseItem::get_nth_pronunciation`, `phrase_index.cpp:33-43`).
fn readings(item: &[u8]) -> Vec<&[u8]> {
    let (length, prons) = (usize::from(item[0]), usize::from(item[1]));
    let start = 6 + 4 * length;
    let stride = 2 * length + 4;
    (0..prons)
        .map(|i| {
            let at = (start + stride * i).min(item.len());
            &item[at..(at + 2 * length).min(item.len())]
        })
        .collect()
}

/// Plants a REMOVE of `token`'s `item` in merged.dbin under a header that
/// keeps the library total.
fn plant_remove(k: &Kit, library: &Library, token: u32, item: &[u8]) {
    write_user_conf(k);
    let log = [
        modify_header(0, library.total, library.total),
        remove_record(token, item),
    ]
    .concat();
    write_log(k, &log, "merged.dbin");
}

/// A removed item and the prefix it extends. `SEARCH_CONTINUED` comes from the
/// pinyin table's DB get alone (`chewing_large_table2_bdb.cpp:182-187` at
/// 074a2219): a REMOVE clears only the sub-index (`phrase_index.cpp:219-236`), so
/// the table still reports the removed phrase's key path as continuing, and the
/// item drops out later, where `get_phrase_item` fails (`phonetic_lookup.h:646`,
/// `:674`). The probe removes the one four-character phrase of the merged
/// library that nothing extends, then looks at what the C ABI shows of a search
/// over its reading: the parse and the guessed sentence for the reading, for a
/// reading that goes on past it and for the prefix the phrase extends; and the
/// candidate list of the prefix. The pin lists the removed token itself through
/// a stale item (a SIGSEGV), so no candidate list is taken where it is one.
pub(super) fn removed_extension(k: &mut Kit, candidates: bool, labels: &[&str], before: bool) -> J {
    let library = system_items(&k.data, "merged.bin");
    // The item's key path must be no other phrase's prefix, in any system
    // library: only then does the removal leave nothing that extends it.
    let mut longer: BTreeSet<Vec<u8>> = BTreeSet::new();
    for name in ["merged.bin", "opengram.bin", "gb_char.bin", "gbk_char.bin"] {
        for item in system_items(&k.data, name).items.values() {
            if item[0] > 4 {
                longer.extend(
                    readings(item)
                        .iter()
                        .map(|reading| reading[..8.min(reading.len())].to_vec()),
                );
            }
        }
    }
    let slot =
        library.first(|item| item[0] == 4 && item[1] == 1 && !longer.contains(readings(item)[0]));
    let token = 0x0400_0000 | slot;
    // Read the phrase and its reading from the untouched library first.
    let phrase = phrase_text(&token_phrase_of(k, token));
    let inst = k.inst();
    // SAFETY: a fresh GArray of two-byte elements, freed below.
    let keys = unsafe { glib_sys::g_array_new(0, 0, 2) };
    assert!(facade::token_get_nth_pronunciation(
        inst,
        token,
        0,
        keys.cast()
    ));
    let mut syllables = Vec::new();
    for mut key in garray_u16(keys) {
        let mut rendered = seeded();
        assert!(facade::get_pinyin_string(
            inst,
            (&raw mut key).cast(),
            &raw mut rendered
        ));
        syllables.push(take_text(rendered).expect("a rendered syllable"));
    }
    // SAFETY: the array was allocated above and is freed once.
    unsafe { glib_sys::g_array_free(keys, 1) };
    let reading = syllables.concat();
    close(k);
    plant_remove(k, &library, token, &library.items[&slot]);
    let mut out = obj! {
        "phrase" => phrase,
        "reading" => reading.clone(),
        "removed" => token_phrase_of(k, token),
    };
    if facade::ZHUYIN {
        assert!(facade::set_options(k.ctx(), 1 << 5));
    }
    for label in labels {
        let text = match *label {
            "exact" => reading.clone(),
            "longer" => format!("{reading}ni"),
            "prefix" => syllables[..syllables.len() - 1].concat(),
            other => panic!("no search label {other}"),
        };
        let inst = k.alloc();
        let parsed = facade::parse_more_full_pinyins(inst, c(&text).as_ptr());
        let mut observed = obj! { "parsed" => parsed };
        observed.set("guessed", facade::guess_sentence(inst));
        observed.set("sentence", sentence_out(inst));
        if candidates && !facade::ZHUYIN {
            observed.set("candidates", guess_j(guess_rows(inst, 0, 0x1e)));
        } else if candidates && before {
            let ret = facade::guess_candidates_before_cursor(inst, parsed);
            observed.set(
                "candidates",
                obj! { "ret" => ret, "rows" => candidate_rows(inst) },
            );
        } else if candidates {
            observed.set("candidates", guess_j(zhuyin_guess_rows(inst, 0)));
        }
        facade::free_instance(inst);
        out.set(*label, observed);
    }
    out
}

/// `sentence_out(k, inst, 0)` / `zhuyin_sentence_out`: the flag and the text,
/// `None` or `"untouched"`.
fn sentence_out(inst: P) -> J {
    let mut out = seeded();
    let ret = if facade::ZHUYIN {
        facade::get_sentence_only(inst, &raw mut out)
    } else {
        facade::get_sentence_at(inst, 0, &raw mut out)
    };
    J::from((ret, owned_out(out)))
}

/// The candidate list over the removed token itself. `_compute_phrase_strings_of_items`
/// leaves `m_phrase_string` unset when `_token_get_phrase` finds no item
/// (`pinyin.cpp:1635-1637`, result ignored at `:2013-2016`; `zhuyin.cpp:1103-1105`),
/// and `_remove_duplicated_items_by_phrase_string` hands the NULL to `strcmp` in
/// `compare_indexed_item_with_phrase_string` (`pinyin.cpp:2053`, called from
/// `pinyin_guess_candidates` at `:2300`; `zhuyin.cpp:1360`, from
/// `zhuyin_guess_candidates_after_cursor` at `:1537`): SIGSEGV. A removed token
/// stays in the phrase table, so a search over its reading lists it. oxpinyin
/// lists no candidate for an item that does not exist.
///
/// Row 102's answer for the site: an item a reload freed adds no candidate and
/// the call returns normally. A removed item is the same case, a token whose item
/// `get_phrase_item` no longer finds, so the calls answer `true`.
pub(super) fn removed_candidate(k: &mut Kit, before: bool) -> J {
    let mut out = removed_extension(k, true, &["exact"], before);
    let J::M(map) = &mut out else { unreachable!() };
    let exact = map.remove("exact").expect("the exact search");
    let field = |key: &str| exact.get(key).expect("a field").clone();
    let candidates = field("candidates");
    out.set("exact-parsed", field("parsed"));
    out.set("exact-guessed", field("guessed"));
    out.set(
        "exact-candidates-ret",
        candidates.get("ret").expect("ret").clone(),
    );
    out.set(
        "~exact-candidates",
        candidates.get("rows").expect("rows").clone(),
    );
    out
}

/// #1231: a system journal REMOVE leaves the phrase-table token, and the
/// prediction walk (`_compute_predicted_prefix_candidates`) reads its item
/// without checking the result (`pinyin.cpp:2389-2405` at 074a2219). The
/// loop-wide `PhraseItem` keeps the previous success's length, so a missing
/// token can pass or be skipped; an appended one carries a NULL phrase
/// string. With a sole row both dedup comparisons are skipped and the NULL
/// survives; with two or more rows `strcmp` reads it (row 106's class (b)).
/// The fixture removes one merged.bin phrase whose prefix is a stored phrase
/// and observes the plain prediction API.
///
/// `_removed_predict_setup`: removes the merged.bin phrase whose item text is
/// `phrase`, closes the cached context and instance and plants a REMOVE log.
/// Answers the removed token; the phrase table keeps it.
fn removed_predict_setup(k: &mut Kit, phrase: &str) -> u32 {
    let library = system_items(&k.data, "merged.bin");
    let slot = library.first(|item| item_text(item).as_deref() == Some(phrase));
    let token = 0x0400_0000 | slot;
    close(k);
    plant_remove(k, &library, token, &library.items[&slot]);
    token
}

/// `_predicted_surface`: the plain prediction API's flag, count and rows, each
/// row every getter's flag, the type, and the string with NULL kept distinct
/// from an empty one; and the candidate pointers, instance-borrowed.
fn predicted_surface(inst: P, prefix: &str) -> (J, Vec<P>) {
    let ret = facade::guess_predicted_candidates(inst, c(prefix).as_ptr());
    let mut count = 0_u32;
    let got = facade::get_n_candidate(inst, &raw mut count);
    let mut rows = Vec::new();
    let mut pointers = Vec::new();
    for index in 0..count {
        let (mut cand, mut kind, mut string): (P, i32, *const std::ffi::c_char) =
            (std::ptr::null_mut(), 0, std::ptr::null());
        let fetched = facade::get_candidate(inst, index, &raw mut cand);
        let typed = facade::get_candidate_type(inst, cand, &raw mut kind);
        let rendered = facade::get_candidate_string(inst, cand, &raw mut string);
        rows.push(J::from((
            fetched,
            typed,
            kind,
            rendered,
            borrowed_text(string),
        )));
        pointers.push(cand);
    }
    let surface = obj! { "ret" => ret, "get-count" => got, "count" => count, "rows" => rows };
    (surface, pointers)
}

/// Removes 不仅仅是 and predicts from 不仅仅: the sole pre-dedup row. Records
/// the sole row's getters, user status, and the token's failed phrase read
/// with both outputs left unchanged.
pub(super) fn predicted_sole(k: &mut Kit) -> J {
    let token = removed_predict_setup(k, "不仅仅是");
    let inst = k.inst();
    let (surface, pointers) = predicted_surface(inst, "不仅仅");
    let (mut length, mut text) = (UNTOUCHED, seeded());
    let phrase_ret = facade::token_get_phrase(inst, token, &raw mut length, &raw mut text);
    obj! {
        "predicted" => surface,
        "token-phrase" => (phrase_ret, length == UNTOUCHED, text == seeded()),
        "is-user" => pointers.first().map(|&cand| facade::is_user_candidate(inst, cand)),
    }
}

/// Chooses the sole missing row, then reads the token's frequency. The pin
/// advances the loaded facade total by 483 and leaves the missing item
/// unwritten; the later unigram read still fails with its seeded output
/// unchanged. (The facade total has no C getter; the runtime's own
/// amplified-total assertion covers that side effect.)
pub(super) fn predicted_choose(k: &mut Kit) -> J {
    let token = removed_predict_setup(k, "不仅仅是");
    let inst = k.inst();
    let (surface, pointers) = predicted_surface(inst, "不仅仅");
    let before = unigram_j(k, token);
    let chosen = pointers
        .first()
        .map(|&cand| facade::choose_predicted_candidate(inst, cand));
    let after = unigram_j(k, token);
    obj! {
        "predicted" => surface,
        "choose" => chosen,
        "unigram-before" => before,
        "unigram-after" => after,
    }
}

/// The flag, count and rows of a prediction after removing `phrase`.
fn predicted_rows(k: &mut Kit, phrase: &str, prefix: &str) -> J {
    removed_predict_setup(k, phrase);
    let (surface, _pointers) = predicted_surface(k.inst(), prefix);
    let field = |key: &str| surface.get(key).expect("a field").clone();
    obj! { "ret" => field("ret"), "count" => field("count"), "rows" => field("rows") }
}

/// Removes 这样一个 and predicts from 这样一: the missing row and a live row.
/// The pin reaches the NULL phrase string in dedup (SIGSEGV); the subject
/// drops the missing row and keeps the live prefix candidate.
pub(super) fn predicted_class_b(k: &mut Kit) -> J {
    predicted_rows(k, "这样一个", "这样一")
}
