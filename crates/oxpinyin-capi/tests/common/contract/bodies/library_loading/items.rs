//! Crafted items inside a system library: shared and overlapping extents, a
//! replacement reloaded without a user directory, and a SYSTEM_FILE above 4.

use std::ffi::c_char;

use super::{
    at_offset, hex, init, le, le16, modify_header, token_phrase, unigram, word, write_log,
};
use crate::facade;
use crate::fixtures::{
    ConfEdit, chunk, conf_system, items_system, private_system, write_user_conf,
};
use crate::readers::{c, import_rows, lookup, owned_out, seeded, tokens_of};
use crate::support::{J, Kit, P, UNTOUCHED, take_text};

/// `aliased_offset_chunk`: a library-four chunk whose slots 1 and 2 share one
/// offset. Slot 1's and slot 3's items are the shipped merged.bin's own, so
/// their text and keys are real; slot 2's word repeats slot 1's. Both builders
/// emit distinct offsets (`phrase_index.cpp:208-216`), so only a crafted file
/// holds the alias; neither loader rejects it (`:335-362`).
fn aliased_offset_chunk(k: &Kit) -> Vec<u8> {
    let source = std::fs::read(k.data.join("merged.bin")).expect("merged.bin");
    let payload = &source[8..8 + at_offset(word(&source, 0))];
    let (one, two, three) = (
        at_offset(word(payload, 4)),
        at_offset(word(payload, 8)),
        at_offset(word(payload, 12)),
    );
    let (offsets, content) = (&payload[one..two - 1], &payload[two..three - 1]);
    let item = |slot: usize| {
        let at = at_offset(word(offsets, 4 * slot));
        let (length, prons) = (usize::from(content[at]), usize::from(content[at + 1]));
        &content[at..at + 6 + 4 * length + prons * (2 * length + 4)]
    };
    let (first, third) = (item(1), item(3));
    let body = [&[0_u8; 8][..], first, third].concat();
    let first_len = u32::try_from(first.len()).expect("an item length");
    let words = [0, 8, 8, 8 + first_len];
    let total = word(first, 2).wrapping_add(word(third, 2));
    let index_two = 17 + 4 * 4 + 1;
    let body_len = u32::try_from(body.len()).expect("a body length");
    let mut payload: Vec<u8> = [total, 17, index_two, index_two + body_len + 1]
        .iter()
        .flat_map(|value| le(*value))
        .collect();
    payload.push(b'#');
    payload.extend(words.iter().flat_map(|value| le(*value)));
    payload.push(b'#');
    payload.extend(&body);
    payload.push(b'#');
    chunk(&payload)
}

/// `aliased_offset_unigram`: a unigram added through slot 1 of the aliased
/// pair, slot 2 read, saved, reopened and read. The shared bytes make slot 2
/// show slot 1's delta at once (`phrase_index.cpp:150-178`, `:180-205`). The
/// save diffs the file on disk again and emits one MODIFY per slot (`:392-437`,
/// `pinyin.cpp:946-963`); the reinit applies slot 1's in place, stops at slot
/// 2's mismatch (`:487-493`, ignored at `pinyin.cpp:276`) and drops slot 3's
/// record, so slot 3 reads its shipped count again.
pub(super) fn aliased_offset_unigram(k: &mut Kit) -> J {
    let system = private_system(k);
    let path = system.join("merged.bin");
    std::fs::remove_file(&path).expect("unlink merged.bin");
    std::fs::write(&path, aliased_offset_chunk(k)).expect("write merged.bin");
    let tokens = [0x0400_0001_u32, 0x0400_0002, 0x0400_0003];
    let reads = |inst: P| J::from(tokens.map(|token| J::from(unigram(inst, token))).to_vec());
    let ctx = init(k, &system);
    assert!(!ctx.is_null(), "aliased fixture init");
    let inst = facade::alloc_instance(ctx);
    let mut out = obj! { "before" => reads(inst) };
    out.set(
        "add-first",
        facade::token_add_unigram_frequency(inst, tokens[0], 5),
    );
    out.set("after-first", reads(inst));
    out.set(
        "add-third",
        facade::token_add_unigram_frequency(inst, tokens[2], 7),
    );
    out.set("after-third", reads(inst));
    out.set("save", facade::save(ctx));
    facade::free_instance(inst);
    facade::fini(ctx);
    let ctx = init(k, &system);
    assert!(!ctx.is_null(), "aliased fixture reinit");
    let inst = facade::alloc_instance(ctx);
    out.set("reopened", reads(inst));
    facade::free_instance(inst);
    facade::fini(ctx);
    out
}

