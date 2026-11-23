//! The readings of a replayed system item (#871).
//!
//! Basis: `074a2219 phrase_index.cpp:56-143,477-501`: an import adds to the first entry with
//! the keys, and training moves each matching entry on its own count.

use std::path::PathBuf;

use crate::data::phrase_library::{ChunkItem, encode_phrase_item};
use crate::data::user_files::{LogRecord, SystemVersions};
use crate::model::UserModel;
use crate::parser::ChewingKey;

use super::super::{export_state, packed_to_pinyin_keys};
use crate::user::store::UserStore;
use crate::user::test_session::tempdir;
use crate::user::{SystemLibrary, SystemOriginals, UserConfLaw};

const LIBRARY: u32 = 0x0100_0000;

fn versions() -> SystemVersions {
    SystemVersions::for_this_build(7, 14)
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
    crate::user::LibraryTable::from([(
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

fn open(dir: &std::path::Path, base: &SystemOriginals) -> UserStore {
    crate::user::test_session::open(dir, base.fixture_copy(), versions(), UserConfLaw::Pinyin)
        .expect("open")
}

/// A profile whose `gb_char.dbin` holds `records` over `base`.
fn open_over(name: &str, base: &SystemOriginals, records: &[LogRecord]) -> (PathBuf, UserStore) {
    let dir = tempdir(name);
    let payload = crate::data::user_files::encode_log_records(records).expect("log");
    std::fs::write(
        dir.join("gb_char.dbin"),
        crate::data::chunk_format::build_memory_chunk(&payload).expect("chunk"),
    )
    .expect("write log");
    assert!(
        crate::data::user_files::UserTableInfo::conform_to(&versions())
            .save(dir.join("user.conf").as_path())
    );
    let store = open(&dir, base);
    (dir, store)
}

/// Basis: `074a2219 phrase_index.cpp:439-517` (each slot's replayed value; the header's total).
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
    assert_eq!(store.system_unigram(LIBRARY | 1), Some(100), "unchanged");
    assert_eq!(store.system_unigram(LIBRARY | 2), Some(70), "changed");
    assert_eq!(store.system_unigram(LIBRARY | 3), Some(25), "restored");
    assert_eq!(store.unigram_delta(LIBRARY | 5), 11);
    assert_eq!(
        Some(crate::user::test_session::library_total(&store, 1)),
        Some(971)
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

fn train(store: &mut UserStore, slot: u32, reading: ChewingKey, paths: usize) {
    let mut readings = (0..paths).map(|_| vec![reading]);
    UserModel::observe_with_keys(store, &[], &(LIBRARY | slot), &mut readings).expect("train");
}

fn dense(syllable: &str, tone_: u8) -> Vec<crate::user::PinyinKey> {
    packed_to_pinyin_keys(&tone(syllable, tone_)).expect("dense")
}

/// Basis: `phrase_index.cpp:56-91,122-139` (an import adds to the first entry with the keys).
#[test]
fn entries_with_the_same_keys_stay_separate_through_import_save_and_reopen() {
    let ni3 = tone("ni", 3);
    let hao3 = tone("hao", 3);
    let one = item(0x4f60, 100, &[(ni3.clone(), 100)]);
    let two = item(0x597d, 50, &[(hao3.clone(), 30), (hao3.clone(), 40)]);
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
    assert_eq!(counts(&store, 1), [100, 160]);
    assert_eq!(exported(&store, &base, 1), [100, 160]);
    assert_eq!(counts(&store, 2), [30, 40]);
    assert_eq!(exported(&store, &base, 2), [30, 40]);

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
    store.save_reporting().expect("save");
    drop(store);

    let store = open(&dir, &base);
    assert_eq!(counts(&store, 1), [107, 160]);
    assert_eq!(exported(&store, &base, 1), [107, 160]);
    assert_eq!(counts(&store, 2), [39, 40]);
    assert_eq!(exported(&store, &base, 2), [39, 40]);
    drop(store);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// Basis: `phrase_index.cpp:677-715`, `pinyin.cpp:1194-1200` (the reload after a mask maps the chunk again).
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
    let mut remapped = base.fixture_copy();
    store
        .reload_system_libraries_retaining(&mut remapped, 0xff00_0000, LIBRARY)
        .expect("reload");
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
    store.save_reporting().expect("save");
    drop(store);
    let store = open(&dir, &base);
    assert_eq!(counts(&store, 1), [104, 40]);
    drop(store);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// What a session answers for a mixed profile, in the order a caller asks:
/// the readings and the unigram delta of each system token, the count reads,
/// the grams, the user phrases and the saved shape's overrides and phrase
/// table.
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
    // The library's sub-index total is not part of this view either: the
    // pin's merge counts a logged item's unigram beside the header's total,
    // so a live session's and a reopened one's differ. The caller asserts
    // each.
    out.push_str(&format!("gram {:?}\n", store.bigram_row(LIBRARY | 1).ok()));
    let state = export_state(store, base).expect("export");
    out.push_str(&format!(
        "{:?} {:?} {:?}\n",
        state.system_overrides,
        store.phrases(),
        state.phrase_table
    ));
    out
}

/// Basis: `074a2219 phrase_index.cpp:215,233,395-397,446-451,484-512,648-675` (the totals below).
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
    assert_eq!(counts(&store, 1), [107]);
    assert_eq!(counts(&store, 2), [50]);
    assert_eq!(counts(&store, 3), [33]);
    assert_eq!(store.system_unigram(LIBRARY | 1), Some(120));
    assert_eq!(store.system_unigram(LIBRARY | 2), Some(77));
    assert_eq!(store.unigram_delta(LIBRARY | 3), 33);

    let kept = store
        .add_phrase_in(
            crate::vocab::USER_DICTIONARY,
            &"龘".into(),
            &dense("ni", 2),
            Some(20),
        )
        .expect("kept");
    let gone = store
        .add_phrase_in(
            crate::vocab::USER_DICTIONARY,
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
    // The live sub-index total, by the pin's merge (074a2219
    // `phrase_index.cpp`): the header record sets it to 230 (`:504-512`),
    // the in-place MODIFY records leave it (`:490-498`), and the ADD
    // record adds the item's 33 again (`:446-451`, `:215`), which the
    // header's 230 already held. The session's adds and trainings then
    // raise it with the items: 1086 + 560 + 516 - (120 + 77 + 33).
    assert_eq!(
        crate::user::test_session::library_total(&store, 1),
        263 + 1932
    );
    store.mark_modified();
    store.save_reporting().expect("save");
    drop(store);

    let mut store = open(&dir, &base);
    assert_eq!(mixed_view(&store, &base, &tokens), live);
    // The reopened sub-index total, by the pin's save and merge. The save
    // compacts, so the total is the items' sum, 2162 (`pinyin.cpp:1139`,
    // `phrase_index.cpp:648-675`), and `diff` logs it as the header's new
    // total (`:395-397`). The merge sets it (`:504-512`). The MODIFY of
    // item 1 grows the item, so it takes out the old item's 100 and adds
    // the new one's 1086 (`:484-489`, `:233`, `:215`). The MODIFY of item 2
    // is in place (`:490-498`). The ADD of item 3 adds its 516 (`:446-451`).
    assert_eq!(
        crate::user::test_session::library_total(&store, 1),
        2162 - 100 + 1086 + 516
    );
    assert_eq!(
        store
            .token_for_phrase_in(crate::vocab::USER_DICTIONARY, "龘")
            .expect("lookup"),
        Some(kept)
    );
    assert_eq!(
        crate::user::test_session::stored_phrase_rows(&store, crate::vocab::USER_DICTIONARY).len(),
        1
    );
    assert!(store.bigram_row(LIBRARY | 1).expect("gram").is_some());
    train(&mut store, 1, key("ni", 3), 1);
    train(&mut store, 3, key("ta", 1), 1);
    let again = mixed_view(&store, &base, &tokens);
    assert_ne!(again, live);
    store.mark_modified();
    store.save_reporting().expect("save again");
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
            Some(store.unigram_delta(token)),
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

/// Basis: Rust contract (the count gate is a fast path: forced open, the reads are the same).
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
    store
        .observe_selection(LIBRARY | 1, LIBRARY | 2)
        .expect("gram");
    assert_the_gate_suppresses_nothing(&store, &tokens, "a gram");
    train(&mut store, 2, key("hao", 3), 1);
    assert_the_gate_suppresses_nothing(&store, &tokens, "training");
    store.mask_out(0xff00_0000, 0x0200_0000).expect("mask 2");
    assert_the_gate_suppresses_nothing(&store, &tokens, "a mask of another library");
    store
        .add_system_phrase_in(1, Some(LIBRARY | 1), &"你".into(), &dense("ni", 3), Some(3))
        .expect("import");
    assert_the_gate_suppresses_nothing(&store, &tokens, "an import");
    let user = store
        .add_phrase_in(
            crate::vocab::USER_DICTIONARY,
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
    store
        .load_system_libraries(base.fixture_copy())
        .expect("reload");
    assert_the_gate_suppresses_nothing(&store, &tokens, "a reload");
    store.unload_system_libraries(base.fixture_copy());
    store
        .load_system_libraries(base.fixture_copy())
        .expect("load");
    assert_the_gate_suppresses_nothing(&store, &tokens, "an unload and load");
    drop(store);
    std::fs::remove_dir_all(dir).expect("cleanup");

    // A gram that outlives every unigram, table and phrase row keeps the gate open.
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

/// Basis: `pinyin.cpp:563-604`, `phrase_index.cpp:208-235` (one live item per slot; each total moves once).
#[test]
fn a_replayed_add_and_the_imports_live_only_in_the_buffer() {
    let one = item(0x4f60, 100, &[(tone("ni", 3), 10)]);
    let two = item(0x597d, 50, &[(tone("hao", 3), 20)]);
    let added = item(0x4e94, 11, &[(tone("wu", 3), 50)]);
    let base = library_of(vec![(1, one.clone()), (2, two.clone())]);
    let (dir, mut store) = open_over(
        "buffer-only",
        &base,
        &[LogRecord::Add {
            token: 5,
            new_item: encode_phrase_item(&added).expect("added"),
        }],
    );
    let arena_library =
        |store: &UserStore| store.session_target().phrases().libraries.contains_key(&1);
    let appended = LIBRARY | 5;
    assert!(!arena_library(&store));
    assert!(store.with_user_item(appended, |_| ()).is_some());
    assert_eq!(store.system_unigram(appended), Some(11));
    assert_eq!(
        Some(crate::user::test_session::library_total(&store, 1)),
        Some(161)
    );

    // `pinyin.cpp:593-604`: the new phrase's `count * 3` moves the item and the total once.
    let new = store
        .add_system_phrase_in(1, None, &"六".into(), &dense("liu", 4), Some(5))
        .expect("import a new phrase");
    assert_eq!(new, LIBRARY | 6);
    assert!(!arena_library(&store));
    assert_eq!(store.system_unigram(new), Some(15));
    assert_eq!(
        Some(crate::user::test_session::library_total(&store, 1)),
        Some(176)
    );
    assert_eq!(store.unigram_total(), 0);

    store
        .add_system_phrase_in(1, Some(appended), &"五".into(), &dense("wu", 3), Some(4))
        .expect("onto the appended item");
    store
        .add_system_phrase_in(1, Some(LIBRARY | 1), &"你".into(), &dense("ni", 3), Some(7))
        .expect("onto a shipped item");
    store
        .add_system_phrase_in(1, Some(new), &"六".into(), &dense("liu", 4), Some(3))
        .expect("onto the new phrase");
    assert_eq!(counts(&store, 5), [54]);
    assert_eq!(counts(&store, 1), [17]);
    assert_eq!(counts(&store, 6), [8]);
    assert!(!arena_library(&store));
    assert_eq!(
        Some(crate::user::test_session::library_total(&store, 1)),
        Some(176)
    );
    assert_eq!(store.system_unigram(LIBRARY | 1), Some(100));

    store.mark_modified();
    store.save_reporting().expect("save");
    drop(store);
    let store = open(&dir, &base);
    assert!(!arena_library(&store));
    assert_eq!(counts(&store, 5), [54]);
    assert_eq!(counts(&store, 6), [8]);
    assert_eq!(counts(&store, 1), [17]);
    drop(store);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// Basis: policy row 98 (a refused mask leaves the replayed ADD live, with its text, for an import).
#[test]
fn live_appended_item_accepts_import_after_failed_mask() {
    let base = library_of(vec![(1, item(0x4f60, 100, &[(tone("ni", 3), 10)]))]);
    let added = item(0x4e94, 11, &[(tone("wu", 3), 50)]);
    let (dir, store) = open_over(
        "materialized-appended-failed-mask",
        &base,
        &[LogRecord::Add {
            token: 5,
            new_item: encode_phrase_item(&added).expect("added"),
        }],
    );
    drop(store);
    crate::user::native_fixture::craft_user_bigram(
        &dir.join("user_bigram.db"),
        &[(1_u32.to_le_bytes().to_vec(), vec![0, 0, 0, 0, 1])],
    );
    let mut store = open(&dir, &base);
    let token = LIBRARY | 5;
    assert!(store.with_live_item(token, |_| ()).is_some());
    assert_eq!(store.lookup().phrase_text(token), Some("五".into()));
    assert!(matches!(
        store.mask_out(u32::MAX, 0),
        Err(crate::user::UserStoreError::PartialUserBigramTail)
    ));
    let before = Some(crate::user::test_session::library_total(&store, 1));
    assert_eq!(
        store
            .add_system_phrase_in(1, Some(token), &"五".into(), &dense("wu", 3), Some(4))
            .expect("import existing"),
        token
    );
    assert!(store.with_live_item(token, |_| ()).is_some());
    assert_eq!(counts(&store, 5), [54]);
    assert_eq!(store.system_unigram(token), Some(11));
    assert_eq!(
        Some(crate::user::test_session::library_total(&store, 1)),
        before
    );
    drop(store);
    std::fs::remove_dir_all(dir).expect("cleanup");
}

/// Basis: policy row 98, for an item a session import created.
#[test]
fn live_new_import_keeps_text_and_accepts_another_reading_after_failed_mask() {
    let base = library_of(vec![(1, item(0x4f60, 100, &[(tone("ni", 3), 10)]))]);
    let (dir, store) = open_over("materialized-new-import", &base, &[]);
    drop(store);
    crate::user::native_fixture::craft_user_bigram(
        &dir.join("user_bigram.db"),
        &[(1_u32.to_le_bytes().to_vec(), vec![0, 0, 0, 0, 1])],
    );
    let mut store = open(&dir, &base);
    let token = store
        .add_system_phrase_in(1, None, &"五".into(), &dense("wu", 3), Some(5))
        .expect("import new");
    assert!(store.with_live_item(token, |_| ()).is_some());
    assert_eq!(store.lookup().phrase_text(token), Some("五".into()));
    assert!(matches!(
        store.mask_out(u32::MAX, 0),
        Err(crate::user::UserStoreError::PartialUserBigramTail)
    ));
    assert_eq!(
        store
            .add_system_phrase_in(1, Some(token), &"五".into(), &dense("wu", 3), Some(4))
            .expect("import existing"),
        token
    );
    assert!(store.with_live_item(token, |_| ()).is_some());
    assert_eq!(
        store.with_user_item(token, |item| item.pronunciation(0).expect("reading").freq),
        Some(9)
    );
    assert_eq!(store.system_unigram(token), Some(15));
    drop(store);
    std::fs::remove_dir_all(dir).expect("cleanup");
}
