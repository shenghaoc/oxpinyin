//! The user store: mask-out, user index and bigram rows, journal logs and
//! user files.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use super::*;

/// The first key of `text` as parsed by a scratch instance: its two bytes.
fn first_key_bytes(context: *mut PinyinContext, text: &str) -> Vec<u8> {
    let _slots = crate::test_support::key_slot_guard();
    let instance = alloc(context);
    assert!(parse(instance, text) > 0);
    let mut key: *mut ChewingKey = ptr::null_mut();
    assert!(unsafe { pinyin_get_pinyin_key(instance, 0, &raw mut key) });
    // SAFETY: a true answer points at a live two-byte ChewingKey, read
    // before the instance is freed.
    let bytes = unsafe { std::slice::from_raw_parts(key.cast::<u8>(), 2) }.to_vec();
    unsafe { pinyin_free_instance(instance) };
    bytes
}

/// A profile whose user pinyin index holds `keys`, each with an empty
/// value, under a conforming `user.conf` for `system`; then a context on it.
fn overlong_index_context(
    system: &Path,
    user: &Path,
    keys: impl FnOnce(&[u8]) -> Vec<Vec<u8>>,
) -> *mut PinyinContext {
    let context = init_at(system, user);
    assert!(!context.is_null());
    let first = first_key_bytes(context, "ni");
    unsafe { pinyin_fini(context) };
    let rows: Vec<(Vec<u8>, Vec<u8>)> = keys(&first)
        .into_iter()
        .map(|key| (key, Vec::new()))
        .collect();
    craft_user_index(&user.join("user_pinyin_index.bin"), &rows);
    write_user_conf(user, system);
    let context = init_at(system, user);
    assert!(!context.is_null());
    context
}

/// The first key of `ni` and the 17-key run of it.
fn ni_and_its_overlong_run(first: &[u8]) -> Vec<Vec<u8>> {
    vec![first.to_vec(), first.repeat(17)]
}

/// After a guess over another key, a reparse onto the over-long key's
/// prefix does not reach it: only an explicit guess gates on these keys.
#[test]
fn a_reparse_after_a_guess_ignores_an_overlong_index_key() {
    let user = TempUserDir::new("overlong-reparse");
    let context = overlong_index_context(&system_dir(), &user.path, ni_and_its_overlong_run);
    let instance = alloc(context);
    assert_eq!(parse(instance, "hao"), 3);
    assert!(unsafe { pinyin_guess_candidates(instance, 0, 0) });
    assert_eq!(parse(instance, "ni"), 2);
    close(context, instance);
}

/// Without its prefix row, the over-long key is never walked to: the
/// guess answers true (`walk_suggestions`' key_exists gate).
#[test]
fn an_overlong_index_key_without_its_prefix_is_not_walked() {
    let user = TempUserDir::new("overlong-no-prefix");
    let context = overlong_index_context(&system_dir(), &user.path, |first| vec![first.repeat(17)]);
    let instance = alloc(context);
    assert_eq!(parse(instance, "ni"), 2);
    assert!(unsafe { pinyin_guess_candidates(instance, 0, 0) });
    close(context, instance);
}

/// `mask_out` asserts on an empty user index key.
#[test]
fn mask_out_aborts_on_an_empty_index_key() {
    assert_aborts(
        "boundary_tests::user_store::mask_out_aborts_on_an_empty_index_key",
        || {
            let user = TempUserDir::new("mask-empty-key");
            let context =
                overlong_index_context(&system_dir(), &user.path, |_| vec![Vec::new(), vec![0, 1]]);
            unsafe { pinyin_mask_out(context, 0xFFFF_FFFF, 0) };
        },
    );
}

/// A context on a profile whose user bigram holds exactly `rows`.
fn crafted_bigram_context(user: &Path, rows: Vec<(Vec<u8>, Vec<u8>)>) -> *mut PinyinContext {
    plant_bigram(user, rows);
    let context = init_at(&system_dir(), user);
    assert!(!context.is_null());
    context
}

/// Two null-token MODIFY_HEADER records.
fn two_headers() -> Vec<u8> {
    [
        crate::test_support::modify_header(0, 1, 1),
        crate::test_support::modify_header(0, 1, 2),
    ]
    .concat()
}

