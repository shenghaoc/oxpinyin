//! Regression cases for the count memo across the real session write paths.
//!
//! These run on the selected backend, including its native user-bigram
//! container. The pin's count laws are already derived in store.rs: training
//! uses phonetic_lookup.h:925-929, imports use pinyin.cpp:515-611, reload and
//! journal replay use pinyin.cpp:264-277, all at 074a2219.

use super::*;
use crate::persistence::{SystemLibrary, SystemOriginals, UserConfLaw};
use oxpinyin_data::chunk_write::{ChunkItem, encode_phrase_item};
use oxpinyin_data::user_files::{LogRecord, SystemVersions, UserTableInfo, encode_log_records};
use std::collections::BTreeMap;

const SYSTEM_TOKEN: Token = 0x0100_0001;
const APPENDED_TOKEN: Token = 0x0100_0002;

fn key(text: &str) -> PinyinKey {
    PinyinKey::try_from(
        SyllableKey::from_text(text)
            .expect("frozen syllable")
            .index(),
    )
    .expect("syllable id")
}

fn originals() -> SystemOriginals {
    BTreeMap::from([(
        1,
        SystemLibrary {
            total: 100,
            range_end: 2,
            mapped: None,
            items: BTreeMap::from([(
                1,
                ChunkItem {
                    phrase: vec![u32::from('你')],
                    unigram: 100,
                    prons: vec![(
                        vec![ChewingKey::from_pinyin("ni").expect("key").to_packed()],
                        100,
                    )],
                },
            )]),
        },
    )])
    .into()
}

fn versions() -> SystemVersions {
    SystemVersions::for_this_build(7, 14)
}

fn transient() -> UserStore {
    let mut store =
        UserStore::open_transient(originals(), versions(), UserConfLaw::Pinyin).expect("open");
    open_count_gate(&mut store);
    store
}

fn open_count_gate(store: &mut UserStore) {
    // Keep some user data without making any tested unigram present. This
    // reaches the memo instead of the empty-store atomic fast path.
    store.set_bigram_count(900, 901, 1).expect("unrelated gram");
}

fn profile(dir: &Path) -> UserStore {
    std::fs::write(
        dir.join("user.conf"),
        UserTableInfo::conform_to(&versions()).to_text(),
    )
    .expect("marker");
    let mut store =
        UserStore::open_libpinyin(dir, originals(), versions(), UserConfLaw::Pinyin).expect("open");
    open_count_gate(&mut store);
    store
}

fn assert_memo(store: &UserStore, token: Token, value: u64) {
    assert_eq!(store.unigram_delta(token).expect("read"), value);
    assert_eq!(
        store
            .count_cache()
            .as_ref()
            .and_then(|cache| cache.unigram.get(&token).copied()),
        Some(value),
        "the read must reach the memo, including an absent row"
    );
}

fn stored_unigram(store: &UserStore, token: Token) -> Option<u64> {
    get_u64(&*store.database(), UNIGRAM, &codec::encode_token(token)).expect("raw count")
}

fn fail_followup(store: &UserStore) {
    store
        .inner
        .fail_count_followup
        .store(true, Ordering::SeqCst);
}

fn assert_injected_failure<T: std::fmt::Debug>(result: Result<T, UserStoreError>) {
    let error = result.expect_err("follow-up must fail");
    assert!(error.to_string().contains("injected post-commit failure"));
}

#[test]
fn training_entry_points_invalidate_memoised_absence_for_clones() {
    for entry in 0..3 {
        let mut store = transient();
        let reader = store.clone();
        assert_memo(&reader, SYSTEM_TOKEN, 0);
        assert_eq!(stored_unigram(&store, SYSTEM_TOKEN), None);
        let generation = store.generation();
        let seed = match entry {
            0 => store.observe_selection(1, SYSTEM_TOKEN),
            1 => store.observe_predicted(1, SYSTEM_TOKEN),
            _ => store.observe_predicted_prefix(SYSTEM_TOKEN),
        }
        .expect("train");
        assert_eq!(seed, 69);
        assert!(store.generation() > generation);
        assert_eq!(stored_unigram(&store, SYSTEM_TOKEN), Some(483));
        assert_memo(&reader, SYSTEM_TOKEN, 483);
    }
}

