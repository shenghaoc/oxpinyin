//! The row58_ranking family's case bodies: each answers the result document the old case returned.
//!
//! Inventory row 57 (#1175, #697, #712, register row 58): the predicted
//! candidates' ranking (`pinyin.cpp:1812-1866`, `:2418-2438`) under seeded
//! bigrams, PREFIX choices, score ties, large counts, wrapped totals, lambda
//! edits and system-library imports.

use std::ffi::c_char;

use crate::facade;
use crate::fixtures::private_system;
use crate::readers::{
    c, candidate_at, candidate_text, n_candidate, row58_rows, tokens_of, unigram_of,
};
use crate::support::{J, Kit, P, borrowed_text};

/// `FACADE_TOTAL`: the pinned data's loaded libraries, summed.
const FACADE_TOTAL: u64 = 51_051_831;

/// The prediction option word, with DYNAMIC_ADJUST when `dynamic`.
fn options(dynamic: bool) -> u32 {
    0x18a | if dynamic { 1 << 9 } else { 0 }
}

/// `row58_rows` as its document.
fn rows(k: &mut Kit, prefix: &str) -> J {
    let predicted = row58_rows(k, prefix);
    obj! { "ret" => predicted.ret, "rows" => predicted.rows }
}

/// `choose_text`: guesses at `offset` and chooses the first row reading `wanted`.
fn choose_text(inst: P, offset: usize, wanted: &str) -> Option<i32> {
    facade::guess_candidates(inst, offset, 0);
    (0..n_candidate(inst)).find_map(|index| {
        let cand = candidate_at(inst, index).unwrap_or(std::ptr::null_mut());
        (candidate_text(inst, cand).as_deref() == Some(wanted))
            .then(|| facade::choose_candidate(inst, offset, cand))
    })
}

/// `row58_seed_bigrams`: trains 我北京 once and 我上海 three times, then boosts 北京.
fn seed_bigrams(k: &mut Kit) -> J {
    facade::set_options(k.ctx(), 0x18a | (1 << 9));
    for (text, phrase, repeats) in [("wobeijing", "北京", 1), ("woshanghai", "上海", 3)] {
        let inst = k.inst();
        assert_eq!(
            facade::parse_more_full_pinyins(inst, c(text).as_ptr()),
            text.len()
        );
        assert!(facade::guess_sentence(inst));
        let offset = choose_text(inst, 0, "我");
        assert_eq!(offset, Some(2));
        let end = choose_text(inst, 2, phrase);
        assert_eq!(end, Some(i32::try_from(text.len()).expect("a length")));
        assert!(facade::guess_sentence(inst));
        for _ in 0..repeats {
            assert!(facade::train_top(inst));
        }
        assert!(facade::reset(inst));
    }
    let (beijing, shanghai) = (tokens_of(k, "北京")[0], tokens_of(k, "上海")[0]);
    assert!(facade::token_add_unigram_frequency(
        k.inst(),
        beijing,
        100_000
    ));
    obj! { "北京" => unigram_of(k, beijing), "上海" => unigram_of(k, shanghai) }
}

/// `row58_choose`: chooses the first predicted row of `kind` reading `text` `times` times.
fn choose(k: &mut Kit, kind: i32, text: &str, times: usize) -> Vec<bool> {
    let inst = k.inst();
    let mut count = 0_u32;
    assert!(facade::get_n_candidate(inst, &raw mut count));
    for index in 0..count {
        let mut cand: P = std::ptr::null_mut();
        assert!(facade::get_candidate(inst, index, &raw mut cand));
        let mut found = 0_i32;
        assert!(facade::get_candidate_type(inst, cand, &raw mut found));
        let mut string: *const c_char = std::ptr::null();
        assert!(facade::get_candidate_string(inst, cand, &raw mut string));
        if found == kind && borrowed_text(string).as_deref() == Some(text) {
            return (0..times)
                .map(|_| facade::choose_predicted_candidate(inst, cand))
                .collect();
        }
    }
    panic!("missing predicted row: {kind} {text}");
}

