//! Removed items: the search paths over a journal REMOVE's phrase, its
//! candidate list and the prediction walk over it (#1231).

use std::collections::BTreeSet;

use super::{Library, close, guess_j, phrase_text, remove_record, system_items, token_phrase_of};
use crate::facade;
use crate::fixtures::{modify_header, write_log, write_user_conf};
use crate::readers::{c, garray_u16, guess_rows, owned_out, seeded, zhuyin_guess_rows};
use crate::support::{J, Kit, P, take_text};

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
pub(super) fn removed_extension(k: &mut Kit) -> J {
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
    let inst = k.alloc();
    let parsed = facade::parse_more_full_pinyins(inst, c(&reading).as_ptr());
    let mut observed = obj! { "parsed" => parsed };
    observed.set("guessed", facade::guess_sentence(inst));
    observed.set("sentence", sentence_out(inst));
    if facade::ZHUYIN {
        observed.set("candidates", guess_j(zhuyin_guess_rows(inst, 0)));
    } else {
        observed.set("candidates", guess_j(guess_rows(inst, 0, 0x1e)));
    }
    facade::free_instance(inst);
    out.set("exact", observed);
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
pub(super) fn removed_candidate(k: &mut Kit) -> J {
    let mut out = removed_extension(k);
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
