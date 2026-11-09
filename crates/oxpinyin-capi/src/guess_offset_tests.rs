//! The dbff264 lookup-offset law through the C ABI.
//!
//! ibus-libpinyin ≥ 1.16.1 passes `pinyin_guess_candidates` the raw begin
//! of the next key rest, which can sit one position past the zero-key `'`
//! separator run (ibus-libpinyin issue #570). The guess normalizes the
//! caller offset back across the run, refuses — never aborts — when the
//! run leads the input, and a choose at the caller offset still advances.

use std::ptr;

use crate::candidates::{pinyin_choose_candidate, pinyin_clear_constraint, pinyin_get_candidate};
use crate::config::{
    pinyin_set_double_pinyin_scheme, pinyin_set_full_pinyin_scheme, pinyin_set_zhuyin_scheme,
};
use crate::parse::{
    pinyin_get_parsed_input_length, pinyin_parse_more_chewings, pinyin_parse_more_double_pinyins,
    pinyin_parse_more_full_pinyins,
};
use crate::sentence::pinyin_guess_candidates;
use crate::sentence::pinyin_guess_sentence;
use crate::state::instance_ref;
use crate::test_support::{DEFAULT_SORT, TempUserDir, cstr, open};
use crate::types::{LookupCandidate, PinyinInstance};

/// The current snapshot's candidate texts, in list order.
fn texts(instance: *mut PinyinInstance) -> Vec<String> {
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let inst = unsafe { instance_ref(instance) };
    inst.state
        .candidates
        .iter()
        .map(|c| c.text_str().expect("UTF-8 candidate").to_owned())
        .collect()
}

/// The snapshot position of the first candidate with `text`.
fn position_of(instance: *mut PinyinInstance, text: &str) -> usize {
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let inst = unsafe { instance_ref(instance) };
    inst.state
        .candidates
        .iter()
        .position(|c| c.text_str() == Ok(text))
        .unwrap_or_else(|| panic!("{text:?} is offered"))
}

/// Borrowed pointer to the snapshot candidate at `index`.
fn candidate_at(instance: *mut PinyinInstance, index: usize) -> *mut LookupCandidate {
    let mut cand: *mut LookupCandidate = ptr::null_mut();
    assert!(
        pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small candidate index"),
            &raw mut cand,
        ),
        "candidate {index} exists"
    );
    assert!(!cand.is_null());
    cand
}

fn parse(instance: *mut PinyinInstance, input: &str) -> usize {
    let text = cstr(input);
    pinyin_parse_more_full_pinyins(instance, text.as_ptr())
}

