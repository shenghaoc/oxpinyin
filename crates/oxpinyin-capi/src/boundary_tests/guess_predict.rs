//! Candidate guesses, predictions, prefix and punctuation rows and the
//! zero facade total.

use super::*;

/// Adds the negation of the loaded facade total to token 0x01000001, so
/// the total the model divides by wraps to zero (register row 61).
fn wrap_facade_total_to_zero(instance: *mut PinyinInstance) {
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let total = unsafe { crate::state::instance_ref(instance) }
        .core
        .lm
        .amplified_total();
    let total = u32::try_from(total).expect("a guint32 total");
    assert!(pinyin_token_add_unigram_frequency(
        instance,
        0x0100_0001,
        total.wrapping_neg()
    ));
    // SAFETY: as above.
    let after = unsafe { crate::state::instance_ref(instance) }
        .core
        .lm
        .amplified_total();
    assert_eq!(after, 0, "the facade total wraps to zero");
}

/// A toned initial aborts the candidate guess without the prepended rows
/// too.
#[test]
fn guess_candidates_aborts_on_a_toned_initial() {
    assert_aborts(
        "boundary_tests::guess_predict::guess_candidates_aborts_on_a_toned_initial",
        || {
            let user = TempUserDir::new("toned-candidates");
            let (context, instance) = open_at(&user.path);
            assert!(pinyin_set_options(context, 0x28));
            assert_eq!(parse(instance, "n4"), 2);
            // SORT_WITHOUT_SENTENCE_CANDIDATE | SORT_WITHOUT_LONGER_CANDIDATE.
            pinyin_guess_candidates(instance, 0, 0x3);
        },
    );
}

/// A predicted bigram row whose successor is a user phrase is not a user
/// candidate: `is_user_candidate` tests the type first.
#[test]
fn a_predicted_bigram_row_is_not_a_user_candidate() {
    let user = TempUserDir::new("predicted-is-user");
    let successor = saved_user_phrase(&user.path);
    let (context, instance) = open_at(&user.path);
    let prefixes = lookup(instance, "你");
    close(context, instance);
    let gram = gram_bytes(20, &[(successor, 20)]);
    let rows: Vec<_> = prefixes
        .iter()
        .map(|token| (token.to_le_bytes().to_vec(), gram.clone()))
        .collect();
    replace_user_bigram(&user.path, &rows);
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_guess_predicted_candidates_with_punctuations(
        instance,
        c"你".as_ptr()
    ));
    let index = capture_rows(instance)
        .iter()
        .position(|(kind, _, _)| *kind == PREDICTED_BIGRAM)
        .expect("a predicted bigram row");
    assert_eq!(capture_rows(instance)[index].1, "龘龘");
    let row = candidate_at(instance, index);
    let mut kind = lookup_candidate_type_t::ZOMBIE_CANDIDATE;
    assert!(pinyin_get_candidate_type(instance, row, &raw mut kind));
    assert_eq!(kind, PREDICTED_BIGRAM);
    assert!(!pinyin_is_user_candidate(instance, row));
    close(context, instance);
}

/// With the facade total wrapped to zero, the candidate guess aborts in
/// the scorer (register row 61).
#[test]
fn guess_candidates_aborts_on_a_zero_facade_total() {
    assert_aborts(
        "boundary_tests::guess_predict::guess_candidates_aborts_on_a_zero_facade_total",
        || {
            let user = TempUserDir::new("facade-total-zero");
            let (_context, instance) = open_at(&user.path);
            wrap_facade_total_to_zero(instance);
            assert_eq!(parse(instance, "nihao"), 5);
            pinyin_guess_candidates(instance, 0, crate::test_support::DEFAULT_SORT);
        },
    );
}

/// A guess that ranks no word row never reaches the scorer: true with no
/// row on a zero facade total, mid-key (`sentence_rows_only`) and at the
/// reserved end slot, where the search loop does not run
/// (`pinyin.cpp:2230`).
#[test]
fn a_guess_with_no_word_row_ranks_nothing_on_a_zero_total() {
    let user = TempUserDir::new("facade-total-zero-no-rank");
    let (context, _) = open_at(&user.path);
    let logs = crate::test_support::LogCapture::new();
    for (offset, sort) in [(3, 3), (5, 2)] {
        let instance = alloc(context);
        assert_eq!(parse(instance, "nihao"), 5);
        wrap_facade_total_to_zero(instance);
        assert!(pinyin_guess_candidates(instance, offset, sort), "{offset}");
        assert_eq!(n_candidate(instance), 0, "{offset}");
        pinyin_free_instance(instance);
    }
    assert!(logs.take().is_empty());
    pinyin_fini(context);
}