/// Init accepts a log of two headers.
#[test]
fn init_accepts_two_log_headers() {
    let user = TempUserDir::new("init-two-headers");
    write_user_conf(&user.path, &system_dir());
    write_log(&user.path, "gb_char.dbin", &two_headers());
    let context = init_at(&system_dir(), &user.path);
    assert!(!context.is_null());
    unsafe { pinyin_fini(context) };
}

/// `mask_out` does not read the log of an unloaded library: true.
#[test]
fn mask_out_skips_the_log_of_an_unloaded_library() {
    let user = TempUserDir::new("mask-unloaded-log");
    write_user_conf(&user.path, &system_dir());
    let context = init_at(&system_dir(), &user.path);
    assert!(!context.is_null());
    write_log(&user.path, "gbk_char.dbin", &two_headers());
    assert!(unsafe { crate::config::pinyin_unload_phrase_library(context, 2) });
    assert!(unsafe { pinyin_mask_out(context, 0, 0) });
    unsafe { pinyin_fini(context) };
}

/// `mask_out` over an unmappable library answers true, prints the one
/// `mmap … failed!` line and changes no file and no item.
#[test]
fn mask_out_over_an_unmappable_library_changes_nothing() {
    let run = crate::test_support::run_in_child(
        "boundary_tests::user_store::mask_out_over_an_unmappable_library_changes_nothing",
        |user| {
            write_user_conf(user, &system_dir());
            let (system, context) = private_context(user);
            let instance = alloc(context);
            assert_eq!(import_batch(context, 1, &[("龘龘", "da2'da2", 1)]), [true]);
            assert!(unsafe { pinyin_save(context) });
            assert_eq!(
                import_batch(context, 7, &[("龘龘龘", "da2'da2'da2", 1)]),
                [true]
            );
            assert!(unsafe { pinyin_token_add_unigram_frequency(instance, 0x0100_1225, 7) });
            let items = [lookup(instance, "龘龘"), lookup(instance, "龘龘龘")];
            let count = unigram(instance, 0x0100_1225);
            let files = files_of(user);
            let library = system.join("gb_char.bin");
            let original = std::fs::read(&library).expect("gb_char.bin");
            std::fs::remove_file(&library).expect("unlink gb_char.bin");
            assert!(unsafe { pinyin_mask_out(context, 0, 0) });
            assert_eq!(files_of(user), files);
            assert_eq!(unigram(instance, 0x0100_1225), count);
            assert_eq!(
                [lookup(instance, "龘龘"), lookup(instance, "龘龘龘")],
                items
            );
            std::fs::write(&library, original).expect("restore gb_char.bin");
            assert!(unsafe { pinyin_save(context) });
        },
    );
    let system = run.user_dir.parent().expect("scratch").join("system");
    assert_eq!(
        String::from_utf8_lossy(&run.stderr),
        format!("mmap {} failed!{NL}", system.join("gb_char.bin").display())
    );
}

/// A user-gram mask reaches the file only at the next armed save: an
/// unarmed save keeps the masked gram on disk, and the save after an
/// import removes it (#677).
#[test]
fn a_masked_user_gram_leaves_the_file_at_the_next_armed_save() {
    let user = TempUserDir::new("issue677");
    let key = 0x0700_0001_u32.to_le_bytes().to_vec();
    let gram = gram_bytes(100, &[(0x0100_0001, 100)]);
    let context = crafted_bigram_context(&user.path, vec![(key.clone(), gram.clone())]);
    let stored = || {
        crate::test_support::store_rows(&user.path.join("user_bigram.db"), true)
            .into_iter()
            .find(|(found, _)| *found == key)
            .map(|(_, value)| value)
    };
    assert_eq!(
        import_batch(context, 7, &[("达达", "da2'da2", 100)]),
        [true]
    );
    assert!(unsafe { pinyin_save(context) });
    assert!(unsafe { pinyin_mask_out(context, 0x0F00_0000, 0x0700_0000) });
    assert!(!unsafe { pinyin_save(context) });
    assert_eq!(stored(), Some(gram));
    assert_eq!(
        import_batch(context, 7, &[("你你", "ni3'ni3", 100)]),
        [true]
    );
    assert!(unsafe { pinyin_save(context) });
    assert_eq!(stored(), None);
    unsafe { pinyin_fini(context) };
}

