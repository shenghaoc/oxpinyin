//! C-API literals on the pinned libpinyin-data package (`tools/oracle/libpinyin-data.json`), in
//! tkrzw format: the observations of the pin capture of 2026-11-07 (tools/oracle/pin_capture.py,
//! runs 1 and 2 identical) whose values are the full tables', which the w3 mini fixture cannot
//! hold. `just fetch-contract-data` provides the package; a missing one is a failure, not a skip.

use std::path::PathBuf;
use std::ptr;

use crate::candidates::{pinyin_choose_candidate, pinyin_get_candidate, pinyin_train};
use crate::config::pinyin_set_options;
use crate::e2e_tests::{capture_rows, export_of};
use crate::instance::{pinyin_alloc_instance, pinyin_reset};
use crate::parse::pinyin_parse_more_full_pinyins;
use crate::sentence::{pinyin_get_sentence, pinyin_guess_candidates, pinyin_guess_sentence};
use crate::test_support::{DEFAULT_SORT, TempUserDir, cstr};
use crate::types::{LookupCandidate, PinyinContext, PinyinInstance, lookup_candidate_type_t};

/// `PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE | IS_PINYIN`, the capture's word.
const PARITY: u32 = 0x18a;
const NBEST: lookup_candidate_type_t = lookup_candidate_type_t::NBEST_MATCH_CANDIDATE;
const NORMAL: lookup_candidate_type_t = lookup_candidate_type_t::NORMAL_CANDIDATE;

/// Where `just fetch-contract-data` extracts the pinned version.
fn contract_data_dir() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let pin = std::fs::read_to_string(root.join("tools/oracle/libpinyin-data.json"))
        .expect("the data pin is committed");
    let version = pin
        .lines()
        .find_map(|line| line.trim().strip_prefix("\"version\": \""))
        .and_then(|rest| rest.strip_suffix("\","))
        .expect("the data pin names a version");
    let dir = root
        .join("target/contract-data")
        .join(version)
        .join("libpinyin/data");
    assert!(
        dir.join("bigram.db").is_file(),
        "pinned system data missing at {}; run just fetch-contract-data",
        dir.display()
    );
    dir
}

/// A context and instance on the pinned data with a fresh user directory.
fn open_pinned(user_dir: &TempUserDir) -> (*mut PinyinContext, *mut PinyinInstance) {
    let system = cstr(contract_data_dir().to_str().expect("UTF-8 path"));
    let user = cstr(user_dir.path.to_str().expect("UTF-8 path"));
    let context = crate::context::pinyin_init(system.as_ptr(), user.as_ptr());
    assert!(!context.is_null(), "pinyin_init opens the pinned data");
    let instance = pinyin_alloc_instance(context);
    assert!(!instance.is_null());
    (context, instance)
}

