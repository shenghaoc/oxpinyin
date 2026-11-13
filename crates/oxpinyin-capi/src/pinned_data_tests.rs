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

#[path = "../../oxpinyin-runtime/tests/common/contract_data.rs"]
mod contract_data;
use contract_data::contract_data_dir;

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

/// The first row of TEXT, of any type, as `pinyin_choose_candidate` takes a row by text (`pinyin.cpp:2501-2589` at 074a2219).
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

/// D8: the n-best training sequence (`pinyin.cpp:1372-1387`, `:2184-2308`, `:2501-2589`, `:2670-2691` at 074a2219): three
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

// ── C-boundary decisions that need the full tables ─────────────────────────

/// A context on the pinned data and `user`; NULL when init refuses.
fn init_pinned_at(system: &std::path::Path, user: &std::path::Path) -> *mut PinyinContext {
    let system = cstr(system.to_str().expect("UTF-8 path"));
    let user = cstr(user.to_str().expect("UTF-8 path"));
    crate::context::pinyin_init(system.as_ptr(), user.as_ptr())
}

/// A private system under `parent`: every pinned file linked in.
fn private_pinned_system(parent: &std::path::Path) -> PathBuf {
    let dir = parent.join("system");
    std::fs::create_dir_all(&dir).expect("private system dir");
    for entry in std::fs::read_dir(contract_data_dir()).expect("pinned data") {
        let entry = entry.expect("pinned entry");
        if entry.file_type().expect("file type").is_file() {
            crate::test_support::link_file(&entry.path(), &dir.join(entry.file_name()));
        }
    }
    dir
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The rows of a user container as hex pairs, sorted.
fn hex_rows(path: &std::path::Path, hash: bool) -> Vec<(String, String)> {
    crate::test_support::store_rows(path, hash)
        .iter()
        .map(|(key, value)| (hex(key), hex(value)))
        .collect()
}

fn pairs(rows: &[(&str, &str)]) -> Vec<(String, String)> {
    rows.iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

fn import(context: *mut PinyinContext, library: u8, phrase: &str, reading: &str, count: i32) {
    assert_eq!(
        crate::e2e_tests::import_batch(context, library, &[(phrase, reading, count)]),
        [true]
    );
}

fn unigram(instance: *mut PinyinInstance, token: u32) -> (bool, u32) {
    let mut freq = 0x00AB_CDEF;
    let ret = crate::dict::pinyin_token_get_unigram_frequency(instance, token, &raw mut freq);
    (ret, freq)
}

/// A first chunk header word and the record counts of a `.dbin` journal
/// (`PhraseIndexLogger::next_record`, `phrase_index_logger.h:158-211`):
/// `(first header totals, [header, add, remove, modify])`.
fn journal_walk(data: &[u8]) -> (Option<(u32, u32)>, [u32; 4]) {
    let payload = &data[8..];
    let word = |at: usize| u32::from_le_bytes(payload[at..at + 4].try_into().expect("a word"));
    let half = |at: usize| usize::from(u16::from_le_bytes([payload[at], payload[at + 1]]));
    let (mut at, mut header, mut counts) = (0, None, [0; 4]);
    while at + 10 <= payload.len() {
        match word(at) {
            4 => {
                header.get_or_insert((word(at + 10), word(at + 14)));
                counts[0] += 1;
                at += 10 + 2 * half(at + 8);
            }
            kind @ (1 | 2) => {
                counts[kind as usize] += 1;
                at += 10 + half(at + 8);
            }
            3 => {
                counts[3] += 1;
                at += 12 + half(at + 8) + half(at + 10);
            }
            _ => break,
        }
    }
    (header, counts)
}

/// A save over a library replaced by a chunk whose layout fails diffs against
/// an empty sub-index: true, no GLib critical, and every original item is
/// journalled as added
/// (docs/findings/upstream-divergences.md#phrase-library-layout-faults-write-no-critical).
#[test]
fn a_save_over_a_failed_layout_journals_against_an_empty_index() {
    let run = crate::test_support::run_in_child(
        "pinned_data_tests::a_save_over_a_failed_layout_journals_against_an_empty_index",
        |user| {
            let system = private_pinned_system(user.parent().expect("scratch"));
            let context = init_pinned_at(&system, user);
            assert!(!context.is_null());
            let instance = pinyin_alloc_instance(context);
            assert_eq!(
                pinyin_parse_more_full_pinyins(instance, c"nihao".as_ptr()),
                5
            );
            assert!(pinyin_guess_sentence(instance));
            assert!(pinyin_train(instance, 0));
            crate::instance::pinyin_free_instance(instance);
            let mut payload = [0_u8; 32];
            for (at, value) in [(0, 123_u32), (4, 17), (8, 20), (12, 24)] {
                payload[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
            payload[16] = b'#';
            payload[19] = b'!';
            let library = system.join("merged.bin");
            let staged = system.join("merged.bin.new");
            std::fs::write(
                &staged,
                oxpinyin_data::chunk_format::build_memory_chunk(&payload).expect("frame"),
            )
            .expect("write the replacement");
            std::fs::rename(&staged, &library).expect("replace merged.bin");
            assert!(crate::context::pinyin_save(context));
        },
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(!stderr.contains("CRITICAL"), "{stderr}");
    let journal = std::fs::read(run.user_dir.join("merged.dbin")).expect("merged.dbin");
    assert_eq!(journal.len(), 38_252);
    let digest = {
        use sha2::Digest as _;
        hex(&sha2::Sha256::digest(&journal))
    };
    assert_eq!(
        digest,
        "59eed2f9ef43edc1f6e62b72a6f83c6ac2b83e4a56f9956a19c0452dce92a34a"
    );
    assert_eq!(journal_walk(&journal), (Some((123, 1001)), [1, 1001, 0, 0]));
}

/// The pin's saved profile (#929) opens with its logical records: the
/// three user containers' rows and the user tokens, and a save after a
/// count-zero import of a known phrase changes no record.
#[test]
fn the_pins_saved_profile_opens_with_its_records() {
    let user = TempUserDir::new("save-929-cross-open");
    crate::test_support::write_pin_profile("save-929-logical-cross-open-pinyin", &user.path);
    let logs = crate::test_support::LogCapture::new();
    let context = init_pinned_at(&contract_data_dir(), &user.path);
    assert!(!context.is_null());
    let rows = || {
        [
            hex_rows(&user.path.join("user_bigram.db"), true),
            hex_rows(&user.path.join("user_phrase_index.bin"), false),
            hex_rows(&user.path.join("user_pinyin_index.bin"), false),
        ]
    };
    let before = rows();
    assert_eq!(
        before[0],
        pairs(&[("01000007", "250000000200000725000000")])
    );
    assert_eq!(
        before[1],
        pairs(&[
            ("969f0000", ""),
            ("969f0000969f0000", "02000007"),
            ("989f0000", ""),
            ("989f0000989f0000", "01000007"),
        ])
    );
    assert_eq!(
        before[2],
        pairs(&[
            ("0400", ""),
            ("04000400", "0100000784208420"),
            ("1200", ""),
            ("12001200", "0200000792409240"),
            ("8400", ""),
            ("84008400", "0100000784208420"),
            ("9200", ""),
            ("92009200", "0200000792409240"),
        ])
    );
    let instance = pinyin_alloc_instance(context);
    let tokens = ["龘龘", "龖龖", "靐靐"].map(|text| crate::test_support::lookup(instance, text));
    assert_eq!(tokens, [vec![0x0700_0001], vec![0x0700_0002], vec![]]);
    crate::instance::pinyin_free_instance(instance);
    import(context, 7, "龘龘", "da2'da2", 0);
    assert!(crate::context::pinyin_save(context));
    assert_eq!(rows(), before);
    crate::context::pinyin_fini(context);
    assert!(logs.take().is_empty());
}

/// The three user grams of #1114 whose keys collide in one TinyDBM bucket.
fn collision_rows() -> Vec<(Vec<u8>, Vec<u8>)> {
    [
        (0x0100_0d46_u32, 1_u32),
        (0x0100_4404, 6),
        (0x0100_4dd8, 100),
    ]
    .into_iter()
    .map(|(token, degree)| {
        let items: Vec<(u32, u32)> = (0x0100_0001..0x0100_0001 + degree)
            .map(|item| (item, 69))
            .collect();
        (
            token.to_le_bytes().to_vec(),
            crate::test_support::gram_bytes(69 * degree, &items),
        )
    })
    .collect()
}

/// Saved HashDBM bytes with the timestamp (`META_OFFSET_TIMESTAMP`, bytes
/// 48 to 56) zeroed.
fn masked(raw: &[u8]) -> Vec<u8> {
    assert!(raw.len() >= 128, "the HashDBM metadata");
    let mut masked = raw.to_vec();
    masked[48..56].fill(0);
    masked
}

/// Two cycles on the profile under `user`: an import, a save whose user
/// bigram keeps `rows` (and, with `original`, its masked bytes), a fini and
/// a reinit. Answers the size and masked SHA-256 of each save.
fn collision_cycles(
    user: &std::path::Path,
    rows: &[(Vec<u8>, Vec<u8>)],
    original: Option<&[u8]>,
) -> Vec<(usize, String)> {
    let path = user.join("user_bigram.db");
    let expected: Vec<(String, String)> = {
        let mut expected: Vec<_> = rows
            .iter()
            .map(|(key, value)| (hex(key), hex(value)))
            .collect();
        expected.sort();
        expected
    };
    let mut context = init_pinned_at(&contract_data_dir(), user);
    assert!(!context.is_null());
    let mut saves = Vec::new();
    for (text, reading) in [("达达", "da2'da2"), ("你你", "ni3'ni3")] {
        import(context, 7, text, reading, 100);
        assert!(crate::context::pinyin_save(context));
        let raw = std::fs::read(&path).expect("the saved bigram");
        if let Some(original) = original {
            assert_eq!(
                masked(&raw),
                masked(original),
                "the reload changed the saved bytes"
            );
        }
        assert_eq!(hex_rows(&path, true), expected);
        let digest = {
            use sha2::Digest as _;
            hex(&sha2::Sha256::digest(masked(&raw)))
        };
        saves.push((raw.len(), digest));
        crate::context::pinyin_fini(context);
        context = init_pinned_at(&contract_data_dir(), user);
        assert!(!context.is_null());
    }
    crate::context::pinyin_fini(context);
    saves
}

/// Colliding user grams (#1114) survive loads and saves: the saved user
/// bigram keeps the rows and equals the pin's bytes outside the timestamp,
/// and the pin's own profile reloads and saves to the same bytes.
#[test]
fn colliding_user_grams_survive_load_and_save() {
    let rows = collision_rows();
    let own = TempUserDir::new("issue1114-own");
    let context = init_pinned_at(&contract_data_dir(), &own.path);
    assert!(!context.is_null());
    crate::context::pinyin_fini(context);
    crate::test_support::craft_hash(&own.path.join("user_bigram.db"), &rows);
    crate::test_support::write_user_conf(&own.path, &contract_data_dir());
    let saved = (
        4_199_312,
        "44042431c088b786e90ee01b493aa01661912dda345a8cc5f758a41c682f7c22".to_owned(),
    );
    assert_eq!(
        collision_cycles(&own.path, &rows, None),
        [saved.clone(), saved]
    );

    let donor = TempUserDir::new("issue1114-pin");
    crate::test_support::write_pin_profile("issue1114-collision-load-save-pinyin", &donor.path);
    let bigram = |dir: &std::path::Path| std::fs::read(dir.join("user_bigram.db")).expect("bigram");
    assert!(masked(&bigram(&donor.path)) == masked(&bigram(&own.path)));
    let original = bigram(&donor.path);
    let logs = crate::test_support::LogCapture::new();
    collision_cycles(&donor.path, &rows, Some(&original));
    assert!(logs.take().is_empty());
}

/// The user.conf versions are the host scanner's `%d`: a value 2^32 above
/// the table.conf version converts to it on every host (oxpinyin-input
/// native.rs `fscanf`), so the profile conforms and keeps its trained and
/// imported state; one more makes it non-conforming and the init wipes it
/// (#920).
#[test]
fn user_conf_versions_past_int_take_the_scanners_int() {
    let session = |user: &std::path::Path, extra: i64| -> [(bool, u32); 2] {
        let context = init_pinned_at(&contract_data_dir(), user);
        assert!(!context.is_null());
        let instance = pinyin_alloc_instance(context);
        let trained = 0x0100_3e57;
        assert_eq!(crate::test_support::lookup(instance, "癫痫"), [trained]);
        assert_eq!(
            pinyin_parse_more_full_pinyins(instance, c"dian4xian4".as_ptr()),
            10
        );
        assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
        let index = capture_rows(instance)
            .iter()
            .position(|(kind, text, _)| *kind == NORMAL && text == "癫痫")
            .expect("a NORMAL 癫痫 row");
        let cand = candidate_at(instance, u32::try_from(index).expect("small index"));
        assert!(pinyin_choose_candidate(instance, 0, cand) > 0);
        assert!(pinyin_guess_sentence(instance));
        assert!(pinyin_train(instance, 0));
        assert_eq!(unigram(instance, trained), (true, 550));
        import(context, 7, "你好你好", "ni3'hao3'ni3'hao3", 9);
        let imported = crate::test_support::lookup(instance, "你好你好")
            .into_iter()
            .find(|token| token >> 24 == 7)
            .expect("the imported token");
        assert_eq!(unigram(instance, imported), (true, 27));
        assert!(crate::context::pinyin_save(context));
        close(context, instance);
        let conf_path = user.join("user.conf");
        let conf = std::fs::read_to_string(&conf_path).expect("user.conf");
        let edited: String = conf
            .split_inclusive('\n')
            .map(|line| {
                for key in ["binary format version:", "model data version:"] {
                    if let Some(rest) = line.strip_prefix(key) {
                        let value: i64 = rest.trim().parse().expect("a version");
                        return format!("{key}{}\n", value + (1 << 32) + extra);
                    }
                }
                line.to_owned()
            })
            .collect();
        std::fs::write(&conf_path, edited).expect("write user.conf");
        let context = init_pinned_at(&contract_data_dir(), user);
        assert!(!context.is_null());
        let instance = pinyin_alloc_instance(context);
        let after = [unigram(instance, trained), unigram(instance, imported)];
        close(context, instance);
        after
    };
    let conforms = TempUserDir::new("user-conf-920-conforms");
    assert_eq!(session(&conforms.path, 0), [(true, 550), (true, 27)]);
    let wiped = TempUserDir::new("user-conf-920-nonconforms");
    assert_eq!(session(&wiped.path, 1), [(true, 67), (false, 0)]);
}

/// A system chunk's total and its items by slot (`_system_items`).
fn system_items(name: &str) -> (u32, std::collections::BTreeMap<u32, Vec<u8>>) {
    let bytes = std::fs::read(contract_data_dir().join(name)).expect("a system library");
    let payload = &bytes[8..];
    let word = |at: usize| u32::from_le_bytes(payload[at..at + 4].try_into().expect("a word"));
    let (total, index, content) = (word(0), word(4) as usize, word(8) as usize);
    let mut items = std::collections::BTreeMap::new();
    for slot in 0..(content - index - 1) / 4 {
        let offset = word(index + slot * 4);
        if offset != 0 {
            let start = content + offset as usize;
            let (length, prons) = (usize::from(payload[start]), usize::from(payload[start + 1]));
            let size = 6 + 4 * length + prons * (2 * length + 4);
            let end = (start + size).min(payload.len());
            items.insert(
                u32::try_from(slot).expect("a slot"),
                payload[start..end].to_vec(),
            );
        }
    }
    (total, items)
}

/// The reading key bytes of each pronunciation of a stored item.
fn item_readings(item: &[u8]) -> Vec<&[u8]> {
    let (length, prons) = (usize::from(item[0]), usize::from(item[1]));
    let start = 6 + 4 * length;
    let stride = 2 * length + 4;
    (0..prons)
        .map(|nth| {
            let at = (start + stride * nth).min(item.len());
            &item[at..(at + 2 * length).min(item.len())]
        })
        .collect()
}

/// A merged-library item removed by the journal adds no candidate, and the
/// search over its reading answers normally
/// (docs/findings/compatibility-policy.md, row 102).
#[test]
fn a_journal_removed_item_adds_no_candidate() {
    let (total, items) = system_items("merged.bin");
    let mut longer = std::collections::BTreeSet::new();
    for name in ["merged.bin", "opengram.bin", "gb_char.bin", "gbk_char.bin"] {
        for item in system_items(name).1.values() {
            if item[0] > 4 {
                longer.extend(
                    item_readings(item)
                        .iter()
                        .map(|reading| reading[..8.min(reading.len())].to_vec()),
                );
            }
        }
    }
    let (slot, item) = items
        .iter()
        .find(|(_, item)| item[0] == 4 && item[1] == 1 && !longer.contains(item_readings(item)[0]))
        .expect("a four-character item no longer phrase extends");
    let token = 0x0400_0000 | slot;
    let user = TempUserDir::new("removed-candidate");
    let (context, instance) = open_pinned(&user);
    let (mut length, mut text) = (0_u32, ptr::null_mut());
    assert!(crate::dict::pinyin_token_get_phrase(
        instance,
        token,
        &raw mut length,
        &raw mut text
    ));
    let phrase = crate::ffi::take_owned_cstr(text);
    let reading: String = item_readings(item)[0]
        .chunks(2)
        .map(|key| {
            let mut key = u16::from_le_bytes([key[0], key[1]]);
            let mut out = ptr::null_mut();
            assert!(crate::cursor::pinyin_get_pinyin_string(
                instance,
                (&raw mut key).cast(),
                &raw mut out
            ));
            crate::ffi::take_owned_cstr(out)
        })
        .collect();
    close(context, instance);
    crate::test_support::write_user_conf(&user.path, &contract_data_dir());
    let mut remove = 2_u32.to_le_bytes().to_vec();
    remove.extend(token.to_le_bytes());
    remove.extend(u16::try_from(item.len()).expect("a length").to_le_bytes());
    remove.extend(item);
    let log = [crate::test_support::modify_header(0, total, total), remove].concat();
    std::fs::write(
        user.path.join("merged.dbin"),
        oxpinyin_data::chunk_format::build_memory_chunk(&log).expect("frame"),
    )
    .expect("write the journal");
    let (context, instance) = open_pinned(&user);
    let (mut length, mut text) = (0_u32, ptr::null_mut());
    assert!(!crate::dict::pinyin_token_get_phrase(
        instance,
        token,
        &raw mut length,
        &raw mut text
    ));
    assert_eq!((length, text.is_null()), (0, true));
    let input = cstr(&reading);
    assert_eq!(
        pinyin_parse_more_full_pinyins(instance, input.as_ptr()),
        reading.len()
    );
    assert!(pinyin_guess_sentence(instance));
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    assert!(
        !capture_rows(instance)
            .iter()
            .any(|(kind, text, _)| *kind == NORMAL && *text == phrase),
        "{phrase} is listed"
    );
    close(context, instance);
}

/// The segmenter reads an unsorted sentence-start user gram as stored
/// (docs/findings/upstream-divergences.md#stored-gram-partitions-phrase-segmentation).
#[test]
fn the_segmenter_reads_an_unsorted_user_gram_as_stored() {
    let user = TempUserDir::new("unsorted-segment");
    let (context, instance) = open_pinned(&user);
    close(context, instance);
    let gram = crate::test_support::gram_bytes(
        (1 << 31) + 2,
        &[(16_800_245, 1 << 31), (16_800_252, 1), (16_800_244, 1)],
    );
    crate::test_support::craft_hash(
        &user.path.join("user_bigram.db"),
        &[(1_u32.to_le_bytes().to_vec(), gram)],
    );
    crate::test_support::write_user_conf(&user.path, &contract_data_dir());
    let (context, instance) = open_pinned(&user);
    assert!(crate::phrase::pinyin_phrase_segment(
        instance,
        c"今天天气很好".as_ptr()
    ));
    let mut count = 0;
    assert!(crate::phrase::pinyin_get_n_phrase(instance, &raw mut count));
    let tokens: Vec<u32> = (0..count)
        .map(|index| {
            let mut token = 0;
            assert!(crate::phrase::pinyin_get_phrase_token(
                instance,
                index,
                &raw mut token
            ));
            token
        })
        .collect();
    assert_eq!(
        tokens,
        [16_800_245, 0, 16_810_538, 0, 16_779_492, 16_779_429]
    );
    close(context, instance);
}