/// Whether the phrase segmenter matched the one-character `龘`, and the
/// token it stored (`null_token` after a failed match).
fn segmented_token(instance: *mut PinyinInstance) -> (bool, u32) {
    let matched = unsafe { crate::phrase::pinyin_phrase_segment(instance, c"龘".as_ptr()) };
    let mut count = UNTOUCHED;
    assert!(unsafe { crate::phrase::pinyin_get_n_phrase(instance, &raw mut count) });
    assert_eq!(count, 1);
    let mut token = UNTOUCHED;
    assert!(unsafe { crate::phrase::pinyin_get_phrase_token(instance, 0, &raw mut token) });
    (matched, token)
}

/// Every pin scoring step reads the token's phrase item first and skips the
/// token when the read fails (`phrase_lookup.cpp:310,329`,
/// `phonetic_lookup.h:646,674`). A phrase-table token of library 7 whose item
/// is gone is skipped by the segmenter and the sentence guess however strong
/// its sentence-start bigram; the same bigram admits the readable item with a
/// zero unigram. The stale phase has no sentence witness: the n-best result
/// is the same with and without the gate, so the segmenter is the C-visible
/// one and the model layer covers the n-best costs.
#[test]
fn a_user_file_token_without_its_item_is_skipped_and_a_zero_item_is_not() {
    let user = TempUserDir::new("stale-user-file-token");
    let (context, instance) = open_at(&user.path);
    // The first import gives the unigram 0 and the reading count 0; the
    // second adds 1 to that reading and nothing to the unigram
    // (`phrase_index.cpp:73-84`, `pinyin.cpp:563-582`), so the step prices
    // the reading at 1/1, not at the zero total's 0 (`phrase_index.h:155-157`).
    assert_eq!(
        import_batch(context, 7, &[("龘", "ni", 0), ("龘", "ni", 1)]),
        [true, true]
    );
    let token = lookup(instance, "龘")
        .into_iter()
        .find(|token| token >> 24 == 7)
        .expect("a user token");
    assert!(unsafe { pinyin_save(context) });
    close(context, instance);
    let item_file = std::fs::read(user.path.join("user.bin")).expect("user.bin");
    // The sentence-start gram admits the token with all of its mass.
    let gram = gram_bytes(100, &[(token, 100)]);
    let plant = || {
        plant_bigram(
            &user.path,
            vec![(1_u32.to_le_bytes().to_vec(), gram.clone())],
        )
    };
    let guess = |instance| {
        assert_eq!(parse(instance, "ni"), 2);
        sentence_after_guess(instance)
    };

    // The item is gone: the phrase table and the pinyin index still name it.
    let empty = oxpinyin_data::chunk_write::build_chunk(&[]).expect("an empty library");
    std::fs::write(
        user.path.join("user.bin"),
        oxpinyin_data::chunk_format::build_memory_chunk(
            &empty[oxpinyin_data::chunk_format::CHUNK_HEADER_SIZE..],
        )
        .expect("frame"),
    )
    .expect("write user.bin");
    plant();
    let (context, instance) = open_at(&user.path);
    assert_eq!(lookup(instance, "龘")[..], [token], "the stale phrase row");
    assert_eq!(
        guess(instance).as_deref(),
        Some("你"),
        "the system sentence"
    );
    assert_eq!(segmented_token(instance), (false, crate::types::null_token));
    close(context, instance);

    // The item is back with a zero unigram: the bigram branch admits it.
    std::fs::write(user.path.join("user.bin"), &item_file).expect("restore user.bin");
    plant();
    let (context, instance) = open_at(&user.path);
    let mut unigram = UNTOUCHED;
    assert!(unsafe { pinyin_token_get_unigram_frequency(instance, token, &raw mut unigram) });
    assert_eq!(unigram, 0);
    assert_eq!(guess(instance).as_deref(), Some("龘"));
    assert_eq!(segmented_token(instance), (true, token));
    close(context, instance);
}