#[test]
fn phrase_imports_and_addon_promotion_invalidate_memoised_absence() {
    for library in [USER_DICTIONARY, NETWORK_DICTIONARY, ADDON_DICTIONARY] {
        let mut store = transient();
        let token = first_library_token(library);
        assert_memo(&store, token, 0);
        let actual = if library == ADDON_DICTIONARY {
            store
                .promote_addon_phrase("你", &[(vec![key("ni")], 5)], 40)
                .expect("promote")
        } else {
            store
                .add_phrase_in(library, "你", &[key("ni")], Some(5))
                .expect("import")
        };
        assert_eq!(actual, token);
        let expected = if library == ADDON_DICTIONARY { 40 } else { 15 };
        assert_eq!(stored_unigram(&store, token), Some(expected));
        assert_memo(&store, token, expected);
        // A reading merged into the existing item adds no unigram.
        if library != ADDON_DICTIONARY {
            assert_eq!(
                store
                    .add_phrase_in(library, "你", &[key("ni")], Some(7))
                    .expect("merge"),
                token
            );
            assert_memo(&store, token, expected);
        }
    }
}

#[test]
fn system_phrase_import_invalidates_memoised_absence() {
    let mut store = transient();
    assert_memo(&store, APPENDED_TOKEN, 0);
    let token = store
        .add_system_phrase_in(1, None, "好", &[key("hao")], Some(5))
        .expect("append");
    assert_eq!(token, APPENDED_TOKEN);
    assert_eq!(stored_unigram(&store, token), Some(15));
    assert_memo(&store, token, 15);
}

#[test]
fn removals_and_masks_invalidate_present_rows_and_reused_token_misses() {
    for remove in [false, true] {
        let mut store = transient();
        let token = store.add_phrase("你", &[key("ni")], Some(5)).expect("add");
        assert_memo(&store, token, 15);
        if remove {
            assert!(store.remove_user_phrase(token).expect("remove"));
        } else {
            store.mask_out(u32::MAX, token).expect("mask");
        }
        assert_eq!(stored_unigram(&store, token), None);
        assert_memo(&store, token, 0);
        let reused = store
            .add_phrase("好", &[key("hao")], Some(7))
            .expect("reuse");
        assert_eq!(reused, token);
        assert_eq!(stored_unigram(&store, token), Some(21));
        assert_memo(&store, token, 21);
    }
}

#[test]
fn live_system_bumps_override_memoised_absence_and_flush_invalidates_it() {
    let store = transient();
    assert_memo(&store, SYSTEM_TOKEN, 0);
    assert!(
        store
            .add_system_unigram_frequency(SYSTEM_TOKEN, 9)
            .expect("bump")
    );
    assert_eq!(stored_unigram(&store, SYSTEM_TOKEN), None);
    // The memo still has the stored zero; the live overlay is added to it.
    assert_eq!(store.unigram_delta(SYSTEM_TOKEN).expect("live read"), 9);
    assert_eq!(
        store
            .count_cache()
            .as_ref()
            .unwrap()
            .unigram
            .get(&SYSTEM_TOKEN),
        Some(&0)
    );
    store.flush_system_unigrams().expect("flush");
    assert_eq!(stored_unigram(&store, SYSTEM_TOKEN), Some(9));
    assert_memo(&store, SYSTEM_TOKEN, 9);
}