/// `row58_import`: imports the rows into library 7, each add required.
fn import(k: &mut Kit, rows: &[(&str, &str, i32)]) {
    let it = facade::begin_add_phrases(k.ctx(), 7);
    assert!(!it.is_null());
    for (text, reading, count) in rows {
        assert!(facade::iterator_add_phrase(
            it,
            c(text).as_ptr(),
            c(reading).as_ptr(),
            *count
        ));
    }
    facade::end_add_phrases(it);
}

fn scoring(k: &mut Kit, dynamic: bool) -> J {
    let counts = seed_bigrams(k);
    assert!(facade::set_options(k.ctx(), options(dynamic)));
    obj! { "unigrams" => counts, "prediction" => rows(k, "我") }
}

fn prefix(k: &mut Kit, text: &str, times: usize) -> J {
    assert!(facade::set_options(k.ctx(), options(true)));
    let token = tokens_of(k, &format!("我{text}"))[0];
    let before = rows(k, "我");
    let count_before = unigram_of(k, token);
    let chosen = choose(k, 5, text, times);
    obj! {
        "before" => before,
        "count_before" => count_before,
        "chosen" => chosen,
        "count_after" => unigram_of(k, token),
        "after" => rows(k, "我"),
    }
}

fn bigram_choices(k: &mut Kit, dynamic: bool) -> J {
    let counts = seed_bigrams(k);
    assert!(facade::set_options(k.ctx(), options(dynamic)));
    let mut out = obj! { "unigrams" => counts, "before" => rows(k, "我") };
    for times in [1, 20, 200] {
        out.set(format!("chosen-{times}"), choose(k, 4, "上海", times));
        out.set(format!("rows-{times}"), rows(k, "我"));
    }
    out
}

fn conversion(k: &mut Kit) -> J {
    assert!(facade::set_options(k.ctx(), options(true)));
    import(
        k,
        &[
            ("我乙乙", "wo3'yi3'yi3", 33),
            ("我甲甲", "wo3'jia3'jia3", 33),
            ("我丙丙", "wo3'bing3'bing3", 1),
            ("我丁丁", "wo3'ding1'ding1", 2),
        ],
    );
    let mut out = obj! { "ties" => rows(k, "我") };
    import(
        k,
        &[
            ("我戊戊", "wo3'wu4'wu4", 5_592_405),
            ("我己己", "wo3'ji3'ji3", 5_592_406),
        ],
    );
    out.set("float-edge", rows(k, "我"));
    let mut unigrams = J::map();
    for text in ["我戊戊", "我己己"] {
        let token = tokens_of(k, text)[0];
        unigrams.set(text, unigram_of(k, token));
    }
    out.set("unigrams", unigrams);
    out
}

/// A private system whose table.conf has `re.sub('lambda parameter:[^\n]+', ...)`
/// applied; the kit's context opens on it.
fn lambda_system(k: &mut Kit, lambda: &str) {
    let system = private_system(k);
    let conf = system.join("table.conf");
    let text = std::fs::read_to_string(&conf).expect("table.conf");
    let edited: String = text
        .split_inclusive('\n')
        .map(|line| {
            let Some(at) = line.find("lambda parameter:") else {
                return line.to_owned();
            };
            let body = line.trim_end_matches('\n');
            let tail = &body[at + "lambda parameter:".len()..];
            if tail.is_empty() {
                return line.to_owned();
            }
            format!(
                "{}lambda parameter:{lambda}{}",
                &body[..at],
                &line[body.len()..]
            )
        })
        .collect();
    std::fs::remove_file(&conf).expect("unlink table.conf");
    std::fs::write(&conf, edited).expect("write table.conf");
    k.data = system;
    k.ctx();
}

fn lambda_ties(k: &mut Kit) -> J {
    lambda_system(k, "1");
    assert!(facade::set_options(k.ctx(), options(true)));
    let token = tokens_of(k, "我是谁")[0];
    assert!(facade::token_add_unigram_frequency(
        k.inst(),
        token,
        100_000
    ));
    obj! { "prediction" => rows(k, "我") }
}

