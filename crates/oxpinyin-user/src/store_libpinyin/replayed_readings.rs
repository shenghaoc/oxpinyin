//! The readings of a replayed system item (#871).
//!
//! A `.dbin` MODIFY record changes the item the sub-index holds in place
//! (`phrase_index.cpp:477-501` at 074a2219). The session keeps that item whole
//! in its live system-item view, [`seed_txn`] writes no `PRONUNCIATION` row for
//! it, and the readings' counts are read from the item: training starts from
//! them, writes the row for the readings it moves, and the save writes them.
//! These tests hold what that must not change. The item is a list of (keys,
//! count) entries in the file's order, as the pin keeps it: an import adds to
//! the first entry with the keys (`PhraseItem::add_pronunciation`,
//! `phrase_index.cpp:56-91`), training to every matching entry on its own count
//! (`increase_pronunciation_possibility`, `:114-143`), and neither keys the
//! list by them, so two entries with the same keys stay two entries.

use std::collections::BTreeMap;
use std::path::PathBuf;

use oxpinyin_core::{ChewingKey, PhraseToken, UserModel};
use oxpinyin_data::chunk_write::{ChunkItem, encode_phrase_item};
use oxpinyin_data::user_files::{LogRecord, SystemVersions};

use super::{export_state, packed_to_pinyin_keys};
use crate::persistence::{SystemLibrary, SystemOriginals, UserConfLaw};
use crate::store::UserStore;

const LIBRARY: u32 = 0x0100_0000;

fn versions() -> SystemVersions {
    SystemVersions::for_this_build(7, 14)
}

fn tempdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "oxpinyin-replayed-readings-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmpdir");
    dir
}

fn tone(syllable: &str, tone: u8) -> Vec<u16> {
    vec![
        ChewingKey::from_pinyin(syllable)
            .expect("syllable")
            .with_tone(tone)
            .to_packed(),
    ]
}

fn key(syllable: &str, tone_: u8) -> ChewingKey {
    ChewingKey::from_pinyin(syllable)
        .expect("syllable")
        .with_tone(tone_)
}

fn item(phrase: u32, unigram: u32, prons: &[(Vec<u16>, u32)]) -> ChunkItem {
    ChunkItem {
        phrase: vec![phrase],
        unigram,
        prons: prons.to_vec(),
    }
}

/// Library 1 holding `items` by slot, its total their unigrams' sum.
fn library_of(items: Vec<(u32, ChunkItem)>) -> SystemOriginals {
    let total = items.iter().map(|(_, item)| item.unigram).sum();
    let range_end = items
        .iter()
        .map(|(slot, _)| *slot)
        .max()
        .map_or(0, |last| last + 1);
    BTreeMap::from([(
        1_u8,
        SystemLibrary {
            mapped: None,
            total,
            range_end,
            items: items.into_iter().collect(),
        },
    )])
    .into()
}

fn header(old_total: u32, new_total: u32) -> LogRecord {
    LogRecord::ModifyHeader {
        old_total,
        new_total,
    }
}

fn modify(slot: u32, old: &ChunkItem, new: &ChunkItem) -> LogRecord {
    LogRecord::Modify {
        token: slot,
        old_item: encode_phrase_item(old).expect("old item"),
        new_item: encode_phrase_item(new).expect("new item"),
    }
}

fn raw_modify(slot: u32, old: &[u8], new: &[u8]) -> LogRecord {
    LogRecord::Modify {
        token: slot,
        old_item: old.to_vec(),
        new_item: new.to_vec(),
    }
}

fn open(dir: &std::path::Path, base: &SystemOriginals) -> UserStore {
    UserStore::open_libpinyin(dir, base.clone(), versions(), UserConfLaw::Pinyin).expect("open")
}

/// A profile whose `gb_char.dbin` holds `records` over `base`.
fn open_over(name: &str, base: &SystemOriginals, records: &[LogRecord]) -> (PathBuf, UserStore) {
    let dir = tempdir(name);
    let payload = oxpinyin_data::user_files::encode_log_records(records).expect("log");
    std::fs::write(
        dir.join("gb_char.dbin"),
        oxpinyin_data::chunk_format::build_memory_chunk(&payload).expect("chunk"),
    )
    .expect("write log");
    std::fs::write(
        dir.join("user.conf"),
        oxpinyin_data::user_files::UserTableInfo::conform_to(&versions()).to_text(),
    )
    .expect("write marker");
    let store = open(&dir, base);
    (dir, store)
}