#[test]
fn guess_offset_normalizes_across_the_separator_run() {
    let (input, post_run, zero_start) = ("ni'hao", 3usize, 2usize);
    let user_dir = TempUserDir::new("guess-zero-run");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    assert_eq!(parse(instance, input), input.len(), "{input} parses whole");
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    let ni = candidate_at(instance, position_of(instance, "\u{4f60}"));
    assert_eq!(
        pinyin_choose_candidate(instance, 0, ni),
        2,
        "choosing \u{4f60} consumes the first group"
    );

    // ibus's raw begin of the next key rest (one past the run) and the
    // run's first zero-key byte answer the same list.
    assert!(pinyin_guess_candidates(instance, post_run, DEFAULT_SORT));
    let after_run = texts(instance);
    assert!(
        !after_run.is_empty(),
        "{input}: the remaining group offers candidates"
    );
    assert!(pinyin_guess_candidates(instance, zero_start, DEFAULT_SORT));
    assert_eq!(
        texts(instance),
        after_run,
        "{input}: both caller offsets normalize to the same lookup"
    );

    // The scan anchor never moved to the normalized offset, so a choose
    // at the caller offset still advances past it.
    let hao = candidate_at(instance, position_of(instance, "\u{597d}"));
    let cursor = pinyin_choose_candidate(instance, post_run, hao);
    assert!(
        cursor > i32::try_from(post_run).expect("small input offset"),
        "{input}: choose at the post-separator offset advances (got {cursor})"
    );

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn a_post_separator_choose_returns_the_candidate_end() {
    // The ibus selectCandidate idiom: `cursor = choose(...); if (cursor ==
    // length) commit; else key_rest(cursor)`. The post-separator choose
    // must answer the candidate's absolute end — caller offset plus the
    // separator-inclusive span would count the run twice, answer
    // parsed length + 1, skip the commit branch and send ibus into
    // key_rest(NULL) territory.
    let user_dir = TempUserDir::new("guess-zero-choose-end");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    assert_eq!(parse(instance, "ni'hao"), 6);
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    let ni = candidate_at(instance, position_of(instance, "\u{4f60}"));
    assert_eq!(pinyin_choose_candidate(instance, 0, ni), 2);
    // ibus's caller offset: the raw begin of the next key rest, one past
    // the separator.
    assert!(pinyin_guess_candidates(instance, 3, DEFAULT_SORT));
    let hao = candidate_at(instance, position_of(instance, "\u{597d}"));
    let cursor = pinyin_choose_candidate(instance, 3, hao);
    assert_eq!(
        cursor, 6,
        "the post-separator choose answers the absolute end, not 3 + the \
         separator-inclusive span"
    );
    let parsed = pinyin_get_parsed_input_length(instance);
    assert!(
        usize::try_from(cursor).expect("non-negative cursor") <= parsed,
        "the cursor never overshoots"
    );
    assert_eq!(
        usize::try_from(cursor).expect("non-negative cursor"),
        parsed,
        "the ibus idiom lands in the commit branch (cursor == length), \
         not a further key_rest"
    );

    // Mid-input the end is below the parse length, so a clamp could not
    // fake it — driven on the composition the R5 revert (register #8)
    // keeps alive: the re-parse of "ni'hao'a" CONTINUES the committed
    // composition (upstream's parse path never touches the store,
    // `pinyin.cpp:1497-1517`), so the \u{4f60} and \u{597d} forcings
    // survive — each probe answers true exactly because they did, and
    // dropping both re-opens the walk at 0. (The pre-revert rule
    // re-parsed this shape fresh: the probes answered false and the
    // head window answered \u{4f60} at offset 0.)
    assert_eq!(parse(instance, "ni'hao'a"), 8);
    assert!(
        pinyin_clear_constraint(instance, 0),
        "the \u{4f60} forcing survived the committed re-parse"
    );
    assert!(
        pinyin_clear_constraint(instance, 3),
        "the \u{597d} forcing survived the committed re-parse"
    );
    assert!(pinyin_guess_candidates(instance, 3, DEFAULT_SORT));
    let hao = candidate_at(instance, position_of(instance, "\u{597d}"));
    assert_eq!(
        pinyin_choose_candidate(instance, 3, hao),
        6,
        "the mid-input end is the group's own boundary, not the parse length"
    );
    // The walk continues from ibus's next post-separator begin and commits
    // exactly at the parse length.
    assert!(pinyin_guess_candidates(instance, 7, DEFAULT_SORT));
    let rest = candidate_at(instance, 0);
    let cursor = pinyin_choose_candidate(instance, 7, rest);
    assert_eq!(
        usize::try_from(cursor).expect("non-negative cursor"),
        pinyin_get_parsed_input_length(instance),
        "the final choose lands the commit branch"
    );

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn double_pinyin_admits_no_zero_key_and_keeps_the_range_law() {
    let user_dir = TempUserDir::new("guess-zero-double");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    // ZRM: ni = n+i, hao = h+k.
    assert!(pinyin_set_double_pinyin_scheme(context, 1));

    // The separator never enters a double composition — the parse stops at
    // it (upstream instead asserts the input carries none at all,
    // `pinyin_parser2.cpp:629`) — so no zero-key column can exist and
    // there is nothing to normalize.
    let split = cstr("ni'hk");
    assert_eq!(
        pinyin_parse_more_double_pinyins(instance, split.as_ptr()),
        2
    );

    // The range half of the law holds in original double coordinates.
    let both = cstr("nihk");
    assert_eq!(pinyin_parse_more_double_pinyins(instance, both.as_ptr()), 4);
    assert!(
        pinyin_guess_candidates(instance, 4, DEFAULT_SORT),
        "the one-past-end offset is the reserved slot"
    );
    assert!(
        pinyin_guess_candidates(instance, 5, DEFAULT_SORT),
        "one past the reserved slot finds nothing, as at the pin"
    );
    // Offset 6 and beyond abort the process, as at the pin (row 66).

    // The choose cursor is the absolute end in original coordinates: the
    // transformed separator the session inserts between the keys never
    // leaks into it, and the walk lands the commit branch.
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    let ni = candidate_at(instance, position_of(instance, "\u{4f60}"));
    assert_eq!(pinyin_choose_candidate(instance, 0, ni), 2);
    assert!(pinyin_guess_candidates(instance, 2, DEFAULT_SORT));
    let hao = candidate_at(instance, position_of(instance, "\u{597d}"));
    let cursor = pinyin_choose_candidate(instance, 2, hao);
    assert_eq!(cursor, 4, "the second group ends at its own original end");
    assert_eq!(
        usize::try_from(cursor).expect("non-negative cursor"),
        pinyin_get_parsed_input_length(instance),
        "the final choose lands the commit branch"
    );

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn zhuyin_keyboards_admit_no_zero_key_and_keep_the_range_law() {
    let user_dir = TempUserDir::new("guess-zero-zhuyin");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    // Standard: su = \u{310B}\u{3127} (ni), cl = \u{310F}\u{3120} (hao).
    assert!(pinyin_set_zhuyin_scheme(context, 1));

    // Standard binds no `'`: the parse stops there — no zero-key column.
    let split = cstr("su'cl");
    assert_eq!(pinyin_parse_more_chewings(instance, split.as_ptr()), 2);

    // The range half of the law holds in original zhuyin coordinates.
    let both = cstr("sucl");
    assert_eq!(pinyin_parse_more_chewings(instance, both.as_ptr()), 4);
    assert!(pinyin_guess_candidates(instance, 4, DEFAULT_SORT));
    assert!(pinyin_guess_candidates(instance, 5, DEFAULT_SORT));

    // The choose cursor is the absolute end in original coordinates.
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    let ni = candidate_at(instance, position_of(instance, "\u{4f60}"));
    assert_eq!(pinyin_choose_candidate(instance, 0, ni), 2);
    assert!(pinyin_guess_candidates(instance, 2, DEFAULT_SORT));
    let hao = candidate_at(instance, position_of(instance, "\u{597d}"));
    let cursor = pinyin_choose_candidate(instance, 2, hao);
    assert_eq!(cursor, 4, "the second group ends at its own original end");
    assert_eq!(
        usize::try_from(cursor).expect("non-negative cursor"),
        pinyin_get_parsed_input_length(instance)
    );

    // Eten binds `'` to the content symbol \u{3118} (c) — an offset beside
    // it is an ordinary cursor, never normalized away or refused as a
    // leading separator run.
    assert!(pinyin_set_zhuyin_scheme(context, 5));
    let cu = cstr("'x"); // \u{3118}\u{3128} = cu
    assert_eq!(pinyin_parse_more_chewings(instance, cu.as_ptr()), 2);
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    assert!(
        pinyin_guess_candidates(instance, 1, DEFAULT_SORT),
        "a content apostrophe is not a zero key"
    );
    assert!(pinyin_guess_candidates(instance, 2, DEFAULT_SORT));
    assert!(pinyin_guess_candidates(instance, 3, DEFAULT_SORT));

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn luoma_input_carries_the_full_offset_law() {
    let user_dir = TempUserDir::new("guess-zero-luoma");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    // FULL_PINYIN_LUOMA: the pinned index parse consumes `'` as the
    // zero-key separator in original coordinates, so the whole law
    // applies — normalize across internal runs, retain the leading gap and
    // out-of-range. "ni" and "hao" spell the same as hanyu in the index.
    assert!(pinyin_set_full_pinyin_scheme(context, 2));

    assert_eq!(parse(instance, "ni'hao"), 6);
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    let ni = candidate_at(instance, position_of(instance, "\u{4f60}"));
    assert_eq!(pinyin_choose_candidate(instance, 0, ni), 2);

    assert!(pinyin_guess_candidates(instance, 3, DEFAULT_SORT));
    let after_run = texts(instance);
    assert!(!after_run.is_empty());
    assert!(pinyin_guess_candidates(instance, 2, DEFAULT_SORT));
    assert_eq!(
        texts(instance),
        after_run,
        "post-run and run-start offsets answer the same list"
    );

    let hao = candidate_at(instance, position_of(instance, "\u{597d}"));
    let cursor = pinyin_choose_candidate(instance, 3, hao);
    assert_eq!(
        cursor, 6,
        "the post-separator choose answers the absolute end"
    );
    assert_eq!(
        usize::try_from(cursor).expect("non-negative cursor"),
        pinyin_get_parsed_input_length(instance)
    );

    // One past the reserved slot finds nothing (offset 8 and beyond abort,
    // as at the pin); the leading gap does not erase its real key.
    assert_eq!(parse(instance, "ni'hao"), 6);
    assert!(pinyin_guess_candidates(instance, 7, DEFAULT_SORT));
    // The re-send of the same buffer CONTINUES the committed composition
    // (the R5 revert, register #8: upstream's parse path never touches
    // the store, `pinyin.cpp:1497-1517`), so both forcings survive —
    // each probe answers true exactly because they did. The next parse
    // DIVERGES ("'ni"), which is the boundary that still starts fresh
    // and drops the store, so the probes must sit above it.
    assert!(
        pinyin_clear_constraint(instance, 0),
        "the \u{4f60} forcing survived the committed re-send"
    );
    assert!(
        pinyin_clear_constraint(instance, 3),
        "the \u{597d} forcing survived the committed re-send"
    );
    assert_eq!(parse(instance, "'ni"), 3);
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    assert!(pinyin_guess_candidates(instance, 1, DEFAULT_SORT));

    // An unparsed suffix is outside the law's domain: the bound is the
    // parse's consumed prefix, not the stored buffer, exactly like the
    // other transformed seams' parsed lengths.
    assert_eq!(parse(instance, "ni'hao!"), 6);
    assert!(
        pinyin_guess_candidates(instance, 6, DEFAULT_SORT),
        "one past the parsed region is the reserved slot"
    );
    assert!(
        pinyin_guess_candidates(instance, 7, DEFAULT_SORT),
        "one past the reserved slot finds nothing, as at the pin"
    );
    // Offset 8 is past the pin's matrix, whose `get_column_size` asserts:
    // the process aborts (register row 66).

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn a_guess_at_a_mid_syllable_offset_answers_the_empty_column() {
    // The pin's empty matrix column: no key of ni|hao starts at bytes
    // 1/3/4, so `pinyin_guess_candidates` there answers true — the parse
    // is non-empty, the empty-matrix refusal does not fire — with no
    // phrase rows (the suffix re-parse the engine once served must stay
    // gone), then the stored n-best row alone once a sentence lookup
    // ran. The syllable start at byte 2 keeps its window throughout
    // (measured against the pin: nihao@1/3/4 → true n=0 fresh, true n=1
    // 你好 after `pinyin_guess_sentence`; nihao@2 → the hao window).
    let user_dir = TempUserDir::new("guess-mid-syllable");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    assert_eq!(parse(instance, "nihao"), 5);
    for offset in [1usize, 3, 4] {
        assert!(
            pinyin_guess_candidates(instance, offset, DEFAULT_SORT),
            "offset {offset} answers true: the parse is non-empty"
        );
        assert_eq!(
            texts(instance),
            Vec::<String>::new(),
            "offset {offset}: no suffix re-parse rows at the empty column"
        );
    }
    assert!(pinyin_guess_candidates(instance, 2, DEFAULT_SORT));
    assert!(
        texts(instance).iter().any(|text| text == "\u{597d}"),
        "offset 2 keeps the hao window"
    );

    assert!(pinyin_guess_sentence(instance));
    for offset in [1usize, 3, 4] {
        assert!(
            pinyin_guess_candidates(instance, offset, DEFAULT_SORT),
            "offset {offset} still answers true after the sentence lookup"
        );
        // Only the n-best sentence rows appear at the empty column (no
        // suffix re-parse), 你好 first. The mini fixture, now read with
        // the chunk items' real counts, offers 你好 and the near-tie 你号
        // as rows 0 and 1 — the registered gfloat-trellis divergence
        // (`upstream-divergences.md`) tips that tie at fixture scale; the
        // full-data nbest differential is identical.
        let rows = texts(instance);
        assert_eq!(rows.first().map(String::as_str), Some("\u{4f60}\u{597d}"));
        assert!(
            rows.iter().all(|text| text.chars().count() == 2),
            "offset {offset}: only two-character n-best rows: {rows:?}"
        );
    }

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}