/// One MODIFY record (`_log_record(3, ...)`).
fn modify(slot: u32, old: &[u8], new: &[u8]) -> Vec<u8> {
    [
        &le(3)[..],
        &le(slot),
        &le16(old.len()),
        &le16(new.len()),
        old,
        new,
    ]
    .concat()
}

/// `internally_overlapping_modify`: a replay through A with B ten bytes inside
/// it, then a save and a reopen. `phrase_index.cpp:180-205` derives each extent
/// from its header; `:500-508` writes through A, so B changes too. A stays 22
/// bytes (L=1, P=2); B grows from 10 to 16 bytes, still inside the content
/// buffer: row 107's defined in-buffer overlap. The ascending save (`:403-434`)
/// records both; reinit writes A, then stops on B's already-changed old-item
/// comparison (`:487-493`).
pub(super) fn internally_overlapping_modify(k: &mut Kit) -> J {
    let system = private_system(k);
    k.data = system.clone();
    // B occupies A's pronunciation bytes; its enlarged extent remains in bounds.
    let mut old = vec![1_u8, 2];
    old.extend(le(100));
    old.extend(le(0x4f60));
    old.extend([1, 0]);
    old.extend(le(50));
    old.extend(le(0x597d));
    old.extend([0, 0]);
    let mut new = old.clone();
    new[11] = 1;
    new[12..16].copy_from_slice(&le(77));
    let body = [&[0_u8; 8][..], &old, &le(7)].concat();
    let offsets: Vec<u8> = [0_u32, 8, 18].iter().flat_map(|n| le(*n)).collect();
    let content = 17 + u32::try_from(offsets.len()).expect("offsets") + 1;
    let body_len = u32::try_from(body.len()).expect("body");
    let head: Vec<u8> = [150, 17, content, content + body_len + 1]
        .iter()
        .flat_map(|n| le(*n))
        .collect();
    let path = system.join("gb_char.bin");
    std::fs::remove_file(&path).expect("unlink gb_char.bin");
    std::fs::write(
        &path,
        chunk(&[&head[..], b"#", &offsets, b"#", &body, b"#"].concat()),
    )
    .expect("write gb_char.bin");
    write_user_conf(k);
    write_log(
        k,
        &[modify_header(0, 150, 150), modify(1, &old, &new)].concat(),
        "gb_char.dbin",
    );
    let reads = |k: &mut Kit| {
        let inst = k.inst();
        let rows: Vec<J> = [0x0100_0001_u32, 0x0100_0002]
            .into_iter()
            .map(|token| {
                let freq = unigram(inst, token);
                let mut count = UNTOUCHED;
                let count_ret = facade::token_get_n_pronunciation(inst, token, &raw mut count);
                obj! {
                    "unigram" => freq,
                    "phrase" => token_phrase(inst, token),
                    "readings" => (count_ret, count),
                }
            })
            .collect();
        J::from(rows)
    };
    let mut out = obj! { "after-modify" => reads(k) };
    // Mark dirty without changing A or B, so save actually diffs the library.
    let reading = if facade::ZHUYIN {
        "ㄋㄧˇ ㄏㄠˇ"
    } else {
        "ni3'hao3"
    };
    let it = facade::begin_add_phrases(k.ctx(), 7);
    assert!(facade::iterator_add_phrase(
        it,
        c("你好").as_ptr(),
        c(reading).as_ptr(),
        1
    ));
    facade::end_add_phrases(it);
    out.set("save", facade::save(k.ctx()));
    out.set(
        "journal",
        hex(&std::fs::read(k.user.join("gb_char.dbin")).expect("gb_char.dbin")),
    );
    k.close();
    out.set("reopened", reads(k));
    out
}

