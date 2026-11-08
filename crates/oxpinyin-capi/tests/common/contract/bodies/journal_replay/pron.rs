//! #871: the pronunciation counts of a replayed system item. A MODIFY record
//! changes the item the sub-index holds in place (`phrase_index.cpp:477-501`);
//! the item is then a list of (keys, frequency) entries in the file's order,
//! and the pin trains (`phrase_index.cpp:114-143`, every entry in turn), merges
//! an imported reading (`:122-139`) and diffs the list back out at save
//! (`:392-436`) without ever keying the entries by their keys. Two entries with
//! the same keys are two entries. The probe replays one journalled item
//! (癫痫, one reading `dian4 xian4` at 100), then runs the item through the
//! lifecycle a session sees: train it, import a reading, save, open the saved
//! profile, train again, save and open once more. It reports the C ABI's view
//! after each step and the bytes `gb_char.dbin` holds after each save. The
//! export (pinyin only) lists one row per entry, so entries that were merged
//! into one show there.

use super::{
    add_record, export, hex, import_rows, item_text, item_with, modify_record,
    parse_training_input, phrase_text, system_items, token_phrase_of, train_selected_top,
    unigram_j, user_file,
};
use crate::facade;
use crate::fixtures::{modify_header, write_log, write_user_conf};
use crate::support::{J, Kit};

const PRON_TOKEN: u32 = 0x0100_3e57;
const PRON_PHRASE: &str = "癫痫";

/// Another reading's keys (two syllables, as the item's own), taken from a
/// two-reading item of the same library: the pin does not check them.
const PRON_OTHER_KEYS: [u8; 4] = [0x81, 0x02, 0x47, 0x02];

/// One reading entry of a shape: `None` stands for the item's own keys.
type Entry = (Option<&'static [u8]>, u32);

/// `_pron_entries`: `item` with its readings replaced by `entries`, each
/// (keys, freq): the header's count and the unigram stay, the rest is rebuilt.
fn pron_entries(item: &[u8], entries: &[(Vec<u8>, u32)]) -> Vec<u8> {
    let length = usize::from(item[0]);
    let mut out = vec![
        item[0],
        u8::try_from(entries.len()).expect("an entry count"),
    ];
    out.extend(&item[2..6 + 4 * length]);
    for (keys, freq) in entries {
        out.extend(keys);
        out.extend(freq.to_le_bytes());
    }
    out
}

/// `_pron_shape`: the item a MODIFY record leaves, readings replaced and the
/// unigram optionally, entry order and duplicates kept.
fn pron_shape(item: &[u8], entries: &[Entry], unigram: Option<u32>) -> Vec<u8> {
    let length = usize::from(item[0]);
    let start = 6 + 4 * length;
    let keys = &item[start..start + 2 * length];
    let entries: Vec<(Vec<u8>, u32)> = entries
        .iter()
        .map(|(name, freq)| (name.unwrap_or(keys).to_vec(), *freq))
        .collect();
    let built = pron_entries(item, &entries);
    match unigram {
        Some(unigram) => item_with(&built, Some(unigram), None),
        None => built,
    }
}

const OTHER: Option<&[u8]> = Some(&PRON_OTHER_KEYS);

/// Each shape is the item the MODIFY record leaves: (keys, frequency) entries,
/// `None` standing for the item's own keys. The first two leave one reading;
/// the dup- shapes carry the item's keys twice; `other` adds a second reading.
fn shape(name: &str) -> (Vec<Entry>, Option<u32>) {
    match name {
        "raised" => (vec![(None, 250)], None),
        "unigram-only" => (vec![(None, 100)], Some(500)),
        "lowered" => (vec![(None, 40)], None),
        "dup-raised-last" => (vec![(None, 100), (None, 160)], None),
        "dup-raised-first" => (vec![(None, 160), (None, 100)], None),
        "dup-both" => (vec![(None, 160), (None, 220)], None),
        "dup-equal" => (vec![(None, 100), (None, 100)], Some(500)),
        "other-reading" => (vec![(None, 100), (OTHER, 130)], None),
        "other-first" => (vec![(OTHER, 130), (None, 250)], None),
        other => panic!("no pronunciation shape {other}"),
    }
}

/// Runs `replay-pron-<name>`. Training (`phrase_index.cpp:114-143`) and import
/// (`:122-139`) read the entries the shape leaves, so they take every shape on
/// libpinyin. Masking reads none of them, and libzhuyin calls the same code:
/// one shape each.
pub(super) fn case(k: &mut Kit, name: &str) -> J {
    if name == "mixed" {
        return pron_mixed(k);
    }
    if name == "unigram-combined" {
        return pron_lifecycle(k, &[(None, 160), (None, 220)], None, "unigram");
    }
    let (steps, shape_name) = name.split_once('-').expect("steps and a shape");
    let (entries, unigram) = shape(shape_name);
    pron_lifecycle(k, &entries, unigram, steps)
}

/// The facade's readings of 癫痫 for an import.
fn imported_reading(tone: u8) -> &'static str {
    match (facade::ZHUYIN, tone) {
        (false, 3) => "dian3'xian4",
        (false, _) => "dian4'xian4",
        (true, 3) => "ㄉㄧㄢˇ ㄒㄧㄢˋ",
        (true, _) => "ㄉㄧㄢˋ ㄒㄧㄢˋ",
    }
}