#[test]
fn save_entry_points_clear_negative_memos_and_preserve_flushed_counts_on_reopen() {
    for reporting in [false, true] {
        let scratch = tempfile::tempdir().expect("profile");
        let mut store = profile(scratch.path());
        assert_memo(&store, SYSTEM_TOKEN, 0);
        store.mark_modified();
        if reporting {
            store.save_reporting().expect("save empty memo");
        } else {
            assert!(store.save().expect("save empty memo"));
        }
        assert!(
            store.count_cache().is_none(),
            "save retires even unchanged negative entries"
        );
        assert_memo(&store, SYSTEM_TOKEN, 0);
        assert!(
            store
                .add_system_unigram_frequency(SYSTEM_TOKEN, 9)
                .expect("bump")
        );
        store.mark_modified();
        if reporting {
            store.save_reporting().expect("save live counts");
        } else {
            assert!(store.save().expect("save live counts"));
        }
        assert_eq!(stored_unigram(&store, SYSTEM_TOKEN), Some(9));
        assert_memo(&store, SYSTEM_TOKEN, 9);
        drop(store);
        let reopened =
            UserStore::open_libpinyin(scratch.path(), originals(), versions(), UserConfLaw::Pinyin)
                .expect("reopen");
        assert_memo(&reopened, SYSTEM_TOKEN, 9);
    }
}

fn write_changed_system_journal(dir: &Path) {
    let original = originals();
    let old = original[&1].item(1).expect("old item");
    let mut changed = old.clone();
    changed.unigram += 40;
    write_system_journal(
        dir,
        &[
            LogRecord::ModifyHeader {
                old_total: 100,
                new_total: 140,
            },
            LogRecord::Modify {
                token: 1,
                old_item: encode_phrase_item(&old).expect("old bytes"),
                new_item: encode_phrase_item(&changed).expect("new bytes"),
            },
        ],
    );
}

fn write_system_journal(dir: &Path, records: &[LogRecord]) {
    let payload = encode_log_records(records).expect("journal");
    std::fs::write(
        dir.join("gb_char.dbin"),
        oxpinyin_data::chunk_format::build_memory_chunk(&payload).expect("frame"),
    )
    .expect("write journal");
}

#[test]
fn system_load_and_masked_replay_replace_memoised_absence() {
    for masked in [false, true] {
        let scratch = tempfile::tempdir().expect("profile");
        let mut store = profile(scratch.path());
        assert_memo(&store, SYSTEM_TOKEN, 0);
        write_changed_system_journal(scratch.path());
        if masked {
            let logs = store
                .validate_system_logs(|_| true, PHRASE_MASK, 0)
                .expect("snapshot");
            store
                .reload_system_libraries(originals(), PHRASE_MASK, 0, logs)
                .expect("replay");
        } else {
            store.load_system_libraries(originals()).expect("load");
        }
        assert_eq!(stored_unigram(&store, SYSTEM_TOKEN), Some(40));
        assert_memo(&store, SYSTEM_TOKEN, 40);
        store.unload_system_libraries(originals()).expect("unload");
        assert_eq!(stored_unigram(&store, SYSTEM_TOKEN), None);
        assert_memo(&store, SYSTEM_TOKEN, 0);
        drop(store);
        let reopened =
            UserStore::open_libpinyin(scratch.path(), originals(), versions(), UserConfLaw::Pinyin)
                .expect("open replays before constructing its empty memo");
        assert_memo(&reopened, SYSTEM_TOKEN, 40);
    }
}

#[test]
fn journal_add_and_remove_replace_cached_counts() {
    let scratch = tempfile::tempdir().expect("profile");
    let mut store = profile(scratch.path());
    assert_memo(&store, APPENDED_TOKEN, 0);
    let added = ChunkItem {
        phrase: vec![u32::from('好')],
        unigram: 25,
        prons: vec![(
            vec![ChewingKey::from_pinyin("hao").expect("key").to_packed()],
            25,
        )],
    };
    write_system_journal(
        scratch.path(),
        &[
            LogRecord::ModifyHeader {
                old_total: 100,
                new_total: 125,
            },
            LogRecord::Add {
                token: 2,
                new_item: encode_phrase_item(&added).expect("added bytes"),
            },
        ],
    );
    store
        .load_system_libraries(originals())
        .expect("replay ADD");
    assert_eq!(stored_unigram(&store, APPENDED_TOKEN), Some(25));
    assert_memo(&store, APPENDED_TOKEN, 25);

    store
        .observe_selection(1, SYSTEM_TOKEN)
        .expect("train original");
    assert_memo(&store, SYSTEM_TOKEN, 483);
    let original = originals();
    write_system_journal(
        scratch.path(),
        &[
            LogRecord::ModifyHeader {
                old_total: 100,
                new_total: 0,
            },
            LogRecord::Remove {
                token: 1,
                old_item: encode_phrase_item(&original[&1].item(1).expect("original"))
                    .expect("removed bytes"),
            },
        ],
    );
    store
        .load_system_libraries(originals())
        .expect("replay REMOVE");
    assert_eq!(stored_unigram(&store, SYSTEM_TOKEN), None);
    assert_memo(&store, SYSTEM_TOKEN, 0);
    assert_eq!(stored_unigram(&store, APPENDED_TOKEN), None);
    assert_memo(&store, APPENDED_TOKEN, 0);
}

