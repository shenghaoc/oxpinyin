//! The library bits of a token: tokens under high bits, two tokens of one
//! library and five libraries that hold one string.

use super::*;

/// An import aborts where the user phrase table maps the phrase to two
/// tokens of one library.
#[test]
fn import_aborts_on_two_tokens_of_one_library() {
    assert_aborts(
        "boundary_tests::library_tokens::import_aborts_on_two_tokens_of_one_library",
        || {
            let user = TempUserDir::new("import-duplicate-token");
            let (context, instance) = open_at(&user.path);
            close(context, instance);
            set_phrase_table_row(&user.path, "你好", &[0x0700_0001, 0x0700_0002]);
            let (context, _) = open_at(&user.path);
            import_batch(context, 7, &[("你好", "ni'hao", 5)]);
        },
    );
}

/// The high four bits a crafted user file can give a token of 龘龘.
const TAG: u32 = 0x8000_0000;

/// The token getters select the library with `PHRASE_INDEX_LIBRARY_INDEX`
/// and the slot with `PHRASE_MASK` (`pinyin.cpp:2776`, `:2801-2818`,
/// `phrase_index.h:647`, `phrase_index.cpp:186`, `novel_types.h:44` at the
/// pin `074a2219`), so a token with high bits set reads the same item, in a
/// system library and in a user library.
#[test]
fn the_phrase_and_reading_getters_ignore_the_high_bits_of_a_token() {
    let user = TempUserDir::new("tagged-getters");
    let dada = saved_user_phrase(&user.path);
    let (context, instance) = open_at(&user.path);
    for (token, text) in [(lookup(instance, "你")[0], "你"), (dada, "龘龘")] {
        assert_eq!(phrase_of(instance, token).as_deref(), Some(text));
        assert_eq!(phrase_of(instance, token | TAG).as_deref(), Some(text));
        assert_eq!(
            reading_bytes(instance, token | TAG, 0),
            reading_bytes(instance, token, 0)
        );
    }
    close(context, instance);
}

/// A user phrase-table row whose token has high bits set. The search files
/// the token under its masked library and keeps its value
/// (`phrase_large_table3.cpp:42-48`), so `pinyin_lookup_tokens` answers it
/// (`pinyin.cpp:2652-2667`). `_add_phrase` then finds a token of its own
/// library (`pinyin.cpp:552`, `:564`) and takes the existing-item branch:
/// the item gains a reading and no token is made (`:565-582`).
#[test]
fn an_import_finds_the_token_of_its_library_under_high_bits() {
    let user = TempUserDir::new("tagged-import");
    let token = saved_user_phrase(&user.path);
    set_phrase_table_row(&user.path, "龘龘", &[token | TAG]);
    let (context, instance) = open_at(&user.path);
    assert_eq!(lookup(instance, "龘龘"), [token | TAG]);
    assert_eq!(import_batch(context, 7, &[("龘龘", "da4'da4", 5)]), [true]);
    assert_eq!(lookup(instance, "龘龘"), [token | TAG], "no second token");
    let mut count = UNTOUCHED;
    assert!(pinyin_token_get_n_pronunciation(
        instance,
        token,
        &raw mut count
    ));
    assert_eq!(count, 2);
    close(context, instance);
}

/// Two tokens of one library abort the import with one of them under high
/// bits too: the assert compares masked libraries (`pinyin.cpp:552-554`).
#[test]
fn import_aborts_on_two_tokens_of_one_library_under_high_bits() {
    assert_aborts(
        "boundary_tests::library_tokens::import_aborts_on_two_tokens_of_one_library_under_high_bits",
        || {
            let user = TempUserDir::new("tagged-import-duplicate");
            let (context, instance) = open_at(&user.path);
            close(context, instance);
            set_phrase_table_row(&user.path, "你好", &[0x0700_0001, 0x0700_0002 | TAG]);
            let (context, _) = open_at(&user.path);
            import_batch(context, 7, &[("你好", "ni'hao", 5)]);
        },
    );
}

/// A prefix row keeps the token the phrase table holds
/// (`pinyin.cpp:2384-2404`; `phrase_large_table3.cpp:42-48`). Choosing it
/// adds `23 * 3 * 7 = 483` to the unigram of the item its masked library
/// and slot name (`pinyin.cpp:2608-2616`, `phrase_index.h:629`,
/// `phrase_index.cpp:154`).
#[test]
fn a_prefix_row_under_high_bits_is_listed_and_trains_its_item() {
    let user = TempUserDir::new("tagged-prefix");
    let (context, instance) = open_at(&user.path);
    assert_eq!(
        import_batch(context, 7, &[("龘", "da2", 1), ("龘龘", "da2'da2", 1)]),
        [true, true]
    );
    assert!(pinyin_save(context));
    let token = lookup(instance, "龘龘")[0];
    close(context, instance);
    set_phrase_table_row(&user.path, "龘龘", &[token | TAG]);
    let (context, instance) = open_at(&user.path);
    let (found, base) = unigram(instance, token);
    assert!(found);
    assert!(pinyin_guess_predicted_candidates(instance, c"龘".as_ptr()));
    let row = row_with(instance, PREDICTED_PREFIX, "龘");
    assert!(pinyin_choose_predicted_candidate(instance, row));
    assert_eq!(unigram(instance, token), (true, base + 483));
    close(context, instance);
}