#[test]
fn per_slot_seeding_keeps_mixed_restored_added_removed_and_header_counts() {
    let one = item(0x4f60, 100, &[(tone("ni", 3), 10)]);
    let two = item(0x597d, 50, &[(tone("hao", 3), 20)]);
    let three = item(0x4e09, 25, &[(tone("san", 1), 30)]);
    let four = item(0x56db, 40, &[(tone("si", 4), 40)]);
    let added = item(0x4e94, 11, &[(tone("wu", 3), 50)]);
    let base = library_of(vec![
        (1, one.clone()),
        (2, two.clone()),
        (3, three.clone()),
        (4, four.clone()),
    ]);
    let mut reading_only = one.clone();
    reading_only.prons[0].1 = 77;
    let mut changed = two.clone();
    changed.unigram = 60;
    let mut changed_again = changed.clone();
    changed_again.unigram = 70;
    let mut temporary = three.clone();
    temporary.unigram = 35;
    let (dir, store) = open_over(
        "per-slot-unigrams",
        &base,
        &[
            header(215, 1_000),
            modify(1, &one, &reading_only),
            modify(2, &two, &changed),
            modify(2, &changed, &changed_again),
            modify(3, &three, &temporary),
            modify(3, &temporary, &three),
            LogRecord::Add {
                token: 5,
                new_item: encode_phrase_item(&added).expect("added"),
            },
            LogRecord::Remove {
                token: 4,
                old_item: encode_phrase_item(&four).expect("removed"),
            },
        ],
    );
    assert_eq!(store.unigram_delta(LIBRARY | 1).expect("unchanged"), 0);
    assert_eq!(store.unigram_delta(LIBRARY | 2).expect("changed"), 20);
    assert_eq!(store.unigram_delta(LIBRARY | 3).expect("restored"), 0);
    assert_eq!(store.unigram_delta(LIBRARY | 5).expect("added"), 11);
    assert_eq!(store.unigram_library_total(1).expect("seeded deltas"), 31);
    assert_eq!(
        base[&1]
            .total
            .wrapping_add(store.unigram_library_total(1).expect("deltas"))
            .wrapping_add(store.library_offset(1).expect("header offset")),
        971
    );
    assert_eq!(counts(&store, 1), [77]);
    assert!(
        store
            .with_effective_system_item(LIBRARY | 4, |_| ())
            .is_none()
    );
    drop(store);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// The live counts of `slot`'s readings, in the item's order.
fn counts(store: &UserStore, slot: u32) -> Vec<u32> {
    store
        .with_effective_system_item(LIBRARY | slot, |item| {
            item.prons.iter().map(|(_, count)| *count).collect()
        })
        .expect("a live item")
}

/// The counts a save would write for `slot`'s readings.
fn exported(store: &UserStore, base: &SystemOriginals, slot: u32) -> Vec<u32> {
    export_state(store, base).expect("export").system_overrides[&1][&slot]
        .as_ref()
        .expect("item")
        .prons
        .iter()
        .map(|(_, count)| *count)
        .collect()
}

fn pronunciation_rows(store: &UserStore) -> usize {
    let mut rows = 0_usize;
    let db = store.database();
    oxpinyin_store::ReadStore::for_each(&*db, crate::store::PRONUNCIATION, |_, _| {
        rows += 1;
        Ok(())
    })
    .expect("rows");
    rows
}

fn train(store: &mut UserStore, slot: u32, reading: ChewingKey, paths: usize) {
    let mut readings = (0..paths).map(|_| vec![reading]);
    UserModel::observe_with_keys(store, &[], &PhraseToken::new(LIBRARY | slot), &mut readings)
        .expect("train");
}

fn dense(syllable: &str, tone_: u8) -> Vec<crate::phrase::PinyinKey> {
    packed_to_pinyin_keys(&tone(syllable, tone_)).expect("dense")
}

/// Opening writes no reading row for a replayed item, keeps the unigram delta
/// the count reads consult, and training, save and reopen carry the counts as
/// the live item holds them.
#[test]
fn a_replayed_item_opens_without_reading_rows_and_trains_from_its_counts() {
    let (ni3, ni2) = (tone("ni", 3), tone("ni", 2));
    let old = item(0x4f60, 100, &[(ni3.clone(), 100), (ni2.clone(), 40)]);
    let new = item(0x4f60, 150, &[(ni3, 107), (ni2, 40)]);
    let base = library_of(vec![(1, old.clone())]);
    let (dir, mut store) = open_over(
        "opens-without-rows",
        &base,
        &[header(100, 150), modify(1, &old, &new)],
    );
    assert_eq!(pronunciation_rows(&store), 0);
    assert_eq!(counts(&store, 1), [107, 40]);
    assert_eq!(exported(&store, &base, 1), [107, 40]);
    // The unigram is still a stored delta: pricing and the totals read it.
    assert_eq!(store.unigram_delta(LIBRARY | 1).expect("delta"), 50);
    assert_eq!(store.unigram_total().expect("total"), 50);

    // One accepted path adds the seed to the matching live reading alone.
    // Session training no longer writes pronunciation rows.
    train(&mut store, 1, key("ni", 3), 1);
    assert_eq!(pronunciation_rows(&store), 0);
    assert_eq!(counts(&store, 1), [176, 40]);
    assert_eq!(exported(&store, &base, 1), [176, 40]);
    assert!(store.save().expect("save"));
    drop(store);

    // The saved profile replays to the same item, again with no row; a second
    // training starts from the replayed count and not from the original.
    let mut store = open(&dir, &base);
    assert_eq!(pronunciation_rows(&store), 0);
    assert_eq!(counts(&store, 1), [176, 40]);
    train(&mut store, 1, key("ni", 3), 1);
    // The seed of a second training is twice the first: the predecessor's
    // gram is no longer new (`seed.rs`).
    assert_eq!(counts(&store, 1), [314, 40]);
    assert_eq!(exported(&store, &base, 1), [314, 40]);
    assert!(store.save().expect("save again"));
    drop(store);
    let store = open(&dir, &base);
    assert_eq!(counts(&store, 1), [314, 40]);
    drop(store);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// A write through an over-length item changes slot 2 without a record of
/// its own. The replayed view must expose slot 2, seed its unigram delta and
/// carry it into the save, as the pin's save diff does.
#[test]
fn an_unrecorded_overlapped_slot_is_visible_seeded_and_saved() {
    let one = item(0x4f60, 100, &[(tone("ni", 3), 100)]);
    let two = item(0x597d, 50, &[(tone("hao", 3), 30), (tone("hao", 4), 20)]);
    let one_bytes = encode_phrase_item(&one).expect("one");
    let two_bytes = encode_phrase_item(&two).expect("two");
    let base = library_of(vec![(1, one), (2, two)]);
    // The six-byte payload claims two readings (30 bytes): slot 1 borrows
    // slot 2's first 14 bytes, and the second write changes slot 2's
    // unigram through them. Slot 2 has no record of its own.
    let expanded = {
        let mut bytes = vec![2, 2];
        bytes.extend_from_slice(&200_u32.to_le_bytes());
        bytes
    };
    let old = [
        expanded.clone(),
        one_bytes[6..].to_vec(),
        two_bytes[..14].to_vec(),
    ]
    .concat();
    let mut new = old.clone();
    let at = one_bytes.len() + 2;
    new[at..at + 4].copy_from_slice(&170_u32.to_le_bytes());
    let (dir, mut store) = open_over(
        "overlap-unrecorded",
        &base,
        &[
            header(150, 150),
            raw_modify(1, &one_bytes, &expanded),
            raw_modify(1, &old, &new),
        ],
    );
    // Visible and seeded: the item is the replayed bytes, not the original.
    assert_eq!(
        store.with_effective_system_item(LIBRARY | 2, |item| item.unigram),
        Some(170)
    );
    assert_eq!(store.unigram_delta(LIBRARY | 1).expect("slot 1 delta"), 100);
    assert_eq!(store.unigram_delta(LIBRARY | 2).expect("slot 2 delta"), 120);
    assert_eq!(counts(&store, 2), [30, 20]);
    // The over-length item's raw words are the borrowed bytes.
    let rows = store
        .item_pronunciations(LIBRARY | 1)
        .expect("rows")
        .expect("over-length item");
    let keys: Vec<Vec<u16>> = rows
        .iter()
        .map(|(keys, _)| keys.iter().map(|key| key.to_packed()).collect())
        .collect();
    assert_eq!(keys[0], [0x0000, 0x0201]);
    assert_eq!(keys[1], [0x597d, 0x0000]);
    // The save's override map carries slot 2 and the save writes it.
    let saved = export_state(&store, &base).expect("export");
    assert_eq!(
        saved.system_overrides[&1][&2]
            .as_ref()
            .expect("saved slot 2")
            .unigram,
        170
    );
    store.mark_modified();
    assert!(store.save().expect("save"));
    drop(store);
    let store = open(&dir, &base);
    assert_eq!(
        store.with_effective_system_item(LIBRARY | 2, |item| item.unigram),
        Some(170)
    );
    assert_eq!(counts(&store, 2), [30, 20]);
    drop(store);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// Two entries with the same keys are two entries in every view, whether the
/// journal made them or the shipped item already had them: no reading is read
/// back through a map keyed by its keys.
#[test]
fn entries_with_the_same_keys_stay_separate_through_import_save_and_reopen() {
    let ni3 = tone("ni", 3);
    let hao3 = tone("hao", 3);
    let one = item(0x4f60, 100, &[(ni3.clone(), 100)]);
    let two = item(0x597d, 50, &[(hao3.clone(), 30), (hao3.clone(), 40)]);
    // The journal grows the first item by a second entry with its keys, and
    // changes only the unigram of the second, which already has two.
    let one_new = item(0x4f60, 100, &[(ni3.clone(), 100), (ni3, 160)]);
    let two_new = item(0x597d, 77, &[(hao3.clone(), 30), (hao3, 40)]);
    let base = library_of(vec![(1, one.clone()), (2, two.clone())]);
    let (dir, mut store) = open_over(
        "same-keys",
        &base,
        &[
            header(150, 177),
            modify(1, &one, &one_new),
            modify(2, &two, &two_new),
        ],
    );
    assert_eq!(pronunciation_rows(&store), 0);
    assert_eq!(counts(&store, 1), [100, 160]);
    assert_eq!(exported(&store, &base, 1), [100, 160]);
    assert_eq!(counts(&store, 2), [30, 40]);
    assert_eq!(exported(&store, &base, 2), [30, 40]);

    // An imported reading adds to the first entry with its keys
    // (`phrase_index.cpp:122-139`) and to nothing else.
    let token = |slot: u32| LIBRARY | slot;
    store
        .add_system_phrase_in(1, Some(token(1)), &"你".into(), &dense("ni", 3), Some(7))
        .expect("import onto the grown item");
    store
        .add_system_phrase_in(1, Some(token(2)), &"好".into(), &dense("hao", 3), Some(9))
        .expect("import onto the shipped duplicate");
    assert_eq!(counts(&store, 1), [107, 160]);
    assert_eq!(exported(&store, &base, 1), [107, 160]);
    assert_eq!(counts(&store, 2), [39, 40]);
    assert_eq!(exported(&store, &base, 2), [39, 40]);
    store.mark_modified();
    assert!(store.save().expect("save"));
    drop(store);

    let store = open(&dir, &base);
    assert_eq!(counts(&store, 1), [107, 160]);
    assert_eq!(exported(&store, &base, 1), [107, 160]);
    assert_eq!(counts(&store, 2), [39, 40]);
    assert_eq!(exported(&store, &base, 2), [39, 40]);
    drop(store);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// A mask that drops a library's journal takes the counts the journal gave
/// with it: the item is the shipped one again, and nothing of the replay is
/// read back when it is next changed.
#[test]
fn a_mask_leaves_nothing_of_the_replayed_counts() {
    let (ni3, ni2) = (tone("ni", 3), tone("ni", 2));
    let old = item(0x4f60, 100, &[(ni3.clone(), 100), (ni2.clone(), 40)]);
    let new = item(0x4f60, 100, &[(ni3, 100), (ni2, 45)]);
    let base = library_of(vec![(1, old.clone())]);
    let (dir, mut store) = open_over(
        "mask-leaves-nothing",
        &base,
        &[header(100, 100), modify(1, &old, &new)],
    );
    assert_eq!(counts(&store, 1), [100, 45]);
    store
        .mask_out(0xff00_0000, LIBRARY)
        .expect("mask library 1");
    assert!(
        store
            .with_effective_system_item(LIBRARY | 1, |_| ())
            .is_none()
    );
    store
        .add_system_phrase_in(1, Some(LIBRARY | 1), &"你".into(), &dense("ni", 3), Some(4))
        .expect("import");
    assert_eq!(counts(&store, 1), [104, 40]);
    assert_eq!(exported(&store, &base, 1), [104, 40]);
    store.mark_modified();
    assert!(store.save().expect("save"));
    drop(store);
    let store = open(&dir, &base);
    assert_eq!(counts(&store, 1), [104, 40]);
    drop(store);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// The replayed rows the open no longer writes are not what the other seed
/// categories stand on: a populated user library and a user pinyin index that
/// outlives a removed phrase are read and saved as they are without the
/// journal, beside a replayed item.
#[test]
fn a_populated_user_library_and_an_independent_index_beside_a_replayed_item() {
    let (ni3, ni2) = (tone("ni", 3), tone("ni", 2));
    let old = item(0x4f60, 100, &[(ni3.clone(), 100)]);
    let new = item(0x4f60, 100, &[(ni3, 130)]);
    let base = library_of(vec![(1, old.clone())]);
    // What the save writes to the user's own files: the user libraries'
    // items, the pinyin index entries and the raw phrase table, which the
    // replayed item has no part in.
    let user_view = |store: &UserStore| {
        let state = export_state(store, &base).expect("export");
        let libraries: BTreeMap<_, _> = state
            .libraries
            .iter()
            .filter(|(nibble, _)| **nibble >= 5)
            .collect();
        format!("{libraries:?} {:?} {:?}", state.indexed, state.phrase_table)
    };
    let user_session = |store: &mut UserStore| {
        let kept = store
            .add_phrase_in(
                crate::USER_DICTIONARY,
                &"龘".into(),
                &dense("ni", 2),
                Some(20),
            )
            .expect("kept");
        let removed = store
            .add_phrase_in(
                crate::USER_DICTIONARY,
                &"鱻".into(),
                &dense("hao", 4),
                Some(9),
            )
            .expect("removed");
        assert!(store.remove_user_phrase(removed).expect("remove"));
        store.mark_modified();
        assert!(store.save().expect("save"));
        kept
    };
    // The same user session over a profile with the journal and over one
    // without: the user files it writes are the same bytes.
    let (dir, mut store) = open_over(
        "user-library",
        &base,
        &[header(100, 100), modify(1, &old, &new)],
    );
    let kept = user_session(&mut store);
    let view = user_view(&store);
    drop(store);
    let control = tempdir("user-library-control");
    std::fs::write(
        control.join("user.conf"),
        oxpinyin_data::user_files::UserTableInfo::conform_to(&versions()).to_text(),
    )
    .expect("marker");
    let mut plain = open(&control, &base);
    user_session(&mut plain);
    assert_eq!(view, user_view(&plain));
    drop(plain);

    let mut store = open(&dir, &base);
    // Both the user arena and the replayed item own their reading counts.
    assert_eq!(pronunciation_rows(&store), 0);
    assert_eq!(counts(&store, 1), [130]);
    assert_eq!(view, user_view(&store));
    let phrases = store
        .export_phrases_in(crate::USER_DICTIONARY)
        .expect("export");
    assert_eq!(phrases.len(), 1);
    assert_eq!(
        (phrases[0].text.as_slice(), phrases[0].count),
        (&[0x9f98][..], 20)
    );
    train(&mut store, 1, key("ni", 3), 1);
    assert_eq!(counts(&store, 1), [199]);
    assert!(store.save().expect("save again"));
    drop(store);

    let store = open(&dir, &base);
    assert_eq!(counts(&store, 1), [199]);
    assert_eq!(view, user_view(&store));
    assert_eq!(
        store
            .token_for_phrase_in(crate::USER_DICTIONARY, "龘")
            .expect("lookup"),
        Some(kept)
    );
    drop(store);
    let _ = ni2;
    std::fs::remove_dir_all(dir).expect("cleanup");
    std::fs::remove_dir_all(control).expect("cleanup");
}

/// Training gives every entry that matches its own count and the save writes
/// those counts, whether the journal left one entry for a reading, two with
/// the same keys, or lowered a count below the original's.
#[test]
fn training_moves_each_matching_entry_on_its_own_count() {
    let ni3 = tone("ni", 3);
    let old = item(0x4f60, 100, &[(ni3.clone(), 100)]);
    for (name, entries, first, second) in [
        // (after one path, after one more path in the reopened profile)
        (
            "same-keys",
            vec![(ni3.clone(), 100), (ni3.clone(), 160)],
            vec![169, 229],
            vec![307, 367],
        ),
        (
            "same-keys-reversed",
            vec![(ni3.clone(), 160), (ni3.clone(), 100)],
            vec![229, 169],
            vec![367, 307],
        ),
        // The count is a `guint32`: 40 + 69, then 109 + 138.
        ("lowered", vec![(ni3.clone(), 40)], vec![109], vec![247]),
    ] {
        let new = item(0x4f60, 100, &entries);
        let base = library_of(vec![(1, old.clone())]);
        let (dir, mut store) = open_over(
            &format!("train-{name}"),
            &base,
            &[header(100, 100), modify(1, &old, &new)],
        );
        train(&mut store, 1, key("ni", 3), 1);
        assert_eq!(counts(&store, 1), first, "{name}: live");
        assert_eq!(exported(&store, &base, 1), first, "{name}: saved");
        assert!(store.save().expect("save"));
        drop(store);
        let mut store = open(&dir, &base);
        assert_eq!(counts(&store, 1), first, "{name}: reopened");
        train(&mut store, 1, key("ni", 3), 1);
        assert_eq!(counts(&store, 1), second, "{name}: live again");
        assert_eq!(exported(&store, &base, 1), second, "{name}: saved again");
        assert!(store.save().expect("save again"));
        drop(store);
        let store = open(&dir, &base);
        assert_eq!(counts(&store, 1), second, "{name}: reopened again");
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }
}

/// What a session answers for a mixed profile, in the order a caller asks:
/// the readings and the unigram delta of each system token, the count reads,
/// the grams, the user library and what a save would write.
fn mixed_view(store: &UserStore, base: &SystemOriginals, tokens: &[u32]) -> String {
    let mut out = String::new();
    for &token in tokens {
        // The facade total a count read adds differs between a live session
        // and a reopened one (what the session added and removed since it
        // opened against what the saved libraries sum to), on `main` as well:
        // it is not part of this view.
        let delta = store.count_delta(Some(LIBRARY | 1), token).expect("delta");
        out.push_str(&format!(
            "{token:#x} {:?} unigram {} gram {} of {}\n",
            store.with_effective_system_item(token, |item| item.prons.clone()),
            delta.unigram_delta,
            delta.bigram_count,
            delta.bigram_total,
        ));
    }
    out.push_str(&format!(
        "gram {:?} lib1 {:?}\n",
        store.bigram_row(LIBRARY | 1).ok(),
        store.unigram_library_total(1).ok(),
    ));
    let state = export_state(store, base).expect("export");
    out.push_str(&format!(
        "{:?} {:?} {:?}\n",
        state.system_overrides, state.libraries, state.indexed
    ));
    out
}

/// A profile with all of it at once: a shipped item whose reading and
/// unigram the journal changed, a shipped item whose unigram alone changed, an
/// item the journal appended to the library (its readings are rows, as a user
/// phrase's are), a populated user library, grams, and a pinyin index entry
/// that outlives a removed phrase. The shipped and appended items are not
/// alike: the shipped item's rows are the training's, the appended item's are
/// seeded and absolute.
#[test]
fn a_mixed_profile_keeps_shipped_appended_and_user_state_apart() {
    let (ni3, hao3, ta1) = (tone("ni", 3), tone("hao", 3), tone("ta", 1));
    let one = item(0x4f60, 100, &[(ni3.clone(), 100)]);
    let two = item(0x597d, 50, &[(hao3.clone(), 50)]);
    let appended = item(0x4ed6, 33, &[(ta1.clone(), 33)]);
    let one_new = item(0x4f60, 120, &[(ni3, 107)]);
    let two_new = item(0x597d, 77, &[(hao3, 50)]);
    let base = library_of(vec![(1, one.clone()), (2, two.clone())]);
    let (dir, mut store) = open_over(
        "mixed",
        &base,
        &[
            header(150, 230),
            modify(1, &one, &one_new),
            modify(2, &two, &two_new),
            LogRecord::Add {
                token: 3,
                new_item: encode_phrase_item(&appended).expect("added item"),
            },
        ],
    );
    let tokens = [LIBRARY | 1, LIBRARY | 2, LIBRARY | 3];
    // The appended item has one arena body; no session pronunciation rows.
    assert_eq!(pronunciation_rows(&store), 0);
    assert_eq!(counts(&store, 1), [107]);
    assert_eq!(counts(&store, 2), [50]);
    assert_eq!(counts(&store, 3), [33]);
    assert_eq!(store.unigram_delta(LIBRARY | 1).expect("delta"), 20);
    assert_eq!(store.unigram_delta(LIBRARY | 2).expect("delta"), 27);
    assert_eq!(store.unigram_delta(LIBRARY | 3).expect("delta"), 33);

    // A user library with a removed phrase, grams over the three items, and
    // training and imports onto the shipped and the appended item.
    let kept = store
        .add_phrase_in(
            crate::USER_DICTIONARY,
            &"龘".into(),
            &dense("ni", 2),
            Some(20),
        )
        .expect("kept");
    let gone = store
        .add_phrase_in(
            crate::USER_DICTIONARY,
            &"鱻".into(),
            &dense("hao", 4),
            Some(9),
        )
        .expect("gone");
    assert!(store.remove_user_phrase(gone).expect("remove"));
    store
        .observe_selection(LIBRARY | 1, LIBRARY | 2)
        .expect("gram");
    store
        .observe_selection(LIBRARY | 3, LIBRARY | 1)
        .expect("gram into the shipped item");
    train(&mut store, 1, key("ni", 3), 1);
    train(&mut store, 3, key("ta", 1), 1);
    store
        .add_system_phrase_in(
            1,
            Some(LIBRARY | 1),
            &"你".into(),
            &dense("ni", 2),
            Some(11),
        )
        .expect("new reading on the shipped item");
    store
        .add_system_phrase_in(1, Some(LIBRARY | 3), &"他".into(), &dense("ta", 1), Some(4))
        .expect("known reading on the appended item");
    let live = mixed_view(&store, &base, &tokens);
    assert!(live.contains(&format!("{:#x} Some([", LIBRARY | 1)));
    store.mark_modified();
    assert!(store.save().expect("save"));
    drop(store);

    // The saved profile is the live one: every view, the user library and
    // the gram bytes, and the shipped item still owns no row of its own.
    let mut store = open(&dir, &base);
    assert_eq!(mixed_view(&store, &base, &tokens), live);
    assert_eq!(
        store
            .token_for_phrase_in(crate::USER_DICTIONARY, "龘")
            .expect("lookup"),
        Some(kept)
    );
    assert_eq!(
        store
            .export_phrases_in(crate::USER_DICTIONARY)
            .expect("export")
            .len(),
        1
    );
    assert!(store.bigram_row(LIBRARY | 1).expect("gram").is_some());
    // Again, from the reopened profile.
    train(&mut store, 1, key("ni", 3), 1);
    train(&mut store, 3, key("ta", 1), 1);
    let again = mixed_view(&store, &base, &tokens);
    assert_ne!(again, live);
    store.mark_modified();
    assert!(store.save().expect("save again"));
    drop(store);
    let store = open(&dir, &base);
    assert_eq!(mixed_view(&store, &base, &tokens), again);
    drop(store);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// What the count reads answer, with the gate as the session holds it and
/// with the gate forced open. The gate is only a fast path: the two must
/// agree, whatever write last recomputed it.
fn reads(store: &UserStore, tokens: &[u32]) -> String {
    let mut out = String::new();
    for &token in tokens {
        out.push_str(&format!(
            "{token:#x} {:?} {:?} {:?}\n",
            store.unigram_delta(token).ok(),
            store.count_delta(Some(LIBRARY | 1), token).ok(),
            store.count_delta(None, token).ok(),
        ));
    }
    out.push_str(&format!(
        "{:?} {:?}\n",
        store.bigram_row(LIBRARY | 1).ok(),
        store.bigram_row(1).ok()
    ));
    out
}

fn assert_the_gate_suppresses_nothing(store: &UserStore, tokens: &[u32], step: &str) {
    let natural = store.gate_for_test();
    let gated = reads(store, tokens);
    store.force_gate_for_test(true);
    let open = reads(store, tokens);
    store.force_gate_for_test(natural);
    assert_eq!(gated, open, "after {step}, the gate was {natural}");
}

/// A replayed item no longer holds the gate open through its reading rows.
/// After every write that recomputes the gate (a mask, a removal, a library
/// reload, an import), the count reads must still be what they are with the
/// gate forced open, with grams, trained unigrams and a populated user
/// library in the profile.
#[test]
fn the_gate_never_hides_a_count_or_a_gram_after_a_mask_removal_reload_or_import() {
    let (ni3, hao3) = (tone("ni", 3), tone("hao", 3));
    let one = item(0x4f60, 100, &[(ni3.clone(), 100)]);
    let two = item(0x597d, 50, &[(hao3.clone(), 50)]);
    let one_new = item(0x4f60, 100, &[(ni3, 130)]);
    let base = library_of(vec![(1, one.clone()), (2, two)]);
    let records = [header(150, 150), modify(1, &one, &one_new)];
    let tokens = [LIBRARY | 1, LIBRARY | 2, 0x0200_0001, 0x0700_0001];

    let (dir, mut store) = open_over("gate", &base, &records);
    assert_the_gate_suppresses_nothing(&store, &tokens, "open");
    // A gram between the two system items, and a trained unigram.
    store
        .observe_selection(LIBRARY | 1, LIBRARY | 2)
        .expect("gram");
    assert_the_gate_suppresses_nothing(&store, &tokens, "a gram");
    train(&mut store, 2, key("hao", 3), 1);
    assert_the_gate_suppresses_nothing(&store, &tokens, "training");
    // A mask of another library, an import, a user phrase and its removal,
    // the mask of the replayed library, a reload, an unload and a load.
    store.mask_out(0xff00_0000, 0x0200_0000).expect("mask 2");
    assert_the_gate_suppresses_nothing(&store, &tokens, "a mask of another library");
    store
        .add_system_phrase_in(1, Some(LIBRARY | 1), &"你".into(), &dense("ni", 3), Some(3))
        .expect("import");
    assert_the_gate_suppresses_nothing(&store, &tokens, "an import");
    let user = store
        .add_phrase_in(
            crate::USER_DICTIONARY,
            &"龘".into(),
            &dense("ni", 2),
            Some(20),
        )
        .expect("user phrase");
    assert_the_gate_suppresses_nothing(&store, &tokens, "a user phrase");
    assert!(store.remove_user_phrase(user).expect("remove"));
    assert_the_gate_suppresses_nothing(&store, &tokens, "a removal");
    store.mask_out(0xff00_0000, LIBRARY).expect("mask 1");
    assert_the_gate_suppresses_nothing(&store, &tokens, "a mask of the replayed library");
    store.load_system_libraries(base.clone()).expect("reload");
    assert_the_gate_suppresses_nothing(&store, &tokens, "a reload");
    store.unload_system_libraries(base.clone());
    store.load_system_libraries(base.clone()).expect("load");
    assert_the_gate_suppresses_nothing(&store, &tokens, "an unload and load");
    drop(store);
    std::fs::remove_dir_all(dir).expect("cleanup");

    // The state the replayed rows used to hide: a gram that outlives every
    // unigram, table and phrase row. A reload of the library takes the unigram
    // and phrase rows of its tokens away and leaves the session's grams, so the
    // gate stays open on the grams the write finds in the container alone.
    let (dir, mut store) = open_over("gate-gram-only", &base, &records);
    store
        .observe_selection(LIBRARY | 1, LIBRARY | 2)
        .expect("gram");
    assert_the_gate_suppresses_nothing(&store, &tokens, "a gram");
    store.load_system_libraries(base).expect("reload");
    assert!(
        store.bigram_row(LIBRARY | 1).expect("row").is_some(),
        "the gram outlives the reload of the library it points into"
    );
    assert!(store.gate_for_test(), "the gram keeps the gate open");
    assert_the_gate_suppresses_nothing(&store, &tokens, "a reload that leaves only the gram");
    drop(store);
    std::fs::remove_dir_all(dir).expect("cleanup");
}