/// Asserts the complete parse of 癫痫's facade input and trains its selected token.
fn train(k: &mut Kit) -> J {
    let parsed = parse_training_input(k);
    let typed = if facade::ZHUYIN { 8 } else { 10 };
    assert_eq!(parsed, typed, "the whole training input must parse");
    train_selected_top(k, PRON_PHRASE, PRON_TOKEN)
}

/// `steps` is `train`, `import`, `mask` or `unigram`: the lifecycle that
/// follows the replay. `train` trains the item and imports a reading before
/// the first save; `import` only imports (twice, the second time onto the
/// reading the first one added, after a reopen); `mask` masks the library the
/// journal changed before the import. Every variant saves and opens the saved
/// profile at least once.
fn pron_lifecycle(k: &mut Kit, entries: &[Entry], unigram: Option<u32>, steps: &str) -> J {
    let library = system_items(&k.data, "gb_char.bin");
    let token = PRON_TOKEN;
    let item = library.items[&(token & 0xFF_FFFF)].clone();
    assert!(
        item_text(&item).as_deref() == Some(PRON_PHRASE) && item[1] == 1,
        "{}",
        hex(&item)
    );
    write_user_conf(k);
    let log = [
        modify_header(0, library.total, library.total),
        modify_record(token, &item, &pron_shape(&item, entries, unigram)),
    ]
    .concat();
    write_log(k, &log, "gb_char.dbin");
    let mut out = obj! { "ctx" => !k.ctx().is_null() };
    let look = |k: &mut Kit, out: &mut J, tag: &str| {
        let observed = obj! {
            "export" => export(k, 1, Some(PRON_PHRASE), true),
            "unigram" => unigram_j(k, token),
            "phrase" => token_phrase_of(k, token),
        };
        out.set(tag, observed);
    };
    let save = |k: &mut Kit, out: &mut J, tag: &str| {
        out.set(format!("{tag}-save"), facade::save(k.ctx()));
        out.set(format!("{tag}-dbin"), hex(&user_file(k, "gb_char.dbin")));
    };
    let reopen = |k: &mut Kit, out: &mut J, tag: &str| {
        k.close();
        look(k, out, tag);
    };
    look(k, &mut out, "opened");
    let imported = imported_reading(3);
    match steps {
        "unigram" => {
            let add =
                |k: &mut Kit, delta| facade::token_add_unigram_frequency(k.inst(), token, delta);
            out.set("added-unigram", add(k, 7));
            look(k, &mut out, "after-unigram");
            out.set("trained", train(k));
            look(k, &mut out, "after-train");
            save(k, &mut out, "first");
            reopen(k, &mut out, "reopened");
            out.set("added-again", add(k, 5));
            look(k, &mut out, "after-unigram-again");
            // A unigram increment does not set m_modified. Dirty an unrelated
            // user library before the second save, preserving both live readings.
            let reading = if facade::ZHUYIN {
                "ㄋㄧˇ ㄏㄠˇ"
            } else {
                "ni3'hao3"
            };
            out.set("dirty", import_rows(k, 7, &[("你好", reading, 1)]));
            save(k, &mut out, "second");
            reopen(k, &mut out, "reopened-again");
        }
        "train" => {
            out.set("trained", train(k));
            look(k, &mut out, "after-train");
            out.set(
                "imported",
                import_rows(k, 1, &[(PRON_PHRASE, imported, 50)]),
            );
            look(k, &mut out, "after-import");
            save(k, &mut out, "first");
            reopen(k, &mut out, "reopened");
            out.set("trained-again", train(k));
            look(k, &mut out, "after-train-again");
            save(k, &mut out, "second");
            reopen(k, &mut out, "reopened-again");
        }
        "import" => {
            out.set(
                "imported",
                import_rows(k, 1, &[(PRON_PHRASE, imported, 50)]),
            );
            look(k, &mut out, "after-import");
            save(k, &mut out, "first");
            reopen(k, &mut out, "reopened");
            let rows = [
                (PRON_PHRASE, imported, 7),
                (PRON_PHRASE, imported_reading(4), 9),
            ];
            out.set("imported-again", import_rows(k, 1, &rows));
            look(k, &mut out, "after-import-again");
            save(k, &mut out, "second");
            reopen(k, &mut out, "reopened-again");
        }
        "mask" => {
            // Library 1 as a whole: its journal is skipped when the libraries
            // are merged again, so the item is the shipped one.
            out.set("mask", facade::mask_out(k.ctx(), 0xFF00_0000, 0x0100_0000));
            look(k, &mut out, "after-mask");
            out.set(
                "imported",
                import_rows(k, 1, &[(PRON_PHRASE, imported, 50)]),
            );
            look(k, &mut out, "after-import");
            save(k, &mut out, "first");
            reopen(k, &mut out, "reopened");
        }
        other => panic!("no pronunciation lifecycle {other}"),
    }
    out
}

