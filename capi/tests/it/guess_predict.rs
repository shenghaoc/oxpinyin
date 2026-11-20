//! Candidate guesses, predictions, prefix and punctuation rows and the
//! zero facade total.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::support::predicted_texts;

use crate::support::{close, export_order_fixture, init_fixture_at, partial_gram};

use crate::support::{
    DEFAULT_SORT, PinyinInstance, TempUserDir, assert_aborts, candidate, capture_rows, craft_tree,
    cstr, gram_bytes, lookup, lookup_candidate_type_t, open, pinyin_alloc_instance,
    pinyin_choose_candidate, pinyin_choose_predicted_candidate, pinyin_get_candidate_type,
    pinyin_guess_candidates, pinyin_guess_predicted_candidates,
    pinyin_guess_predicted_candidates_with_punctuations, pinyin_guess_sentence,
    pinyin_is_user_candidate, pinyin_parse_more_full_pinyins, pinyin_set_options, plant_bigram,
    private_system, system_dir, write_user_conf, *,
};

use std::ffi::c_char;

use std::path::Path;

use std::ptr;

use oxpinyin::SENTENCE_START;

/// The total of library 1 in the mini fixture's gb_char.bin.
fn library_one_total() -> u32 {
    let file = std::fs::read(system_dir().join("gb_char.bin")).expect("gb_char.bin");
    u32::from_le_bytes(file[8..12].try_into().expect("a word"))
}

/// A private mini system whose punct.bin maps each phrase's tokens to its
/// value; `lookup` reads the tokens from the fixture first.
fn punct_system(parent: &Path, rows: &[(&str, Vec<u8>)]) -> std::path::PathBuf {
    let user = TempUserDir::new("punct-tokens");
    let (context, instance) = open_at(&user.path);
    let mut table = Vec::new();
    for (text, value) in rows {
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        for token in unsafe { lookup(instance, text) } {
            table.push((token.to_le_bytes().to_vec(), value.clone()));
        }
    }
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
    let system = private_system(parent);
    craft_tree(&system.join("punct.bin"), &table);
    system
}