/// Predicted bigram rows on a zero facade total abort in the scorer
/// (register row 61).
#[test]
fn predicted_bigram_rows_abort_on_a_zero_facade_total() {
    assert_aborts(
        "boundary_tests::guess_predict::predicted_bigram_rows_abort_on_a_zero_facade_total",
        || {
            let user = TempUserDir::new("row58-total-zero");
            let (context, instance) = open_at(&user.path);
            let (ni, hao) = (lookup(instance, "你"), lookup(instance, "好")[0]);
            close(context, instance);
            let gram = gram_bytes(20, &[(hao, 20)]);
            plant_bigram(
                &user.path,
                ni.iter()
                    .map(|token| (token.to_le_bytes().to_vec(), gram.clone()))
                    .collect(),
            );
            let (_context, instance) = open_at(&user.path);
            wrap_facade_total_to_zero(instance);
            pinyin_guess_predicted_candidates(instance, c"你".as_ptr());
        },
    );
}

/// The prediction walk aborts on a gram with no item and a total left
/// over (`ngram.cpp:70`).
#[test]
fn prediction_aborts_on_an_item_less_gram_with_a_total() {
    assert_aborts(
        "boundary_tests::guess_predict::prediction_aborts_on_an_item_less_gram_with_a_total",
        || {
            let user = TempUserDir::new("predict-empty-gram");
            let (context, instance) = open_at(&user.path);
            let ni = lookup(instance, "你");
            close(context, instance);
            plant_bigram(
                &user.path,
                ni.iter()
                    .map(|token| (token.to_le_bytes().to_vec(), vec![7, 0, 0, 0]))
                    .collect(),
            );
            let (_context, instance) = open_at(&user.path);
            pinyin_guess_predicted_candidates(instance, c"你".as_ptr());
        },
    );
}

/// The prediction walk reads only the grams of the prefixes it reaches: a
/// short gram under another phrase is never read.
#[test]
fn prediction_reads_only_the_prefixes_it_reaches() {
    let user = TempUserDir::new("predict-unrelated-gram");
    let (context, instance) = open_at(&user.path);
    let ni = lookup(instance, "你");
    close(context, instance);
    plant_bigram(
        &user.path,
        ni.iter()
            .map(|token| (token.to_le_bytes().to_vec(), vec![7, 0, 0]))
            .collect(),
    );
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_guess_predicted_candidates(instance, c"中".as_ptr()));
    close(context, instance);
}

/// Choosing a predicted prefix row whose unigram add would pass the
/// `guint32` limit answers false and leaves the count.
#[test]
fn a_prefix_choose_past_the_unigram_limit_is_refused() {
    let user = TempUserDir::new("row58-prefix-overflow");
    let (context, instance) = open_at(&user.path);
    // PINYIN_INCOMPLETE | DYNAMIC_ADJUST | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE | IS_PINYIN.
    assert!(pinyin_set_options(context, 0x38a));
    assert_eq!(
        import_batch(
            context,
            7,
            &[("我", "wo3", 1), ("我甲甲", "wo3'jia3'jia3", 1_431_655_631)]
        ),
        [true, true]
    );
    let token = lookup(instance, "我甲甲")[0];
    assert_eq!(unigram(instance, token), (true, 4_294_966_893));
    assert!(pinyin_guess_predicted_candidates(instance, c"我".as_ptr()));
    let row = row_with(instance, PREDICTED_PREFIX, "甲甲");
    assert!(!pinyin_choose_predicted_candidate(instance, row));
    assert_eq!(unigram(instance, token), (true, 4_294_966_893));
    close(context, instance);
}

/// The total of library 1 in the mini fixture's gb_char.bin.
fn library_one_total() -> u32 {
    let file = std::fs::read(system_dir().join("gb_char.bin")).expect("gb_char.bin");
    u32::from_le_bytes(file[8..12].try_into().expect("a word"))
}

/// A predicted prefix choose of a library-1 row answers false where the
/// library total cannot take the unigram delta: the sub-index overflow
/// (`phrase_index.cpp:78`).
#[test]
fn a_prefix_choose_past_the_library_total_is_refused() {
    let user = TempUserDir::new("predicted-library-overflow");
    write_user_conf(&user.path, &system_dir());
    let total = library_one_total();
    write_log(
        &user.path,
        "gb_char.dbin",
        &crate::test_support::modify_header(0, total, u32::MAX - 483),
    );
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_guess_predicted_candidates(instance, c"你".as_ptr()));
    let row = row_of(instance, PREDICTED_PREFIX);
    assert!(pinyin_choose_predicted_candidate(instance, row));
    assert!(!pinyin_choose_predicted_candidate(instance, row));
    close(context, instance);
}

/// The punctuation prediction on a candidate's own borrowed string answers true
/// with no row and leaves the sentence (docs/findings/upstream-divergences.md,
/// "Aliased prediction prefix use-after-free").
#[test]
fn a_borrowed_punctuation_prefix_answers_true_with_no_row() {
    let user = TempUserDir::new("punct-alias");
    let (context, instance) = open_at(&user.path);
    assert_eq!(parse(instance, "nihao"), 5);
    assert!(pinyin_guess_sentence(instance));
    let sentence = sentence_out(instance, 0);
    assert!(pinyin_guess_candidates(
        instance,
        0,
        crate::test_support::DEFAULT_SORT
    ));
    let mut text: *const c_char = ptr::null();
    assert!(crate::candidates::pinyin_get_candidate_string(
        instance,
        candidate_at(instance, 0),
        &raw mut text
    ));
    assert!(pinyin_guess_predicted_candidates_with_punctuations(
        instance, text
    ));
    assert_eq!(n_candidate(instance), 0);
    assert_eq!(sentence_out(instance, 0), sentence);
    close(context, instance);
}