fn close(context: *mut PinyinContext, instance: *mut PinyinInstance) {
    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

fn sentence(instance: *mut PinyinInstance, index: u8) -> String {
    let mut text: *mut std::os::raw::c_char = ptr::null_mut();
    assert!(pinyin_get_sentence(instance, index, &raw mut text));
    crate::ffi::take_owned_cstr(text)
}

fn row(
    kind: lookup_candidate_type_t,
    text: &str,
    nbest: Option<u8>,
) -> (lookup_candidate_type_t, String, Option<u8>) {
    (kind, text.to_owned(), nbest)
}

fn candidate_at(instance: *mut PinyinInstance, index: u32) -> *mut LookupCandidate {
    let mut cand: *mut LookupCandidate = ptr::null_mut();
    assert!(pinyin_get_candidate(instance, index, &raw mut cand));
    cand
}

/// The first row of TEXT, of any type, as nbest-train-diff.c's `find_by_text` takes it.
fn first_row_of(instance: *mut PinyinInstance, text: &str) -> u32 {
    let index = capture_rows(instance)
        .iter()
        .position(|(_, row, _)| row == text)
        .unwrap_or_else(|| panic!("{text} is offered"));
    u32::try_from(index).expect("small index")
}

/// S4: the tenth and the last row of the 0x18a list at offset 0, chosen, then the window at the
/// cursor. A NORMAL row's choose answers `offset + add_constraint(m_begin, m_end)`, the span it
/// constrains (`pinyin.cpp:2582-2588` at 074a2219; `lookup/phonetic_lookup.cpp:61-86`): 2 for a
/// `ni` row, 3 for a whole `shi`. The window then leads with the constrained sentences, deduped
/// by string (`pinyin.cpp:2295-2300`). Observations `deep-choose/nihao@10`, `deep-choose/shi@10`,
/// `last-choose/nihao` and `last-choose/shi`.
#[test]
fn choosing_a_deep_or_last_row_constrains_its_span() {
    struct Case {
        input: &'static str,
        count: u32,
        index: u32,
        text: &'static str,
        cursor: i32,
        window: u32,
        head: Vec<(lookup_candidate_type_t, String, Option<u8>)>,
    }
    let after_ni = |first: &str| {
        let mut head = vec![
            row(NBEST, &format!("{first}好"), Some(0)),
            row(NBEST, &format!("{first}号"), Some(1)),
            row(NBEST, &format!("{first}豪"), Some(2)),
        ];
        head.extend(["好", "号", "豪", "浩", "毫"].map(|text| row(NORMAL, text, None)));
        head
    };
    let cases = [
        Case {
            input: "nihao",
            count: 126,
            index: 10,
            text: "溺",
            cursor: 2,
            window: 96,
            head: after_ni("溺"),
        },
        Case {
            input: "nihao",
            count: 126,
            index: 125,
            text: "袮",
            cursor: 2,
            window: 96,
            head: after_ni("袮"),
        },
        Case {
            input: "shi",
            count: 280,
            index: 10,
            text: "史",
            cursor: 3,
            window: 1,
            head: vec![row(NBEST, "史", Some(0))],
        },
        Case {
            input: "shi",
            count: 280,
            index: 279,
            text: "篒",
            cursor: 3,
            window: 1,
            head: vec![row(NBEST, "篒", Some(0))],
        },
    ];
    for case in cases {
        let user_dir = TempUserDir::new("pinned-deep-choose");
        let (context, instance) = open_pinned(&user_dir);
        assert!(pinyin_set_options(context, PARITY));
        let input = cstr(case.input);
        assert_eq!(
            pinyin_parse_more_full_pinyins(instance, input.as_ptr()),
            case.input.len()
        );
        assert!(pinyin_guess_sentence(instance));
        assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
        let rows = capture_rows(instance);
        assert_eq!(rows.len(), case.count as usize, "{} rows", case.input);
        assert_eq!(
            rows[case.index as usize],
            row(NORMAL, case.text, None),
            "{} row {}",
            case.input,
            case.index
        );
        let cursor = pinyin_choose_candidate(instance, 0, candidate_at(instance, case.index));
        assert_eq!(cursor, case.cursor, "{}", case.text);
        assert!(pinyin_guess_sentence(instance));
        assert!(pinyin_guess_candidates(
            instance,
            usize::try_from(cursor).expect("cursor"),
            DEFAULT_SORT
        ));
        let window = capture_rows(instance);
        assert_eq!(window.len(), case.window as usize, "{} window", case.text);
        assert_eq!(
            window[..case.head.len()],
            case.head[..],
            "{} window head",
            case.text
        );
        assert_eq!(
            sentence(instance, 0),
            case.head[0].1,
            "{} sentence",
            case.text
        );
        close(context, instance);
    }
}

/// S4: index n, one past the list, answers false with the out-param NULLed first
/// (`pinyin.cpp:2856-2859` at 074a2219). Observation `past-end/nihao`.
#[test]
fn the_row_one_past_the_list_is_refused() {
    let user_dir = TempUserDir::new("pinned-past-end");
    let (context, instance) = open_pinned(&user_dir);
    assert!(pinyin_set_options(context, PARITY));
    let input = cstr("nihao");
    assert_eq!(pinyin_parse_more_full_pinyins(instance, input.as_ptr()), 5);
    assert!(pinyin_guess_sentence(instance));
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    let mut count = 0;
    assert!(crate::candidates::pinyin_get_n_candidate(
        instance,
        &raw mut count
    ));
    assert_eq!(count, 126);
    let mut cand = ptr::NonNull::<LookupCandidate>::dangling().as_ptr();
    assert!(!pinyin_get_candidate(instance, count, &raw mut cand));
    assert!(cand.is_null());
    close(context, instance);
}

/// D8: nbest-train-diff.c's ordinary sequence at the gate's rounds (`runner_recipes.py:588`): three
/// rounds of 你 then 浩 at the cursor, then three of 你 then 好, by text, on the default options.
/// `train_result3` trains each ONESTEP token and the next: a new pair seeds 69, an existing one
/// adds `max(freq, 69) * 2` (`lookup/phonetic_lookup.h:886-903` at 074a2219), so three rounds
/// leave 621 on (你, 浩) and on (你, 好); the unigram adds the seed times 7 (`:930`). The trained
/// pairs reorder the n-best sentences (observation `nbest-train/ordinary`, runs 1 and 2
/// identical), and survive a save and reopen.
///
/// The bigram export of the trained grams is derived from source, with no capture (the 2026-11-01
/// ruling on register row 1's `g_strjoinv` misuse, `pinyin.cpp:849`, `:862`): 你's gram holds both
/// successors in token order, `insert_freq` keeping it ascending (`storage/ngram.cpp:178-193`),
/// 浩 (16779426) before 好 (16779429); each row's count is the stored 621 doubled (`pinyin.cpp:906`).
#[test]
fn ordinary_training_reorders_the_nbest_rows_and_exports_its_grams() {
    let user_dir = TempUserDir::new("pinned-nbest-train");
    let (context, instance) = open_pinned(&user_dir);
    let surface = |instance: *mut PinyinInstance| {
        let input = cstr("nihao");
        assert_eq!(pinyin_parse_more_full_pinyins(instance, input.as_ptr()), 5);
        assert!(pinyin_guess_sentence(instance));
        assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
        let rows = capture_rows(instance);
        let mut indexes: Vec<u8> = rows.iter().filter_map(|(_, _, nbest)| *nbest).collect();
        indexes.sort_unstable();
        let sentences: Vec<String> = indexes
            .iter()
            .take(3)
            .map(|&index| sentence(instance, index))
            .collect();
        assert!(pinyin_reset(instance));
        (
            rows.len(),
            rows.into_iter().take(8).collect::<Vec<_>>(),
            sentences,
        )
    };
    let train = |first: &str, second: &str| {
        let input = cstr("nihao");
        assert_eq!(pinyin_parse_more_full_pinyins(instance, input.as_ptr()), 5);
        assert!(pinyin_guess_sentence(instance));
        assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
        let cursor = pinyin_choose_candidate(
            instance,
            0,
            candidate_at(instance, first_row_of(instance, first)),
        );
        assert_eq!(cursor, 2);
        assert!(pinyin_guess_sentence(instance));
        assert!(pinyin_guess_candidates(instance, 2, DEFAULT_SORT));
        assert_eq!(
            pinyin_choose_candidate(
                instance,
                2,
                candidate_at(instance, first_row_of(instance, second))
            ),
            5
        );
        pinyin_guess_sentence(instance);
        assert!(pinyin_train(instance, 0));
        assert!(pinyin_reset(instance));
    };
    let normal = |texts: &[&str]| {
        texts
            .iter()
            .map(|&text| row(NORMAL, text, None))
            .collect::<Vec<_>>()
    };
    let mut baseline = vec![row(NBEST, "你好", Some(0))];
    baseline.extend(normal(&["你", "尼", "呢", "泥", "妮", "拟", "逆"]));
    assert_eq!(surface(instance), (126, baseline, vec!["你好".to_owned()]));
    for _ in 0..3 {
        train("你", "浩");
    }
    let mut user_only = vec![row(NBEST, "你浩", Some(0)), row(NBEST, "你好", Some(1))];
    user_only.extend(normal(&["你", "尼", "呢", "泥", "妮", "拟"]));
    assert_eq!(
        surface(instance),
        (127, user_only, vec!["你浩".to_owned(), "你好".to_owned()])
    );
    for _ in 0..3 {
        train("你", "好");
    }
    let mut augmented = vec![row(NBEST, "你好", Some(0)), row(NBEST, "你浩", Some(1))];
    augmented.extend(normal(&["你", "尼", "呢", "泥", "妮", "拟"]));
    let trained = (127, augmented, vec!["你好".to_owned(), "你浩".to_owned()]);
    assert_eq!(surface(instance), trained);
    assert_eq!(export_of(context, 7), [], "training adds no phrase");

    let mut bigrams = Vec::new();
    let iter = crate::iterators::pinyin_begin_get_bigram_phrases(context);
    let mut falses = 0;
    while falses < 2 && bigrams.len() < 16 {
        if !crate::iterators::pinyin_bigram_iterator_has_next_phrase(iter) {
            falses += 1;
            continue;
        }
        falses = 0;
        let (mut phrase, mut pinyin, mut count) = (ptr::null_mut(), ptr::null_mut(), 0);
        crate::iterators::pinyin_bigram_iterator_get_next_phrase(
            iter,
            &raw mut phrase,
            &raw mut pinyin,
            &raw mut count,
        );
        bigrams.push((
            crate::ffi::take_owned_cstr(phrase),
            crate::ffi::take_owned_cstr(pinyin),
            count,
        ));
    }
    crate::iterators::pinyin_end_get_bigram_phrases(iter);
    assert_eq!(
        bigrams,
        [
            ("你浩".to_owned(), "ni'hao".to_owned(), 1242),
            ("你好".to_owned(), "ni'hao".to_owned(), 1242),
        ]
    );

    assert!(crate::context::pinyin_save(context));
    close(context, instance);
    let (context, instance) = open_pinned(&user_dir);
    assert_eq!(surface(instance), trained, "after a save and reopen");
    close(context, instance);
}

/// D5: `pinyin.cpp:1711-1758,2200-2215` at 074a2219 selects the prior
/// result token and conditionally merges its bigram for dynamic adjustment.
#[test]
fn dynamic_adjust_ranks_the_window_after_a_chosen_token() {
    for (options, expected) in [
        (
            0x18a,
            [
                "矣", "移", "蛇", "遗", "毅", "疑", "佚", "驿", "姨", "贻", "轶", "薏", "翌", "饴",
            ],
        ),
        (
            0x38a,
            [
                "遗", "矣", "移", "蛇", "疑", "毅", "驿", "佚", "贻", "姨", "薏", "轶", "饴", "翌",
            ],
        ),
    ] {
        let user = TempUserDir::new("dynamic-chosen-de");
        let (context, instance) = open_pinned(&user);
        assert!(pinyin_set_options(context, options));
        assert_eq!(
            pinyin_parse_more_full_pinyins(instance, c"deyi".as_ptr()),
            4
        );
        assert_eq!(crate::parse::pinyin_get_parsed_input_length(instance), 4);
        assert!(pinyin_guess_sentence(instance));
        assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
        let initial = capture_rows(instance);
        assert_eq!(initial.len(), 33);
        assert_eq!(initial[6], row(NORMAL, "的", None));
        assert_eq!(
            pinyin_choose_candidate(instance, 0, candidate_at(instance, 6)),
            2
        );
        assert!(pinyin_guess_sentence(instance));
        assert!(pinyin_guess_candidates(instance, 2, DEFAULT_SORT));
        let rows = capture_rows(instance);
        assert_eq!(rows.len(), 599);
        assert_eq!(
            &rows[..3],
            &[
                row(NBEST, "的一", Some(0)),
                row(NBEST, "的以", Some(2)),
                row(NORMAL, "一", None)
            ]
        );
        for (index, text) in [23, 24, 25, 26, 32, 33, 38, 39, 52, 53, 70, 71, 75, 76]
            .into_iter()
            .zip(expected)
        {
            assert_eq!(
                rows[index],
                row(NORMAL, text, None),
                "options {options:#x}, row {index}"
            );
        }
        close(context, instance);
    }
}

/// D9: selecting an n-best row applies its constraints against result zero
/// (`pinyin.cpp:2515-2520`; `lookup/phonetic_lookup.h:864-870` at 074a2219).
#[test]
fn choosing_a_nonzero_nbest_row_trains_the_selected_result() {
    let user = TempUserDir::new("nonzero-nbest");
    let (context, instance) = open_pinned(&user);
    assert!(pinyin_set_options(context, PARITY));
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, c"nihao".as_ptr()),
        5
    );
    assert_eq!(crate::parse::pinyin_get_parsed_input_length(instance), 5);
    assert!(pinyin_guess_sentence(instance));
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    let rows = capture_rows(instance);
    assert_eq!(rows.len(), 126);
    assert_eq!(rows[1], row(NORMAL, "你", None));
    assert_eq!(
        pinyin_choose_candidate(instance, 0, candidate_at(instance, 1)),
        2
    );
    assert!(pinyin_guess_sentence(instance));
    assert!(pinyin_guess_candidates(instance, 2, DEFAULT_SORT));
    let rows = capture_rows(instance);
    assert_eq!(rows.len(), 95);
    assert_eq!(
        &rows[..2],
        &[row(NBEST, "你好", Some(0)), row(NBEST, "你号", Some(2))]
    );
    assert_eq!(
        pinyin_choose_candidate(instance, 2, candidate_at(instance, 1)),
        5
    );
    assert!(pinyin_guess_sentence(instance));
    assert_eq!(sentence(instance, 0), "你号");
    assert!(pinyin_train(instance, 0));
    assert!(crate::phrase::pinyin_phrase_segment(
        instance,
        c"你号".as_ptr()
    ));
    let mut count = 0;
    assert!(crate::phrase::pinyin_get_n_phrase(instance, &raw mut count));
    assert_eq!(count, 2);
    for (index, expected_token, expected_frequency) in [(0, 16781861, 53370), (1, 16779448, 32211)]
    {
        let mut token = 0;
        assert!(crate::phrase::pinyin_get_phrase_token(
            instance,
            index,
            &raw mut token
        ));
        assert_eq!(token, expected_token);
        let mut frequency = 0;
        assert!(crate::dict::pinyin_token_get_unigram_frequency(
            instance,
            token,
            &raw mut frequency
        ));
        assert_eq!(frequency, expected_frequency);
    }
    close(context, instance);
}