fn prefix_overflow(k: &mut Kit) -> J {
    assert!(facade::set_options(k.ctx(), options(true)));
    import(
        k,
        &[("我", "wo3", 1), ("我甲甲", "wo3'jia3'jia3", 1_431_655_631)],
    );
    let token = tokens_of(k, "我甲甲")[0];
    let before = rows(k, "我");
    let count_before = unigram_of(k, token);
    let chosen = choose(k, 5, "甲甲", 1);
    obj! {
        "before" => before,
        "count_before" => count_before,
        "chosen" => chosen,
        "count_after" => unigram_of(k, token),
        "after" => rows(k, "我"),
    }
}

fn signed_score(k: &mut Kit, dynamic: bool, duplicate: bool, denominator: u64) -> J {
    assert!(facade::set_options(k.ctx(), options(dynamic)));
    let (phrase, reading) = if duplicate {
        ("我的心", "wo3'de5'xin1")
    } else {
        ("我甲甲", "wo3'jia3'jia3")
    };
    import(k, &[(phrase, reading, 1_431_655_631)]);
    let wrap = 1_u64 << 32;
    let total = (FACADE_TOTAL + 1_431_655_631 * 3) % wrap;
    let delta = u32::try_from((denominator + wrap - total) % wrap).expect("a delta");
    let added = facade::token_add_unigram_frequency(k.inst(), 0x01ff_ffff, delta);
    obj! { "absent_add" => added, "prediction" => rows(k, "我") }
}

fn zero_total(k: &mut Kit, punct: bool) -> J {
    let mut tokens = std::collections::BTreeSet::new();
    for text in ["我", "北京", "上海"] {
        tokens.extend(tokens_of(k, text));
    }
    let sum = |k: &mut Kit| -> u64 {
        tokens
            .iter()
            .map(|&token| u64::from(unigram_of(k, token).1))
            .sum()
    };
    let before = sum(k);
    seed_bigrams(k);
    let after = sum(k);
    assert!(facade::set_options(k.ctx(), options(true)));
    let total = FACADE_TOTAL + after - before;
    let wrap = 1_u64 << 32;
    let delta = u32::try_from((wrap - total % wrap) % wrap).expect("a delta");
    let inst = k.inst();
    let added = facade::token_add_unigram_frequency(inst, 0x01ff_ffff, delta);
    let ret = if punct {
        facade::guess_predicted_candidates_with_punctuations(inst, c("我").as_ptr())
    } else {
        facade::guess_predicted_candidates(inst, c("我").as_ptr())
    };
    let mut count = 0_u32;
    assert!(facade::get_n_candidate(inst, &raw mut count));
    if !ret {
        assert_eq!(count, 0);
    }
    let shown: Vec<&str> = if count == 0 {
        vec![]
    } else {
        vec!["not empty"]
    };
    obj! { "add" => added, "ret" => ret, "rows" => shown }
}

fn prefix_zero_total(k: &mut Kit) -> J {
    assert!(facade::set_options(k.ctx(), options(true)));
    let delta = u32::try_from((1_u64 << 32) - FACADE_TOTAL).expect("a delta");
    let added = facade::token_add_unigram_frequency(k.inst(), (1 << 24) | 1, delta);
    obj! { "add" => added, "prediction" => rows(k, "我") }
}

fn lambda_rounding(k: &mut Kit) -> J {
    lambda_system(k, "0.9999999701976776123036");
    assert!(facade::set_options(k.ctx(), options(true)));
    import(k, &[("我甲甲", "wo3'jia3'jia3", 1_431_655_631)]);
    obj! { "prediction" => rows(k, "我") }
}