/// A private mini system whose punct.bin maps each phrase's tokens to its
/// value; `lookup` reads the tokens from the fixture first.
fn punct_system(parent: &Path, rows: &[(&str, Vec<u8>)]) -> std::path::PathBuf {
    let user = TempUserDir::new("punct-tokens");
    let (context, instance) = open_at(&user.path);
    let mut table = Vec::new();
    for (text, value) in rows {
        for token in lookup(instance, text) {
            table.push((token.to_le_bytes().to_vec(), value.clone()));
        }
    }
    close(context, instance);
    let system = private_system(parent);
    craft_tree(&system.join("punct.bin"), &table);
    system
}

fn punct_words(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

/// The punctuation rows of the current list, as hex of their bytes.
fn punct_texts(instance: *mut PinyinInstance) -> Vec<String> {
    capture_rows(instance)
        .iter()
        .enumerate()
        .filter(|(_, (kind, _, _))| *kind == PREDICTED_PUNCTUATION)
        .map(|(index, _)| {
            let mut text: *const c_char = ptr::null();
            assert!(crate::candidates::pinyin_get_candidate_string(
                instance,
                candidate_at(instance, index),
                &raw mut text
            ));
            // SAFETY: a true answer points at the row's borrowed string.
            let bytes = unsafe { std::ffi::CStr::from_ptr(text) }.to_bytes();
            bytes.iter().map(|byte| format!("{byte:02x}")).collect()
        })
        .collect()
}

/// Punctuations of non-scalar words dedup by their bytes, across the
/// prefixes.
#[test]
fn non_scalar_punctuations_dedup_by_their_bytes() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = punct_system(
        scratch.path(),
        &[
            ("好", punct_words(&[0xD800, 0, 0x11_0000, 0])),
            ("你好", punct_words(&[0x11_0000, 0, 0xD800, 0])),
        ],
    );
    let user = TempUserDir::new("punct-non-scalar");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert!(pinyin_guess_predicted_candidates_with_punctuations(
        instance,
        c"你好".as_ptr()
    ));
    let mut puncts = punct_texts(instance);
    puncts.sort();
    assert_eq!(puncts, ["eda080", "f4908080"]);
    close(context, instance);
}

/// The punctuation dedup ignores the existing candidates: a punctuation
/// equal to the first prediction is listed, and the prediction stays.
#[test]
fn the_punctuation_dedup_ignores_the_candidates() {
    let user = TempUserDir::new("punct-candidate-probe");
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_guess_predicted_candidates(instance, c"你".as_ptr()));
    let first = capture_rows(instance)[0].1.clone();
    close(context, instance);
    let mut words: Vec<u32> = first.chars().map(u32::from).collect();
    words.extend([0, 0xFF0C, 0]);
    let scratch = tempfile::tempdir().expect("scratch");
    let system = punct_system(scratch.path(), &[("你", punct_words(&words))]);
    let user = TempUserDir::new("punct-candidate");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    let instance = alloc(context);
    assert!(pinyin_guess_predicted_candidates_with_punctuations(
        instance,
        c"你".as_ptr()
    ));
    let first_hex: String = first.bytes().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(punct_texts(instance), [first_hex, "efbc8c".to_owned()]);
    assert!(
        capture_rows(instance)
            .iter()
            .any(|(kind, text, _)| *kind == PREDICTED_PREFIX && *text == first)
    );
    close(context, instance);
}

/// The prediction walks the user phrase table by raw key bytes and takes
/// the row's text from the live item: a key 你 + U+110000 lists 龘龘's
/// suffix 龘.
#[test]
fn a_raw_user_key_predicts_its_live_item() {
    let user = TempUserDir::new("raw-key-prediction");
    let token = saved_user_phrase(&user.path);
    craft_user_index(
        &user.path.join("user_phrase_index.bin"),
        &[
            (ucs4("你"), Vec::new()),
            (
                [ucs4("你"), 0x0011_0000_u32.to_le_bytes().to_vec()].concat(),
                token.to_le_bytes().to_vec(),
            ),
        ],
    );
    write_user_conf(&user.path, &system_dir());
    let (context, instance) = open_at(&user.path);
    assert!(pinyin_guess_predicted_candidates(instance, c"你".as_ptr()));
    assert!(
        capture_rows(instance)
            .iter()
            .any(|(kind, text, _)| *kind == PREDICTED_PREFIX && text == "龘")
    );
    close(context, instance);
}