fn punct_words(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

/// The punctuation rows of the current list, as hex of their bytes.
///
/// # Safety
///
/// `instance` is NULL or a live instance: `pinyin_alloc_instance` returned it and
/// `pinyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
unsafe fn punct_texts(instance: *mut PinyinInstance) -> Vec<String> {
    // SAFETY: `instance` meets this function's `# Safety` contract; only this thread calls into it.
    (unsafe { capture_rows(instance) })
        .iter()
        .enumerate()
        .filter(|(_, (kind, _, _))| *kind == PREDICTED_PUNCTUATION)
        .map(|(index, _)| {
            let mut text: *const c_char = ptr::null();
            // SAFETY: `instance` meets this function's `# Safety` contract; only this thread calls
            // into it.
            let row = unsafe { candidate_at(instance, index) };
            // SAFETY: the row is from the current snapshot and text is a writable local slot.
            assert!(unsafe {
                crate::support::pinyin_get_candidate_string(instance, row, &raw mut text)
            });
            // SAFETY: a true answer points at the row's borrowed string.
            let bytes = unsafe { std::ffi::CStr::from_ptr(text) }.to_bytes();
            bytes.iter().map(|byte| format!("{byte:02x}")).collect()
        })
        .collect()
}

/// A toned initial aborts the candidate guess without the prepended rows
/// too.
#[test]
fn guess_candidates_aborts_on_a_toned_initial() {
    assert_aborts(
        "guess_predict::guess_candidates_aborts_on_a_toned_initial",
        || {
            let user = TempUserDir::new("toned-candidates");
            let (context, instance) = open_at(&user.path);
            assert!(unsafe { pinyin_set_options(context, 0x28) });
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            assert_eq!(unsafe { parse(instance, "n4") }, 2);
            // SORT_WITHOUT_SENTENCE_CANDIDATE | SORT_WITHOUT_LONGER_CANDIDATE.
            unsafe { pinyin_guess_candidates(instance, 0, 0x3) };
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
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let prefixes = unsafe { lookup(instance, "你") };
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
    let gram = gram_bytes(20, &[(successor, 20)]);
    let rows: Vec<_> = prefixes
        .iter()
        .map(|token| (token.to_le_bytes().to_vec(), gram.clone()))
        .collect();
    replace_user_bigram(&user.path, &rows);
    let (context, instance) = open_at(&user.path);
    assert!(unsafe {
        pinyin_guess_predicted_candidates_with_punctuations(instance, c"你".as_ptr())
    });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let index = unsafe { capture_rows(instance) }
        .iter()
        .position(|(kind, _, _)| *kind == PREDICTED_BIGRAM)
        .expect("a predicted bigram row");
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { capture_rows(instance) }[index].1, "龘龘");
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let row = unsafe { candidate_at(instance, index) };
    let mut kind = lookup_candidate_type_t::ZOMBIE_CANDIDATE;
    assert!(unsafe { pinyin_get_candidate_type(instance, row, &raw mut kind) });
    assert_eq!(kind, PREDICTED_BIGRAM);
    assert!(!unsafe { pinyin_is_user_candidate(instance, row) });
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// A unigram add on one item changes the order of two others, through the
/// facade total alone. `pinyin_token_add_unigram_frequency` adds the delta to
/// the facade's `guint32` total (`pinyin.cpp:2836-2842`,
/// `phrase_index.h:633`), which is the denominator of every candidate
/// frequency: `(guint32)((1 - lambda) * unigram / (gfloat) total * 2^24)`
/// with no bigram term here (`pinyin.cpp:1855-1866`).
///
/// 甲 (unigram 1) and 乙 (unigram 2) are both `ba1`; the loaded libraries
/// total 2^24 (each load adds its library's total, `phrase_index.cpp:247`). With the fixture's lambda, 0.312699, the frequencies are
/// `(guint32) 0.687` = 0 and `(guint32) 1.375` = 1, so the descending
/// frequency order (`:1700-1708`) lists 乙 first. An add of 3 * 2^24 to 丙,
/// which has another key, makes the total 2^26: both frequencies are 0, the
/// comparator answers 0, and the stable `g_array_sort_with_data` keeps the
/// order in which `_append_items` listed the tokens (`:1777-1781`): 甲, 乙.
#[test]
fn a_unigram_add_on_another_item_reorders_two_candidates() {
    // SORT_WITHOUT_SENTENCE_CANDIDATE | SORT_WITHOUT_LONGER_CANDIDATE |
    // SORT_BY_FREQUENCY.
    const BY_FREQUENCY: u32 = 0x13;
    let total = 1_u32 << 24;
    let others: u32 = ["gbk_char.bin", "opengram.bin", "merged.bin"]
        .into_iter()
        .map(|name| {
            let file = std::fs::read(system_dir().join(name)).expect("a fixture library");
            u32::from_le_bytes(file[8..12].try_into().expect("a word"))
        })
        .sum();
    let library_one = total - others;
    let scratch = tempfile::tempdir().expect("scratch");
    let system = crate::support::items_system(
        &system_dir(),
        scratch.path(),
        &[
            (&[0x7532], &[0x1081], 1),
            (&[0x4e59], &[0x1081], 2),
            (&[0x4e19], &[0x1084], library_one - 3),
        ],
        library_one,
    );
    let user = TempUserDir::new("denominator");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let instance = unsafe { alloc(context) };
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "ba1") }, 3);
    let texts = || {
        assert!(unsafe { pinyin_guess_candidates(instance, 0, BY_FREQUENCY) });
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        (unsafe { capture_rows(instance) })
            .into_iter()
            .map(|(kind, text, _)| {
                assert_eq!(kind, NORMAL);
                text
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(texts(), ["乙", "甲"]);
    assert!(unsafe {
        crate::support::pinyin_token_add_unigram_frequency(instance, 0x0100_0003, 3 * total)
    });
    assert_eq!(texts(), ["甲", "乙"]);
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// POLICY-ACCEPTED (row 106a): a removed item supplies no prefix suggestion.
#[test]
fn a_sole_removed_prefix_item_is_omitted() {
    let scratch = tempfile::tempdir().expect("scratch");
    let total = 1000_u32;
    let system = crate::support::items_system(
        &system_dir(),
        scratch.path(),
        &[
            (&[0x7532], &[0x1081], 100),
            (&[0x7532, 0x4e59], &[0x1081, 0x1081], 50),
        ],
        total,
    );
    craft_hash(&system.join("bigram.db"), &[]);
    // Token 0x01000002 as `items_system` stores it: the length and reading
    // count, the unigram, the text, the keys and the reading's count.
    let item = [
        &[2_u8, 1][..],
        &50_u32.to_le_bytes(),
        &0x7532_u32.to_le_bytes(),
        &0x4e59_u32.to_le_bytes(),
        &0x1081_u16.to_le_bytes(),
        &0x1081_u16.to_le_bytes(),
        &10_u32.to_le_bytes(),
    ]
    .concat();
    let remove = [
        &2_u32.to_le_bytes()[..],
        &0x0100_0002_u32.to_le_bytes(),
        &u16::try_from(item.len()).expect("a length").to_le_bytes(),
        &item,
    ]
    .concat();
    // A live row's string is the phrase after the prefix (`:2018-2022`).
    for removed in [false, true] {
        let user = TempUserDir::new("removed-prefix-row");
        write_user_conf(&user.path, &system);
        if removed {
            crate::support::write_log(
                &user.path,
                "gb_char.dbin",
                &[
                    crate::support::modify_header(0, total, total),
                    remove.clone(),
                ]
                .concat(),
            );
        }
        let context = init_at(&system, &user.path);
        assert!(!context.is_null());
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        let instance = unsafe { alloc(context) };
        assert!(unsafe {
            crate::support::pinyin_guess_predicted_candidates(instance, c"甲".as_ptr())
        });
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        assert_eq!(
            unsafe { n_candidate(instance) },
            u32::from(!removed),
            "removed {removed}"
        );
        if removed {
            // SAFETY: both handles are live and closing them ends this test's use.
            unsafe { close(context, instance) };
            continue;
        }
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        let row = unsafe { candidate_at(instance, 0) };
        let mut kind = lookup_candidate_type_t::ZOMBIE_CANDIDATE;
        assert!(unsafe { pinyin_get_candidate_type(instance, row, &raw mut kind) });
        assert_eq!(kind, PREDICTED_PREFIX);
        let mut string = c"seed".as_ptr();
        assert!(unsafe {
            crate::support::pinyin_get_candidate_string(instance, row, &raw mut string)
        });
        assert!(!string.is_null());
        // SAFETY: the successful getter wrote a live, NUL-terminated string.
        let found = unsafe { std::ffi::CStr::from_ptr(string) }
            .to_str()
            .expect("UTF-8");
        assert_eq!(found, "乙");
        assert!(!unsafe { pinyin_is_user_candidate(instance, row) });
        // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and
        // is not yet freed; nothing uses either handle after this call; only this thread calls into
        // them.
        unsafe { close(context, instance) };
    }
}

/// A prediction list keeps the pin's order where a frequency passes
/// `gint`'s maximum. The comparator answers `-(freq_lhs - freq_rhs)` in
/// `guint32`, read as `gint` (`pinyin.cpp:1700-1705`), under the prediction
/// sort (`:2440-2445`). With a facade total of 1, 甲乙 (unigram 200) has the
/// frequency `(guint32)(0.687 * 200 * 2^24)`, about 2.3e9 and above 2^31,
/// and 甲丙 (unigram 100) has about 1.15e9 (`:1819-1824`). Their difference
/// is below 2^31, so the wrapping order is the descending one: 甲乙 first.
/// A comparison of the two values as `gint`s would put 甲丙 first.
#[test]
fn a_prediction_list_past_gint_max_keeps_the_wrapping_order() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = crate::support::libraries_system(
        &system_dir(),
        scratch.path(),
        &[
            (
                1,
                "gb_char.bin",
                &[
                    (&[0x7532], &[0x1081], 0),
                    (&[0x7532, 0x4e19], &[0x1081, 0x1081], 100),
                    (&[0x7532, 0x4e59], &[0x1081, 0x1081], 200),
                ],
                1,
            ),
            (2, "gbk_char.bin", &[], 0),
            (3, "opengram.bin", &[], 0),
            (4, "merged.bin", &[], 0),
        ],
    );
    craft_hash(&system.join("bigram.db"), &[]);
    let user = TempUserDir::new("predict-past-gint");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let instance = unsafe { alloc(context) };
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { predicted_texts(instance, c"甲") },
        ["乙", "丙"]
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// The candidate frequency uses the `table.conf` lambda as scanned, outside
/// `[0, 1]` too. The pin scans it with `%f` into a `gfloat`
/// (`table_info.cpp:220`) and computes
/// `(guint32)((1 - lambda) * unigram / (gfloat) total * 2^24)`
/// (`pinyin.cpp:1862-1866`). With a facade total of 2^26, 乙 (unigram 1) and
/// 甲 (unigram 2) have the frequencies `(guint32) 0.25` and `(guint32) 0.5`
/// at lambda 0: a tie, listed in token order (`:1777-1781`). At lambda -2 the
/// factor is 3: `(guint32) 0.75` = 0 and `(guint32) 1.5` = 1, so 甲 comes
/// first. At lambda 1.5 the products are -0.125 and -0.25: both truncate to
/// 0, which a `guint32` holds, so the conversion is defined and the tie
/// keeps the token order. At lambda 6 the products are -1.25 and -2.5: they
/// truncate to -1 and -2, which no `guint32` holds, so the pin's conversion
/// is undefined (policy row 83, class (b)). oxpinyin stores 0 for both, as
/// Rust's `as u32` does for a negative value, and the tie keeps the token
/// order.
#[test]
fn candidates_follow_a_lambda_outside_zero_to_one() {
    // SORT_WITHOUT_SENTENCE_CANDIDATE | SORT_WITHOUT_LONGER_CANDIDATE |
    // SORT_BY_FREQUENCY.
    const BY_FREQUENCY: u32 = 0x13;
    for (lambda, order) in [
        ("0", ["乙", "甲"]),
        ("-2", ["甲", "乙"]),
        ("1.5", ["乙", "甲"]),
        ("6", ["乙", "甲"]),
    ] {
        let scratch = tempfile::tempdir().expect("scratch");
        let system = crate::support::libraries_system(
            &system_dir(),
            scratch.path(),
            &[
                (
                    1,
                    "gb_char.bin",
                    &[(&[0x4e59], &[0x1081], 1), (&[0x7532], &[0x1081], 2)],
                    1 << 26,
                ),
                (2, "gbk_char.bin", &[], 0),
                (3, "opengram.bin", &[], 0),
                (4, "merged.bin", &[], 0),
            ],
        );
        let conf = system.join("table.conf");
        let text = std::fs::read_to_string(&conf).expect("table.conf");
        assert!(text.contains("lambda parameter:0.312699"));
        std::fs::remove_file(&conf).expect("unlink the linked table.conf");
        std::fs::write(
            &conf,
            text.replace(
                "lambda parameter:0.312699",
                &format!("lambda parameter:{lambda}"),
            ),
        )
        .expect("write table.conf");
        let user = TempUserDir::new("lambda-outside");
        let context = init_at(&system, &user.path);
        assert!(!context.is_null(), "lambda {lambda}");
        // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
        let instance = unsafe { alloc(context) };
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        assert_eq!(unsafe { parse(instance, "ba1") }, 3);
        assert!(unsafe { pinyin_guess_candidates(instance, 0, BY_FREQUENCY) });
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        let texts: Vec<String> = unsafe { capture_rows(instance) }
            .into_iter()
            .map(|(_, text, _)| text)
            .collect();
        assert_eq!(texts, order, "lambda {lambda}");
        // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and
        // is not yet freed; nothing uses either handle after this call; only this thread calls into
        // them.
        unsafe { close(context, instance) };
    }
}

/// The prediction walk aborts on a gram with no item and a total left
/// over (`ngram.cpp:70`).
#[test]
fn prediction_aborts_on_an_item_less_gram_with_a_total() {
    assert_aborts(
        "guess_predict::prediction_aborts_on_an_item_less_gram_with_a_total",
        || {
            let user = TempUserDir::new("predict-empty-gram");
            let (context, instance) = open_at(&user.path);
            // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into
            // it.
            let ni = unsafe { lookup(instance, "你") };
            // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above
            // and is not yet freed; nothing uses either handle after this call; only this thread
            // calls into them.
            unsafe { close(context, instance) };
            plant_bigram(
                &user.path,
                ni.iter()
                    .map(|token| (token.to_le_bytes().to_vec(), vec![7, 0, 0, 0]))
                    .collect(),
            );
            let (_context, instance) = open_at(&user.path);
            unsafe { pinyin_guess_predicted_candidates(instance, c"你".as_ptr()) };
        },
    );
}

/// The prediction walk reads only the grams of the prefixes it reaches: a
/// short gram under another phrase is never read.
#[test]
fn prediction_reads_only_the_prefixes_it_reaches() {
    let user = TempUserDir::new("predict-unrelated-gram");
    let (context, instance) = open_at(&user.path);
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let ni = unsafe { lookup(instance, "你") };
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
    plant_bigram(
        &user.path,
        ni.iter()
            .map(|token| (token.to_le_bytes().to_vec(), vec![7, 0, 0]))
            .collect(),
    );
    let (context, instance) = open_at(&user.path);
    assert!(unsafe { pinyin_guess_predicted_candidates(instance, c"中".as_ptr()) });
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
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
        &crate::support::modify_header(0, total, u32::MAX - 483),
    );
    let (context, instance) = open_at(&user.path);
    assert!(unsafe { pinyin_guess_predicted_candidates(instance, c"你".as_ptr()) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let row = unsafe { row_of(instance, PREDICTED_PREFIX) };
    assert!(unsafe { pinyin_choose_predicted_candidate(instance, row) });
    assert!(!unsafe { pinyin_choose_predicted_candidate(instance, row) });
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// The punctuation dedup ignores the existing candidates: a punctuation
/// equal to the first prediction is listed, and the prediction stays.
#[test]
fn the_punctuation_dedup_ignores_the_candidates() {
    let user = TempUserDir::new("punct-candidate-probe");
    let (context, instance) = open_at(&user.path);
    assert!(unsafe { pinyin_guess_predicted_candidates(instance, c"你".as_ptr()) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let first = unsafe { capture_rows(instance) }[0].1.clone();
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
    let mut words: Vec<u32> = first.chars().map(u32::from).collect();
    words.extend([0, 0xFF0C, 0]);
    let scratch = tempfile::tempdir().expect("scratch");
    let system = punct_system(scratch.path(), &[("你", punct_words(&words))]);
    let user = TempUserDir::new("punct-candidate");
    let context = init_at(&system, &user.path);
    assert!(!context.is_null());
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let instance = unsafe { alloc(context) };
    assert!(unsafe {
        pinyin_guess_predicted_candidates_with_punctuations(instance, c"你".as_ptr())
    });
    let first_hex: String = first.bytes().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { punct_texts(instance) },
        [first_hex, "efbc8c".to_owned()]
    );
    assert!(
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        unsafe { capture_rows(instance) }
            .iter()
            .any(|(kind, text, _)| *kind == PREDICTED_PREFIX && *text == first)
    );
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

/// A retained prefix whose user gram has a partial item tail emits one
/// warning after replacing the previous candidate snapshot.
#[rstest::rstest]
fn partial_candidate_gram_logs_once(
    #[values("lookup", "prediction", "punctuation")] operation: &str,
) {
    let scratch = TempUserDir::new("partial-candidate-gram-token");
    let (context, instance) = open(scratch.path.to_str().expect("UTF-8 path"));
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let token = unsafe { crate::support::lookup(instance, "你好") }[0];
    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };

    let user = TempUserDir::new("partial-candidate-gram");
    crate::support::plant_bigram(
        &user.path,
        vec![(token.to_le_bytes().to_vec(), partial_gram(token))],
    );
    let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    if operation == "lookup" {
        assert!(unsafe {
            pinyin_set_options(context, oxpinyin::OptionBits::DYNAMIC_ADJUST.bits())
        });
        assert!(!unsafe {
            crate::support::pinyin_guess_sentence_with_prefix(instance, c"你好".as_ptr())
        });
        assert_eq!(
            unsafe { pinyin_parse_more_full_pinyins(instance, c"hao".as_ptr()) },
            3
        );
    }
    let logs = crate::support::LogCapture::new();
    let result = match operation {
        "lookup" => unsafe { crate::support::pinyin_guess_candidates(instance, 0, DEFAULT_SORT) },
        "prediction" => unsafe {
            crate::support::pinyin_guess_predicted_candidates(instance, c"你好".as_ptr())
        },
        _ => unsafe {
            crate::support::pinyin_guess_predicted_candidates_with_punctuations(
                instance,
                c"你好".as_ptr(),
            )
        },
    };
    assert!(!result);
    assert_eq!(
        logs.take(),
        [(Some("libpinyin".to_owned()), glib_sys::G_LOG_LEVEL_WARNING)]
    );
    let mut count = u32::MAX;
    assert!(unsafe { crate::support::pinyin_get_n_candidate(instance, &raw mut count) });
    assert_eq!(count, 0);
    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}

// ---------------------------------------------------------------------
// Ports of contract-diff cases whose pin side reads freed or
// heap-dependent memory (undefined behaviour), so no frozen pin
// reference can hold them. Each asserts oxpinyin's defined behaviour;
// the expectations come from the pin's source at 074a2219 and the
// project's rulings, not from running the pin.
// ---------------------------------------------------------------------

/// The pin's candidate getter lends `m_phrase_string` (`pinyin.cpp:2873-2877`),
/// then prediction frees it before `_compute_prefixes` reads the argument
/// (`:1335-1342`, `:2420-2424`). The pin result is undefined. Oxpinyin
/// recognizes the alias by pointer identity and returns a stable empty
/// prediction; caller-owned text still predicts normally afterward.
#[test]
fn borrowed_prediction_prefix_clears_candidates_and_owned_text_recovers() {
    let user = TempUserDir::new("borrowed-prediction-prefix");
    let (context, instance) = open(user.path.to_str().expect("UTF-8 path"));
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let borrowed_candidate = unsafe { candidate(instance, "nihao", 0) };
    let mut borrowed_text = std::ptr::null();
    assert!(unsafe {
        crate::support::pinyin_get_candidate_string(
            instance,
            borrowed_candidate,
            &raw mut borrowed_text,
        )
    });
    assert!(!borrowed_text.is_null());
    assert!(!unsafe { crate::support::pinyin_guess_predicted_candidates(instance, borrowed_text) });
    let mut count = u32::MAX;
    assert!(unsafe { crate::support::pinyin_get_n_candidate(instance, &raw mut count) });
    assert_eq!(count, 0);
    assert!(unsafe {
        crate::support::pinyin_guess_predicted_candidates(instance, cstr("你好").as_ptr())
    });
    assert!(unsafe { crate::support::pinyin_get_n_candidate(instance, &raw mut count) });
    assert_eq!(
        count, 0,
        "the mini fixture has no predicted suggestion rows"
    );
    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}

/// The punctuation prediction on a candidate's own borrowed string answers true
/// with no row and leaves the sentence (docs/findings/upstream-divergences.md,
/// "Aliased prediction prefix use-after-free").
#[test]
fn a_borrowed_punctuation_prefix_answers_true_with_no_row() {
    let user = TempUserDir::new("punct-alias");
    let (context, instance) = open_at(&user.path);
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { parse(instance, "nihao") }, 5);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let sentence = unsafe { sentence_out(instance, 0) };
    assert!(unsafe { pinyin_guess_candidates(instance, 0, crate::support::DEFAULT_SORT) });
    let mut text: *const c_char = ptr::null();
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let row = unsafe { candidate_at(instance, 0) };
    // SAFETY: the row is from the current snapshot and text is a writable local slot.
    assert!(unsafe { crate::support::pinyin_get_candidate_string(instance, row, &raw mut text) });
    assert!(unsafe { pinyin_guess_predicted_candidates_with_punctuations(instance, text) });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { n_candidate(instance) }, 0);
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    assert_eq!(unsafe { sentence_out(instance, 0) }, sentence);
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
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
    // SAFETY: `context` was opened above and is not yet freed; only this thread calls into it.
    let instance = unsafe { alloc(context) };
    assert!(unsafe {
        pinyin_guess_predicted_candidates_with_punctuations(instance, c"你好".as_ptr())
    });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let mut puncts = unsafe { punct_texts(instance) };
    puncts.sort();
    assert_eq!(puncts, ["eda080", "f4908080"]);
    // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and is
    // not yet freed; nothing uses either handle after this call; only this thread calls into them.
    unsafe { close(context, instance) };
}

#[test]
fn predicted_candidates_include_trained_bigram_successors() {
    let user_dir = TempUserDir::new("predict");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let first = unsafe { candidate(instance, "nihao", 0) };
    assert!(unsafe { pinyin_choose_candidate(instance, 0, first) } > 0);
    assert!(unsafe { crate::support::pinyin_guess_sentence(instance) });
    assert!(unsafe { crate::support::pinyin_train(instance, 0) });
    // First train already seeds 69, which is above the copied filter of 10.
    for _ in 0..4 {
        assert!(unsafe { crate::support::pinyin_reset(instance) });
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        let again = unsafe { candidate(instance, "nihao", 0) };
        assert!(unsafe { pinyin_choose_candidate(instance, 0, again) } > 0);
        assert!(unsafe { crate::support::pinyin_guess_sentence(instance) });
        assert!(unsafe { crate::support::pinyin_train(instance, 0) });
    }

    let prefix = cstr("你");
    assert!(unsafe {
        pinyin_guess_predicted_candidates_with_punctuations(instance, prefix.as_ptr())
    });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let rows = unsafe { capture_rows(instance) };
    assert!(
        !rows.is_empty(),
        "prediction must return at least the prefix suggestions or bigrams"
    );
    assert!(
        rows.iter().all(|(kind, _, _)| {
            matches!(
                kind,
                lookup_candidate_type_t::PREDICTED_BIGRAM_CANDIDATE
                    | lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE
            )
        }),
        "你 has no punct.table rows, so the list stays phrase-only"
    );

    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}

#[test]
fn predicted_punctuation_uses_shortest_suffix_first_and_dedups() {
    let user_dir = TempUserDir::new("predict-punct-suffix");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let prefix = cstr("中国");
    assert!(unsafe {
        pinyin_guess_predicted_candidates_with_punctuations(instance, prefix.as_ptr())
    });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let rows = unsafe { capture_rows(instance) };
    let puncts: Vec<&str> = rows
        .iter()
        .filter(|(kind, _, _)| *kind == lookup_candidate_type_t::PREDICTED_PUNCTUATION_CANDIDATE)
        .map(|(_, text, _)| text.as_str())
        .collect();
    // last-1 国 (，) then last-2 中国 (none). 中 is not a suffix.
    assert_eq!(puncts, ["，"]);

    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}

/// The pin slices the predicted prefix candidate's phrase string from
/// `m_begin = prefix_len` (`pinyin.cpp:1976-1980, 2018-2023`): the user
/// already committed the prefix, so the suggestion must carry only the
/// remainder — both frontends commit the library string verbatim.
#[test]
fn predicted_prefix_candidates_slice_the_prefix_from_the_text() {
    let user_dir = TempUserDir::new("predict-slice");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    let prefix = cstr("\u{4e2d}"); // 中
    assert!(unsafe {
        pinyin_guess_predicted_candidates_with_punctuations(instance, prefix.as_ptr())
    });
    // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
    let rows = unsafe { capture_rows(instance) };
    let suggestions: Vec<&str> = rows
        .iter()
        .filter(|(kind, _, _)| *kind == lookup_candidate_type_t::PREDICTED_PREFIX_CANDIDATE)
        .map(|(_, text, _)| text.as_str())
        .collect();
    assert_eq!(
        suggestions,
        ["\u{56fd}"],
        "中国 suggests 国: the prefix 中 is sliced, not part of the text"
    );

    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}

/// GLib log records the child's default handler collected:
/// `(domain, level, message)`.
static GLIB_LOG: std::sync::Mutex<Vec<(String, u32, String)>> = std::sync::Mutex::new(Vec::new());

/// Collects every GLib log record instead of printing it, so the child's
/// stderr holds only the library's raw `fprintf` lines.
///
/// # Safety
///
/// GLib calls it as a log handler: `domain` and `message` are each NULL or a NUL-terminated string
/// that stays valid for the call.
unsafe extern "C" fn collect_glib_log(
    domain: *const std::ffi::c_char,
    level: glib_sys::GLogLevelFlags,
    message: *const std::ffi::c_char,
    _data: glib_sys::gpointer,
) {
    let text = |ptr: *const std::ffi::c_char| {
        if ptr.is_null() {
            String::new()
        } else {
            // SAFETY: GLib hands NUL-terminated strings that outlive the call.
            unsafe { std::ffi::CStr::from_ptr(ptr) }
                .to_string_lossy()
                .into_owned()
        }
    };
    GLIB_LOG
        .lock()
        .expect("log lock")
        .push((text(domain), level, text(message)));
}

#[test]
fn repeated_candidate_guess_refuses_the_same_partial_predecessor() {
    let scratch = tempfile::tempdir().expect("scratch");
    let user = scratch.path();
    let (_, _, successor) = export_order_fixture();
    crate::support::plant_bigram(
        user,
        vec![(
            SENTENCE_START.to_le_bytes().to_vec(),
            partial_gram(successor),
        )],
    );
    let context = init_fixture_at(user);
    assert!(unsafe { pinyin_set_options(context, oxpinyin::OptionBits::DYNAMIC_ADJUST.bits()) });
    let instance = unsafe { pinyin_alloc_instance(context) };
    assert_eq!(
        unsafe { pinyin_parse_more_full_pinyins(instance, c"ni".as_ptr()) },
        2
    );

    // SAFETY: nextest runs this test in a process of its own, and the
    // handler needs no user data and lives for the process.
    unsafe {
        glib_sys::g_log_set_default_handler(Some(collect_glib_log), ptr::null_mut());
    }
    for _ in 0..2 {
        assert!(!unsafe { pinyin_guess_candidates(instance, 0, DEFAULT_SORT) });
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        assert!(unsafe { capture_rows(instance) }.is_empty());
        let logs = std::mem::take(&mut *GLIB_LOG.lock().expect("log lock"));
        assert_eq!(
            logs,
            [(
                "libpinyin".to_owned(),
                glib_sys::G_LOG_LEVEL_WARNING,
                "pinyin_guess_candidates: a bigram value would be read past its end (upstream reads past it)".to_owned(),
            )]
        );
    }
    unsafe { crate::support::pinyin_free_instance(instance) };
    unsafe { crate::support::pinyin_fini(context) };
}

/// The predicted ranking uses the table.conf lambda: at λ = 1 every
/// unigram term is zero, so a large count does not move its row ahead
/// of the collection order; at the fixture's λ it does.
#[test]
fn predicted_rows_take_the_table_conf_lambda() {
    let order = |lambda: Option<&str>| {
        let scratch = tempfile::tempdir().expect("scratch");
        let system = crate::support::private_system(scratch.path());
        if let Some(lambda) = lambda {
            let path = system.join("table.conf");
            let text = std::fs::read_to_string(&path).expect("table.conf");
            std::fs::remove_file(&path).expect("unlink the linked table.conf");
            let text = text.replace(
                "lambda parameter:0.312699",
                &format!("lambda parameter:{lambda}"),
            );
            std::fs::write(&path, text).expect("write table.conf");
        }
        let user = TempUserDir::new("predict-lambda");
        let system_c = crate::support::cstr(system.to_str().expect("UTF-8 path"));
        let user_c = crate::support::cstr(user.path.to_str().expect("UTF-8 path"));
        let context = unsafe { crate::support::pinyin_init(system_c.as_ptr(), user_c.as_ptr()) };
        assert!(!context.is_null());
        let instance = unsafe { crate::support::pinyin_alloc_instance(context) };
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        let before = unsafe { predicted_texts(instance, c"你") };
        assert_eq!(before.len(), 2);
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        let last = unsafe { lookup(instance, format!("你{}", before[1])) }[0];
        assert!(unsafe {
            crate::support::pinyin_token_add_unigram_frequency(instance, last, 50_000_000)
        });
        // SAFETY: `instance` was opened above and is not yet freed; only this thread calls into it.
        let after = unsafe { predicted_texts(instance, c"你") };
        // SAFETY: `context` was opened above and is not yet freed; `instance` was opened above and
        // is not yet freed; nothing uses either handle after this call; only this thread calls into
        // them.
        unsafe { close(context, instance) };
        (before, after)
    };
    let (before, after) = order(Some("1"));
    assert_eq!(after, before, "λ = 1 ranks by collection order");
    let (before, after) = order(None);
    assert_eq!(after, [before[1].clone(), before[0].clone()]);
}