#[test]
fn refused_reload_preserves_the_negative_memo_until_a_commit() {
    let scratch = tempfile::tempdir().expect("profile");
    let mut store = profile(scratch.path());
    assert_memo(&store, SYSTEM_TOKEN, 0);
    let generation = store.generation();
    write_changed_system_journal(scratch.path());
    store.fail_next_replace();
    assert!(store.load_system_libraries(originals()).is_err());
    assert_eq!(
        store.generation(),
        generation,
        "the transaction rolled back"
    );
    assert_eq!(stored_unigram(&store, SYSTEM_TOKEN), None);
    assert_memo(&store, SYSTEM_TOKEN, 0);
    store
        .load_system_libraries(originals())
        .expect("committed reload");
    assert_eq!(stored_unigram(&store, SYSTEM_TOKEN), Some(40));
    assert_memo(&store, SYSTEM_TOKEN, 40);
}

#[test]
fn user_library_load_replaces_memoised_absence() {
    let scratch = tempfile::tempdir().expect("profile");
    let mut store = profile(scratch.path());
    let token = store.add_phrase("你", &[key("ni")], Some(5)).expect("add");
    store.mark_modified();
    assert!(store.save().expect("save"));
    assert_memo(&store, token, 15);
    store.unload_user_library(USER_DICTIONARY).expect("unload");
    assert_eq!(stored_unigram(&store, token), None);
    assert_memo(&store, token, 0);
    store.load_user_library(USER_DICTIONARY).expect("load");
    assert_eq!(stored_unigram(&store, token), Some(15));
    assert_memo(&store, token, 15);
}

#[test]
fn failed_bigram_followups_do_not_hide_committed_unigrams() {
    for predicted in [false, true] {
        let mut store = transient();
        assert_memo(&store, SYSTEM_TOKEN, 0);
        fail_followup(&store);
        let result = if predicted {
            store.observe_predicted(1, SYSTEM_TOKEN)
        } else {
            store.observe_selection(1, SYSTEM_TOKEN)
        };
        assert_injected_failure(result);
        assert_eq!(stored_unigram(&store, SYSTEM_TOKEN), Some(483));
        assert_memo(&store, SYSTEM_TOKEN, 483);
    }
}

#[test]
fn failed_mask_followups_do_not_restore_removed_unigrams() {
    for remove in [false, true] {
        let mut store = transient();
        let token = store.add_phrase("你", &[key("ni")], Some(5)).expect("add");
        assert_memo(&store, token, 15);
        fail_followup(&store);
        if remove {
            assert_injected_failure(store.remove_user_phrase(token));
        } else {
            assert_injected_failure(store.mask_out(u32::MAX, token));
        }
        assert_eq!(stored_unigram(&store, token), None);
        assert_memo(&store, token, 0);
        let reused = store
            .add_phrase("好", &[key("hao")], Some(7))
            .expect("reuse");
        assert_eq!(reused, token);
        assert_memo(&store, token, 21);
    }
}

#[test]
fn failed_system_import_followup_does_not_hide_the_appended_unigram() {
    let mut store = transient();
    assert_memo(&store, APPENDED_TOKEN, 0);
    fail_followup(&store);
    assert_injected_failure(store.add_system_phrase_in(1, None, "好", &[key("hao")], Some(5)));
    assert_eq!(stored_unigram(&store, APPENDED_TOKEN), Some(15));
    assert_memo(&store, APPENDED_TOKEN, 15);
}