/// A user pinyin-index record whose token has high bits set. The table
/// files it under its masked library (`chewing_large_table2.h:107`), the
/// candidate keeps its value (`pinyin.cpp:1784`) and is a user candidate
/// (`pinyin.cpp:3719`). Training stores the gram item under that value
/// with the first seed `23 * 3` and adds `69 * 7 = 483` to the unigram of
/// the item its masked library and slot name
/// (`phonetic_lookup.h:856-927`, `phrase_index.h:629`).
#[test]
fn a_candidate_under_high_bits_is_listed_and_trains_its_item() {
    let user = TempUserDir::new("tagged-candidate");
    let token = saved_user_phrase(&user.path);
    let path = user.path.join("user_pinyin_index.bin");
    let mut rows = crate::test_support::store_rows(&path, false);
    let mut retagged = 0;
    for (_, value) in &mut rows {
        for at in 0..value.len().saturating_sub(3) {
            if value[at..at + 4] == token.to_le_bytes() {
                value[at..at + 4].copy_from_slice(&(token | TAG).to_le_bytes());
                retagged += 1;
            }
        }
    }
    assert!(retagged > 0, "the index holds the token");
    craft_user_index(&path, &rows);
    let (instance, candidate) = user_row_of_dada(&user.path);
    let context = crate::instance::pinyin_get_context(instance);
    let (found, base) = unigram(instance, token);
    assert!(found);
    assert!(pinyin_choose_candidate(instance, 0, candidate) > 0);
    assert!(pinyin_guess_sentence(instance));
    assert!(crate::candidates::pinyin_train(instance, 0));
    assert_eq!(unigram(instance, token), (true, base + 483));
    {
        // SAFETY: the instance is live and this read ends before the close.
        let inst = unsafe { crate::state::instance_ref(instance) };
        assert_eq!(
            inst.core.user.bigram_successors(1).expect("the start gram"),
            [(token | TAG, 69)]
        );
    }
    close(context, instance);
}

/// A user gram item whose token has high bits set. The bigram rows keep
/// the item's token and read its phrase through `get_phrase_item`, which
/// masks the library and the slot (`pinyin.cpp:2352-2364`,
/// `phrase_index.h:647`, `phrase_index.cpp:186` at the pin `074a2219`).
/// Choosing the row adds `23 * 3 * 7 = 483` to that item's unigram
/// (`pinyin.cpp:2608-2611`, `phrase_index.h:629`) and `23 * 3 = 69` to the
/// gram item of the whole token and to the gram's total
/// (`pinyin.cpp:2627-2637`).
#[test]
fn a_bigram_row_under_high_bits_is_listed_and_trains_its_item() {
    let user = TempUserDir::new("tagged-bigram-row");
    let successor = saved_user_phrase(&user.path);
    let (context, instance) = open_at(&user.path);
    let prefixes = lookup(instance, "你");
    close(context, instance);
    let gram = gram_bytes(20, &[(successor | TAG, 20)]);
    let rows: Vec<_> = prefixes
        .iter()
        .map(|token| (token.to_le_bytes().to_vec(), gram.clone()))
        .collect();
    replace_user_bigram(&user.path, &rows);
    let (context, instance) = open_at(&user.path);
    let (found, base) = unigram(instance, successor);
    assert!(found);
    assert!(pinyin_guess_predicted_candidates(instance, c"你".as_ptr()));
    let row = row_with(instance, PREDICTED_BIGRAM, "龘龘");
    assert!(pinyin_choose_predicted_candidate(instance, row));
    assert_eq!(unigram(instance, successor), (true, base + 483));
    let trained: Vec<_> = {
        // SAFETY: the instance is live and this read ends before the close.
        let inst = unsafe { crate::state::instance_ref(instance) };
        prefixes
            .iter()
            .map(|prefix| inst.core.user.bigram_successors(*prefix).expect("a gram"))
            .filter(|items| *items != [(successor | TAG, 20)])
            .collect()
    };
    assert_eq!(trained, [[(successor | TAG, 89)]]);
    close(context, instance);
}