/// The same lifecycle over a profile with all of it at once: the journal
/// changes 癫痫's reading and unigram and appends a fresh item to the library
/// (its reading is seeded, the shipped item's is not), a user phrase is
/// imported into library 7, and 癫痫 is trained, which also trains a gram.
/// Each save is followed by a reopen; the second round trains again.
fn pron_mixed(k: &mut Kit) -> J {
    let library = system_items(&k.data, "gb_char.bin");
    let token = PRON_TOKEN;
    let item = library.items[&(token & 0xFF_FFFF)].clone();
    let one = library.first(|item| item[0] == 1 && item[1] == 1);
    let fresh = 0x0100_0000 | (library.max_slot() + 2);
    let appended = item_with(&library.items[&one], Some(11), None);
    let changed = item_with(&item, Some(u32::from(item[2]) + 400), Some(250));
    write_user_conf(k);
    let log = [
        modify_header(0, library.total, library.total + 400 + 11),
        modify_record(token, &item, &changed),
        add_record(fresh, &appended),
    ]
    .concat();
    write_log(k, &log, "gb_char.dbin");
    let mut out = obj! { "ctx" => !k.ctx().is_null() };
    let user_row = (
        "龘龘",
        if facade::ZHUYIN {
            "ㄉㄚˊ ㄉㄚˊ"
        } else {
            "da2'da2"
        },
        20,
    );
    let look = |k: &mut Kit, out: &mut J, tag: &str| {
        let observed = obj! {
            "export" => export(k, 1, Some(PRON_PHRASE), true),
            "user-export" => export(k, 7, Some(user_row.0), true),
            "unigram" => unigram_j(k, token),
            "fresh" => unigram_j(k, fresh),
            "fresh-phrase" => token_phrase_of(k, fresh),
            // The appended item shares its text with slot `one`: walk every row.
            "fresh-export" => {
                let text = phrase_text(&token_phrase_of(k, fresh));
                export(k, 1, text.as_deref(), false)
            },
        };
        out.set(tag, observed);
    };
    let save = |k: &mut Kit, out: &mut J, tag: &str| {
        out.set(format!("{tag}-save"), facade::save(k.ctx()));
        for name in ["gb_char.dbin", "user.bin"] {
            out.set(format!("{tag}-{name}"), hex(&user_file(k, name)));
        }
    };
    let reopen = |k: &mut Kit, out: &mut J, tag: &str| {
        k.close();
        look(k, out, tag);
    };
    look(k, &mut out, "opened");
    out.set("imported-user", import_rows(k, 7, &[user_row]));
    out.set("trained", train(k));
    look(k, &mut out, "after-train");
    out.set(
        "imported",
        import_rows(k, 1, &[(PRON_PHRASE, imported_reading(3), 50)]),
    );
    look(k, &mut out, "after-import");
    save(k, &mut out, "first");
    reopen(k, &mut out, "reopened");
    out.set("trained-again", train(k));
    look(k, &mut out, "after-train-again");
    save(k, &mut out, "second");
    reopen(k, &mut out, "reopened-again");
    out
}
