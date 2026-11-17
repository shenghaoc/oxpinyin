//! Phrase-library load and unload, and the facade total of a failed
//! layout.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use super::*;

/// The facade total of a fresh instance on `context`.
fn facade_total(context: *mut PinyinContext) -> u64 {
    let instance = alloc(context);
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let total = unsafe { crate::state::instance_ref(instance) }
        .core
        .lm
        .amplified_total();
    unsafe { pinyin_free_instance(instance) };
    total
}

/// A context on a private system under `scratch` whose `gbk_char.bin`
/// (library 2) is a checksum-valid, in-bounds chunk that
/// `SubPhraseIndex::load` refuses at its second separator
/// (`phrase_index.cpp:355`). Its local total is T, the facade total of the
/// other libraries, which the facade does not add (`:242-246`). Answers the
/// context and T.
fn layout_failed_gbk_context(scratch: &Path) -> (*mut PinyinContext, u64) {
    let system = private_system(scratch);
    let user = scratch.join("user");
    std::fs::create_dir(&user).expect("user directory");
    let library = system.join("gbk_char.bin");
    let write = |total: u32| {
        let mut payload = [0_u8; 32];
        for (at, value) in [(0, total), (4, 17), (8, 20), (12, 24)] {
            payload[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        payload[16] = b'#';
        payload[19] = b'!';
        let chunk = oxpinyin_data::chunk_format::build_memory_chunk(&payload).expect("frame");
        std::fs::remove_file(&library).expect("unlink gbk_char.bin");
        std::fs::write(&library, chunk).expect("write gbk_char.bin");
    };
    write(0);
    let context = init_at(&system, &user);
    assert!(!context.is_null());
    let total = facade_total(context);
    unsafe { pinyin_fini(context) };
    assert!(total > 0, "the other libraries hold a total");
    write(u32::try_from(total).expect("a guint32 total"));
    let context = init_at(&system, &user);
    assert!(!context.is_null());
    assert_eq!(
        facade_total(context),
        total,
        "the refused library adds nothing"
    );
    (context, total)
}

/// The import iterator's constructor does not compact
/// (`pinyin.cpp:506-512`). An unload of the layout-failed library 2 while
/// the iterator is open takes its local total T out of the facade total T
/// (`pinyin.cpp:473`, `phrase_index.cpp:265`), and the scorer aborts on the
/// zero total (`pinyin.cpp:1859`, register row 61).
#[test]
fn an_unload_inside_an_open_import_zeroes_the_facade_total() {
    assert_aborts(
        "boundary_tests::library_loading::an_unload_inside_an_open_import_zeroes_the_facade_total",
        || {
            let scratch = tempfile::tempdir().expect("scratch");
            let (context, _) = layout_failed_gbk_context(scratch.path());
            let iter = unsafe { pinyin_begin_add_phrases(context, 1) };
            assert!(!iter.is_null());
            assert!(unsafe { pinyin_unload_phrase_library(context, 2) });
            let instance = alloc(context);
            assert_eq!(parse(instance, "nihao"), 5);
            unsafe { pinyin_guess_candidates(instance, 0, crate::test_support::DEFAULT_SORT) };
        },
    );
}

/// An unload inside the open import takes library 2's local total T out
/// of the facade total T (`phrase_index.cpp:261-269`). The end's compact
/// has no sub-index of library 2 left to rebuild (`phrase_index.cpp:651-652`),
/// so the total stays zero and the scorer aborts (`pinyin.cpp:1859`,
/// register row 61).
#[test]
fn an_import_that_ends_after_the_unload_keeps_the_zero_total() {
    assert_aborts(
        "boundary_tests::library_loading::an_import_that_ends_after_the_unload_keeps_the_zero_total",
        || {
            let scratch = tempfile::tempdir().expect("scratch");
            let (context, _) = layout_failed_gbk_context(scratch.path());
            let iter = unsafe { pinyin_begin_add_phrases(context, 1) };
            assert!(!iter.is_null());
            assert!(unsafe { pinyin_unload_phrase_library(context, 2) });
            unsafe { pinyin_end_add_phrases(iter) };
            let instance = alloc(context);
            assert_eq!(parse(instance, "nihao"), 5);
            unsafe { pinyin_guess_candidates(instance, 0, crate::test_support::DEFAULT_SORT) };
        },
    );
}

/// Control: `end_add_phrases` compacts first (`pinyin.cpp:657`), so the
/// library's local total is zero at the unload and the facade total stays
/// T.
#[test]
fn an_unload_after_the_import_ends_keeps_the_facade_total() {
    let scratch = tempfile::tempdir().expect("scratch");
    let (context, total) = layout_failed_gbk_context(scratch.path());
    let iter = unsafe { pinyin_begin_add_phrases(context, 1) };
    assert!(!iter.is_null());
    unsafe { pinyin_end_add_phrases(iter) };
    assert!(unsafe { pinyin_unload_phrase_library(context, 2) });
    assert_eq!(facade_total(context), total);
    let instance = alloc(context);
    assert_eq!(parse(instance, "nihao"), 5);
    assert!(unsafe { pinyin_guess_candidates(instance, 0, crate::test_support::DEFAULT_SORT) });
    assert!(n_candidate(instance) > 0);
    close(context, instance);
}

/// The end of an import into the layout-failed library 2 rebuilds its
/// sub-index from its one item (`pinyin.cpp:657`, `phrase_index.cpp:648-675`):
/// count 9 times the import's factor 3 (`pinyin.cpp:522`, `:603-605`), so
/// the local total is 27. An increment that brings it to `u32::MAX` does not
/// overflow it (`phrase_index.cpp:170-171`) and is accepted.
#[test]
fn an_increment_after_an_import_into_a_failed_layout_sees_the_item_sum() {
    let scratch = tempfile::tempdir().expect("scratch");
    let (context, _) = layout_failed_gbk_context(scratch.path());
    assert_eq!(import_batch(context, 2, &[("龘龘", "da2'da2", 9)]), [true]);
    let instance = alloc(context);
    let token = lookup(instance, "龘龘")
        .into_iter()
        .find(|token| token >> 24 == 2)
        .expect("the imported token");
    assert!(unsafe { pinyin_token_add_unigram_frequency(instance, token, u32::MAX - 27) });
    close(context, instance);
}

/// A save after an import into the layout-failed library 2. The save's
/// compact rebuilds the sub-index from its items (`pinyin.cpp:1139`,
/// `phrase_index.cpp:648-675`), and the journal's header record holds the
/// remapped chunk's total T and the item sum (`phrase_index.cpp:345`,
/// `:392-400`): count 9 times the import's factor 3 (`pinyin.cpp:522`,
/// `:603-605`). An unload then takes that local total out of the facade
/// total T + 27 (`phrase_index.cpp:261-269`).
#[test]
fn a_save_after_an_import_into_a_failed_layout_journals_the_item_sum() {
    let scratch = tempfile::tempdir().expect("scratch");
    let (context, total) = layout_failed_gbk_context(scratch.path());
    assert_eq!(import_batch(context, 2, &[("龘龘", "da2'da2", 9)]), [true]);
    assert!(unsafe { pinyin_save(context) });
    assert!(unsafe { pinyin_unload_phrase_library(context, 2) });
    assert_eq!(facade_total(context), total);
    unsafe { pinyin_fini(context) };
    let journal = std::fs::read(scratch.path().join("user/gbk_char.dbin")).expect("gbk_char.dbin");
    let word = |at: usize| u32::from_le_bytes(journal[8 + at..12 + at].try_into().expect("a word"));
    // LOG_MODIFY_HEADER, null token, then the old and the new total.
    assert_eq!((word(0), word(4)), (4, 0));
    assert_eq!((u64::from(word(10)), word(14)), (total, 27));
}

/// The GBK-library tokens of the current candidate list.
fn gbk_tokens(instance: *mut PinyinInstance) -> Vec<u32> {
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let inst = unsafe { crate::state::instance_ref(instance) };
    inst.state
        .candidates
        .iter()
        .filter_map(|cand| cand.token.get().map(oxpinyin_core::PhraseToken::value))
        .filter(|token| token >> 24 == 2)
        .collect()
}

/// An unload of the GBK library hides its rows from the next candidate
/// guess on the same parse, with no reparse.
#[test]
fn an_unload_hides_its_rows_without_a_reparse() {
    let user = TempUserDir::new("row54-unload");
    let (context, instance) = open_at(&user.path);
    assert!(unsafe { pinyin_set_options(context, 1 << 5) });
    assert_eq!(parse(instance, "ni"), 2);
    assert!(unsafe { pinyin_guess_candidates(instance, 0, crate::test_support::DEFAULT_SORT) });
    assert!(!gbk_tokens(instance).is_empty());
    assert!(unsafe { crate::config::pinyin_unload_phrase_library(context, 2) });
    assert!(unsafe { pinyin_guess_candidates(instance, 0, crate::test_support::DEFAULT_SORT) });
    assert!(gbk_tokens(instance).is_empty());
    assert!(n_candidate(instance) > 0);
    close(context, instance);
}

/// User phrase-table rows whose token names a library that is never
/// loaded (15) are filtered from the lookup and the prediction.
#[test]
fn rows_of_a_never_loaded_library_are_filtered() {
    let user = TempUserDir::new("never-loaded-library");
    let value = ((15_u32 << 24) | 1).to_le_bytes().to_vec();
    craft_user_index(
        &user.path.join("user_phrase_index.bin"),
        &[
            (ucs4("你"), Vec::new()),
            ([ucs4("你"), ucs4("龘")].concat(), value.clone()),
            (ucs4("龘龘"), value),
        ],
    );
    write_user_conf(&user.path, &system_dir());
    let (context, instance) = open_at(&user.path);
    // SAFETY: a fresh GArray of four-byte elements, freed below.
    let array = unsafe { glib_sys::g_array_new(0, 0, 4) };
    assert!(!unsafe {
        crate::dict::pinyin_lookup_tokens(instance, c"龘龘".as_ptr(), array.cast())
    });
    // SAFETY: a live GArray, freed once.
    unsafe {
        assert_eq!((*array).len, 0);
        glib_sys::g_array_free(array, 1);
    }
    assert!(unsafe { pinyin_guess_predicted_candidates(instance, c"你".as_ptr()) });
    assert!(
        !capture_rows(instance)
            .iter()
            .any(|(_, text, _)| text.contains('龘'))
    );
    close(context, instance);
}