/// `overlapping_modify_before_add`: B's first MODIFY precedes A's in-place
/// corruption and a replacing ADD. The ADD does not read the overreaching item
/// (`phrase_index.cpp:453-459`). B's longer replacement relocates on reopen
/// before A's saved MODIFY, because diff visits B (slot 1) before A (slot 2).
/// No operation reads the malformed intermediate extent (row 107).
pub(super) fn overlapping_modify_before_add(k: &mut Kit) -> J {
    items_system(
        k,
        &[(&[0x4e59], &[0x1081], 50), (&[0x4e19], &[0x1084], 100)],
        150,
        true,
    );
    let mut b = vec![1_u8, 1];
    b.extend(le(50));
    b.extend(le(0x4e59));
    b.extend(le16(0x1081));
    b.extend(le(10));
    let modified_b = [&b[..2], &le(77), &b[6..]].concat();
    let a = [&[1_u8, 3][..], &le(100), &le(0x4e19), &b, &[0, 0]].concat();
    let old_a = [&a[..10], &modified_b, &a[26..]].concat();
    let new_a = [&old_a[..10], &[255, 255], &old_a[12..]].concat();
    let added_b = [&[1_u8, 2][..], &modified_b[2..], &le16(0x2081), &le(10)].concat();
    let body = [&[0_u8; 8][..], &a].concat();
    let body_len = u32::try_from(body.len()).expect("body");
    let head: Vec<u8> = [150, 17, 30, 31 + body_len]
        .iter()
        .flat_map(|n| le(*n))
        .collect();
    let offsets: Vec<u8> = [0_u32, 18, 8].iter().flat_map(|n| le(*n)).collect();
    std::fs::write(
        k.data.join("gb_char.bin"),
        chunk(&[&head[..], b"#", &offsets, b"#", &body, b"#"].concat()),
    )
    .expect("write gb_char.bin");
    let add = [&le(1)[..], &le(1), &le16(added_b.len()), &added_b].concat();
    write_log(
        k,
        &[
            modify_header(0, 150, 150),
            modify(1, &b, &modified_b),
            modify(2, &old_a, &new_a),
            add,
        ]
        .concat(),
        "gb_char.dbin",
    );
    let observe = |k: &mut Kit| {
        let inst = k.inst();
        let token = 0x0100_0001;
        let freq = unigram(inst, token);
        let mut count = UNTOUCHED;
        let prons = facade::token_get_n_pronunciation(inst, token, &raw mut count);
        let parsed = if facade::ZHUYIN {
            facade::parse_more_chewings(inst, c("18 ").as_ptr())
        } else {
            facade::parse_more_full_pinyins(inst, c("ba1").as_ptr())
        };
        let guessed = facade::guess_sentence(inst);
        let mut sentence = seeded();
        let ret = if facade::ZHUYIN {
            facade::get_sentence_only(inst, &raw mut sentence)
        } else {
            facade::get_sentence_at(inst, 0, &raw mut sentence)
        };
        obj! {
            "unigram" => freq,
            "readings" => (prons, count),
            "parse" => parsed,
            "guess" => guessed,
            "sentence" => (ret, owned_out(sentence)),
        }
    };
    let mut out = obj! { "replayed" => observe(k) };
    // Import an unrelated phrase to make save diff the replayed library.
    let it = facade::begin_add_phrases(k.ctx(), 1);
    let reading = if facade::ZHUYIN {
        "ㄐㄧㄚˇ"
    } else {
        "jia3"
    };
    out.set(
        "dirty",
        facade::iterator_add_phrase(it, c("甲").as_ptr(), c(reading).as_ptr(), 1),
    );
    facade::end_add_phrases(it);
    out.set("save", facade::save(k.ctx()));
    out.set(
        "journal",
        hex(&std::fs::read(k.user.join("gb_char.dbin")).expect("gb_char.dbin")),
    );
    k.close();
    out.set("reopened", observe(k));
    out
}