/// D10: `pinyin.cpp:2678-2688` at 074a2219 asserts that the train index is
/// below the number of decoded results before training it.
#[test]
fn training_the_last_two_nbest_results_succeeds() {
    for index in [1, 2] {
        let user = TempUserDir::new("train-index");
        let (context, instance) = open_pinned(&user);
        assert_eq!(
            pinyin_parse_more_full_pinyins(instance, c"nihao".as_ptr()),
            5
        );
        assert_eq!(crate::parse::pinyin_get_parsed_input_length(instance), 5);
        assert!(pinyin_guess_sentence(instance));
        assert!(pinyin_train(instance, index));
        close(context, instance);
    }
}

#[test]
fn training_one_past_the_nbest_results_aborts() {
    crate::test_support::assert_aborts(
        "pinned_data_tests::training_one_past_the_nbest_results_aborts",
        || {
            let user = TempUserDir::new("train-index-past-end");
            let (_context, instance) = open_pinned(&user);
            assert_eq!(
                pinyin_parse_more_full_pinyins(instance, c"nihao".as_ptr()),
                5
            );
            assert_eq!(crate::parse::pinyin_get_parsed_input_length(instance), 5);
            assert!(pinyin_guess_sentence(instance));
            pinyin_train(instance, 3);
        },
    );
}
