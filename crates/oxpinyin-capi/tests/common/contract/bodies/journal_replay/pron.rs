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
    export, hex, import_rows, item_text, item_with, modify_record, parse_training_input,
    system_items, token_phrase_of, train_selected_top, unigram_j, user_file,
};
use crate::facade;
use crate::fixtures::{modify_header, write_log, write_user_conf};
use crate::support::{J, Kit};

const PRON_TOKEN: u32 = 0x0100_3e57;
const PRON_PHRASE: &str = "癫痫";

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

/// Each shape is the item the MODIFY record leaves: (keys, frequency) entries,
/// `None` standing for the item's own keys. The first two leave one reading;
/// the dup- shapes carry the item's keys twice; `other` adds a second reading.
fn shape(name: &str) -> (Vec<Entry>, Option<u32>) {
    match name {
        "dup-equal" => (vec![(None, 100), (None, 100)], Some(500)),
        "dup-both" => (vec![(None, 160), (None, 220)], None),
        other => panic!("no pronunciation shape {other}"),
    }
}

/// Runs `replay-pron-<name>`. Training (`phrase_index.cpp:114-143`) and import
/// (`:122-139`) read the entries the shape leaves, so they take every shape on
/// libpinyin. Masking reads none of them, and libzhuyin calls the same code:
/// one shape each.
pub(super) fn case(k: &mut Kit, name: &str) -> J {
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
        other => panic!("no pronunciation lifecycle {other}"),
    }
    out
}