/// `token_get_nth_pronunciation(token, 0)`'s key bytes, empty on failure.
fn nth_key(inst: P, token: u32) -> Vec<u8> {
    // SAFETY: a fresh GArray of two-byte elements, freed below.
    let array = unsafe { glib_sys::g_array_new(0, 0, 2) };
    let ok = facade::token_get_nth_pronunciation(inst, token, 0, array.cast());
    let keys = if ok {
        crate::readers::garray_u16(array)
    } else {
        Vec::new()
    };
    // SAFETY: the array was allocated above and is freed once.
    unsafe { glib_sys::g_array_free(array, 1) };
    keys.iter().flat_map(|key| key.to_le_bytes()).collect()
}

/// `transient_reload_replacement_probe`: a replacement item's bytes and unigram
/// after a NULL-user reload of library 2, then an import over it.
pub(super) fn transient_reload_replacement(k: &mut Kit) -> J {
    let system = private_system(k);
    let ctx = k.init(Some(&system), None);
    assert!(!ctx.is_null(), "transient init failed");
    let inst = facade::alloc_instance(ctx);
    assert!(!inst.is_null());
    let path = system.join("gbk_char.bin");
    let mut payload = std::fs::read(&path).expect("gbk_char.bin")[8..].to_vec();
    let (index, content) = (word(&payload, 4), word(&payload, 8));
    let slots = (content - index - 1) / 4;
    let (slot, token, before_text) = (1..slots)
        .find_map(|slot| {
            let offset = at_offset(word(&payload, at_offset(index + slot * 4)));
            let item = at_offset(content) + offset;
            if offset == 0 || payload[item] != 1 || payload[item + 1] == 0 {
                return None;
            }
            let token = 0x0200_0000 | slot;
            let (mut length, mut text) = (0_u32, std::ptr::null_mut::<c_char>());
            let ret = facade::token_get_phrase(inst, token, &raw mut length, &raw mut text);
            let text = take_text(text).filter(|text| ret && !text.is_empty())?;
            let found = lookup(inst, text.as_bytes());
            (found.len() <= 4
                && found.iter().filter(|t| *t >> 24 == 2).count() == 1
                && found.contains(&token))
            .then_some((slot, token, text))
        })
        .expect("no suitable slot");
    let before_count = unigram(inst, token);
    let before_key = nth_key(inst, token);
    assert!(before_count.0 && before_count.1 < 0xffff_ffef && before_key.len() == 2);
    assert!(facade::unload_phrase_library(ctx, 2));
    let item = at_offset(content + word(&payload, at_offset(index + slot * 4)));
    payload[item + 2..item + 6].copy_from_slice(&le(before_count.1 + 7));
    payload[item + 10] ^= 1;
    std::fs::remove_file(&path).expect("unlink gbk_char.bin");
    std::fs::write(&path, chunk(&payload)).expect("write gbk_char.bin");
    assert!(facade::load_phrase_library(ctx, 2));
    let after_key = nth_key(inst, token);
    let after_count = unigram(inst, token);
    let iterator = facade::begin_add_phrases(ctx, 2);
    let reading = if facade::ZHUYIN { "ㄋㄧˇ" } else { "ni3" };
    let added =
        facade::iterator_add_phrase(iterator, c(&before_text).as_ptr(), c(reading).as_ptr(), 1);
    facade::end_add_phrases(iterator);
    let imported_key = nth_key(inst, token);
    obj! {
        "replacement_key" => after_key != before_key,
        "replacement_unigram" => after_count.0 && after_count.1 == before_count.1 + 7,
        "import" => added,
        "import_replacement_key" => imported_key == after_key,
    }
}

/// `remember_export(k, library, phrase, adjacent=True)`: the export rows of
/// `phrase`, stopping at the first other phrase after its own; the zhuyin
/// facade has no export API.
fn adjacent_export(k: &mut Kit, library: u32, phrase: &str) -> J {
    if facade::ZHUYIN {
        return J::from("NO EXPORT API");
    }
    let it = facade::begin_get_phrases(k.ctx(), library);
    let mut rows = Vec::new();
    while facade::iterator_has_next_phrase(it) {
        let (mut text, mut reading, mut count) =
            (std::ptr::null_mut(), std::ptr::null_mut(), 0_i32);
        let ret =
            facade::iterator_get_next_phrase(it, &raw mut text, &raw mut reading, &raw mut count);
        let (text, reading) = (take_text(text), take_text(reading));
        if text.as_deref() == Some(phrase) {
            rows.push(J::from((ret, text, reading, count)));
        } else if !rows.is_empty() {
            break;
        }
    }
    facade::end_get_phrases(it);
    J::from(rows)
}