/// Imports `phrase` at count 33 into `library`; answers the observations and its token.
fn system_import_rows(k: &mut Kit, library: u32, existing: bool) -> (J, u32) {
    let phrase = if existing { "我是谁" } else { "我甲甲" };
    let reading = match (facade::ZHUYIN, existing) {
        (false, true) => "wo3'shi4'shei2",
        (false, false) => "wo3'jia3'jia3",
        (true, true) => "ㄨㄛˇ ㄕˋ ㄕㄟˊ",
        (true, false) => "ㄨㄛˇ ㄐㄧㄚˇ ㄐㄧㄚˇ",
    };
    if !facade::ZHUYIN {
        assert!(facade::set_options(k.ctx(), options(true)));
    }
    let initial = tokens_of(k, phrase);
    let target = if existing { initial[0] >> 24 } else { library };
    assert!((1..=4).contains(&target));
    let before: Vec<u32> = initial.into_iter().filter(|t| t >> 24 == target).collect();
    assert_eq!(!before.is_empty(), existing);
    let it = facade::begin_add_phrases(k.ctx(), u8::try_from(target).expect("a library"));
    assert!(!it.is_null());
    let added = facade::iterator_add_phrase(it, c(phrase).as_ptr(), c(reading).as_ptr(), 33);
    assert!(added);
    facade::end_add_phrases(it);
    let tokens: Vec<u32> = tokens_of(k, phrase)
        .into_iter()
        .filter(|t| t >> 24 == target)
        .collect();
    assert_eq!(tokens.len(), 1);
    let token = tokens[0];
    let out = obj! {
        "tokens_before" => before,
        "added" => added,
        "tokens_after" => tokens,
        "imported_count" => unigram_of(k, token),
    };
    (out, token)
}

fn system_import(k: &mut Kit, library: u32, existing: bool) -> J {
    let (mut out, token) = system_import_rows(k, library, existing);
    if facade::ZHUYIN {
        return out;
    }
    if !existing {
        let rival = tokens_of(k, "我是谁")[0];
        let rival_count: u32 = (33 * 3 + 3 * 483) * 3 / 2;
        let delta = rival_count.wrapping_sub(unigram_of(k, rival).1);
        assert!(facade::token_add_unigram_frequency(k.inst(), rival, delta));
        out.set("rival_count", unigram_of(k, rival));
    }
    out.set("rows-0", rows(k, "我"));
    let suffix = if existing { "是谁" } else { "甲甲" };
    for accepted in [1, 2] {
        out.set(format!("chosen-{accepted}"), choose(k, 5, suffix, accepted));
        out.set(format!("count-{accepted}"), unigram_of(k, token));
        out.set(format!("rows-{accepted}"), rows(k, "我"));
    }
    out
}

/// Runs the case `id`.
pub fn case(k: &mut Kit, id: &str) -> J {
    if let Some(library) = id.strip_prefix("row58-system-import-zhuyin-").or_else(|| {
        id.strip_prefix("row58-system-import-")
            .and_then(|rest| rest.strip_suffix("-on"))
    }) {
        return system_import(k, library.parse().expect("a library"), false);
    }
    match id {
        "row58-bigram-scoring-off" => scoring(k, false),
        "row58-bigram-scoring-on" => scoring(k, true),
        "row58-prefix-no-bigram-on" => prefix(k, "是", 1),
        "row58-prefix-live-21-on" => prefix(k, "是谁", 21),
        "row58-bigram-choices-off" => bigram_choices(k, false),
        "row58-bigram-choices-on" => bigram_choices(k, true),
        "row58-conversions-on" => conversion(k),
        "row58-lambda-ties-on" => lambda_ties(k),
        "row58-prefix-overflow-on" => prefix_overflow(k),
        "row58-signed-score-on" => signed_score(k, true, false, 20_000_000),
        "row58-signed-duplicate-on" => signed_score(k, true, true, 20_000_000),
        "row58-unsigned-score-off" => signed_score(k, false, false, 10_000_000),
        "row58-unsigned-score-on" => signed_score(k, true, false, 10_000_000),
        "row58-total-zero-on" => zero_total(k, false),
        "row58-total-zero-punct-on" => zero_total(k, true),
        "row58-prefix-zero-total-on" => prefix_zero_total(k),
        "row58-lambda-rounding-on" => lambda_rounding(k),
        "row58-system-existing-on" | "row58-system-existing-zhuyin" => system_import(k, 4, true),
        other => panic!("no case {other}"),
    }
}