/// `FacadePhraseIndex::mask_out` tests a `USER_FILE` library through the
/// four-bit library index of the mask and the value, and
/// `SubPhraseIndex::mask_out` tests each token under their low 24 bits
/// (`novel_types.h:41-44`, `phrase_index.cpp:677-715`, `pinyin.cpp:1288-1291`
/// at 074a2219). The mask and value 0x10000000 have a zero library index
/// and zero low bits, so every item of the user library is removed.
#[test]
fn mask_out_ignores_the_bits_above_the_library_index() {
    let user = TempUserDir::new("mask-high-bits");
    let (context, instance) = open_at(&user.path);
    assert_eq!(import_batch(context, 7, &[("测测", "ce4'ce4", 5)]), [true]);
    assert!(unigram(instance, 0x0700_0001).0);
    assert!(pinyin_mask_out(context, 0x1000_0000, 0x1000_0000));
    assert_eq!(unigram(instance, 0x0700_0001), (false, 0));
    close(context, instance);
}

/// Imports `phrase` under `reading` into libraries 1 to 5, so the phrase
/// table search holds five tokens.
fn hold_in_five_libraries(context: *mut PinyinContext, phrase: &str, reading: &str) {
    for library in 1..=5 {
        assert_eq!(
            import_batch(context, library, &[(phrase, reading, 100_000)]),
            [true],
            "library {library}"
        );
    }
}

/// An import into an unused library index aborts on the five-token search
/// before the sub-index refusal.
#[test]
fn an_import_aborts_on_five_tokens_before_the_library_refusal() {
    assert_aborts(
        "boundary_tests::library_tokens::an_import_aborts_on_five_tokens_before_the_library_refusal",
        || {
            let user = TempUserDir::new("combined-unused-index");
            let (context, _) = open_at(&user.path);
            hold_in_five_libraries(context, "你好", "ni3'hao3");
            import_batch(context, 8, &[("你好", "ni3'hao3", 100_000)]);
        },
    );
}

/// A reading of the wrong length is refused before the five-token search.
#[test]
fn an_import_refuses_a_short_reading_before_the_token_count() {
    let user = TempUserDir::new("combined-invalid-reading");
    let (context, instance) = open_at(&user.path);
    hold_in_five_libraries(context, "你好", "ni3'hao3");
    assert_eq!(
        import_batch(context, 6, &[("你好", "ni3", 100_000)]),
        [false]
    );
    close(context, instance);
}

/// The prefix guess aborts on a prefix of five tokens (`compute_prefixes`).
#[test]
fn the_prefix_guess_aborts_on_five_tokens() {
    assert_aborts(
        "boundary_tests::library_tokens::the_prefix_guess_aborts_on_five_tokens",
        || {
            let user = TempUserDir::new("combined-prefix-guess");
            let (context, instance) = open_at(&user.path);
            hold_in_five_libraries(context, "你好", "ni3'hao3");
            crate::sentence::pinyin_guess_sentence_with_prefix(instance, c"你好".as_ptr());
        },
    );
}

/// The prediction aborts on a prefix of five tokens.
#[test]
fn the_prediction_aborts_on_five_tokens() {
    assert_aborts(
        "boundary_tests::library_tokens::the_prediction_aborts_on_five_tokens",
        || {
            let user = TempUserDir::new("combined-prediction");
            let (context, instance) = open_at(&user.path);
            hold_in_five_libraries(context, "你好", "ni3'hao3");
            pinyin_guess_predicted_candidates(instance, c"你好".as_ptr());
        },
    );
}

/// The character offset aborts on a phrase of five tokens.
#[test]
fn the_character_offset_aborts_on_five_tokens() {
    assert_aborts(
        "boundary_tests::library_tokens::the_character_offset_aborts_on_five_tokens",
        || {
            let user = TempUserDir::new("combined-character-offset");
            let (context, instance) = open_at(&user.path);
            hold_in_five_libraries(context, "你", "ni3");
            assert_eq!(parse(instance, "ni"), 2);
            let mut length = 0_usize;
            pinyin_get_character_offset(instance, c"你".as_ptr(), 0, &raw mut length);
        },
    );
}

/// `remember_user_input` aborts on a phrase of five tokens.
#[test]
fn remember_aborts_on_five_tokens() {
    assert_aborts(
        "boundary_tests::library_tokens::remember_aborts_on_five_tokens",
        || {
            let user = TempUserDir::new("combined-remember");
            let (context, instance) = open_at(&user.path);
            hold_in_five_libraries(context, "你", "ni3");
            assert_eq!(parse(instance, "ni"), 2);
            crate::user_data::pinyin_remember_user_input(instance, c"你".as_ptr(), 1);
        },
    );
}