/// Library 6's tokens of `phrase`, as hex.
fn library_six(k: &mut Kit, phrase: &str) -> Vec<u32> {
    tokens_of(k, phrase)
        .into_iter()
        .filter(|token| token >> 24 == 6)
        .collect()
}

/// `high_system_library_probe`: a SYSTEM_FILE above 4 through reads, import,
/// training and reopen. The pin dispatches the SYSTEM_FILE branch by the row's
/// file type, not by its library number (`pinyin.cpp:246-276`,
/// `zhuyin.cpp:190-221`). The network row supplies index 6 without changing the
/// existing four system rows; its initial items come from art.bin.
pub(super) fn high_system_library(k: &mut Kit) -> J {
    let hexes = |tokens: &[u32]| {
        J::from(
            tokens
                .iter()
                .map(|token| format!("{token:#x}"))
                .collect::<Vec<_>>(),
        )
    };
    let system = conf_system(
        k,
        &[ConfEdit::Replace(
            "default NETWORK_DICTIONARY NULL NULL network.bin USER_FILE",
            "default NETWORK_DICTIONARY art.table art.bin network.dbin SYSTEM_FILE",
        )],
    );
    k.data = system.clone();
    k.ctx();
    let bytes = std::fs::read(system.join("art.bin")).expect("art.bin");
    let payload = &bytes[8..];
    let (index, content) = (word(payload, 4), word(payload, 8));
    let slot = (1..(content - index - 1) / 4)
        .find(|slot| word(payload, at_offset(index + slot * 4)) != 0)
        .expect("no readable item in high system library");
    let token = 0x0600_0000 | slot;
    let inst = k.inst();
    let mut out = obj! {
        "initial_token" => format!("{token:#x}"),
        "initial_phrase" => token_phrase(inst, token),
        "initial_unigram" => unigram(inst, token),
    };
    let phrase = "龘龘";
    let reading = if facade::ZHUYIN {
        "ㄉㄚˇ ㄉㄚˇ"
    } else {
        "da3'da3"
    };
    out.set("import", import_rows(k, 6, &[(phrase, reading, 9)]));
    let imported = library_six(k, phrase);
    out.set("lookup", hexes(&imported));
    if let Some(&chosen) = imported.first() {
        let inst = k.inst();
        out.set("imported_phrase", token_phrase(inst, chosen));
        out.set("before_training", unigram(inst, chosen));
        let parsed = if facade::ZHUYIN {
            facade::parse_more_chewings(inst, c("283283").as_ptr())
        } else {
            facade::parse_more_full_pinyins(inst, c("da3da3").as_ptr())
        };
        out.set("parsed", parsed);
        let training = super::super::case_helpers::train_selected_top(k, phrase, chosen);
        out.set(
            "training",
            obj! {
                "chosen" => training.chosen,
                "kind" => training.kind,
                "cursor" => training.cursor,
                "after-choose" => training.after_choose,
                "guess" => training.guess,
                "train" => training.train,
            },
        );
        out.set("after_training", unigram(k.inst(), chosen));
        out.set("import_again", import_rows(k, 6, &[(phrase, reading, 1)]));
        out.set("export", adjacent_export(k, 6, phrase));
    }
    out.set("save", facade::save(k.ctx()));
    k.close();
    k.ctx();
    let inst = k.inst();
    out.set("reopened_initial", token_phrase(inst, token));
    let reopened = library_six(k, phrase);
    out.set("reopened_lookup", hexes(&reopened));
    if let Some(&first) = reopened.first() {
        let inst = k.inst();
        out.set("reopened_phrase", token_phrase(inst, first));
        out.set("reopened_unigram", unigram(inst, first));
        out.set("reopened_export", adjacent_export(k, 6, phrase));
    }
    out
}
