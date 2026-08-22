//! The dbff264 lookup-offset law through the C ABI.
//!
//! ibus-libpinyin ≥ 1.16.1 passes `pinyin_guess_candidates` the raw begin
//! of the next key rest, which can sit one position past the zero-key `'`
//! separator run (ibus-libpinyin issue #570). The guess normalizes the
//! caller offset back across the run, refuses — never aborts — when the
//! run leads the input, and a choose at the caller offset still advances.

use std::os::raw::{c_int, c_uint};
use std::ptr;

use crate::candidates::{pinyin_choose_candidate, pinyin_get_candidate, pinyin_get_n_candidate};
use crate::parse::{pinyin_get_parsed_input_length, pinyin_parse_more_full_pinyins};
use crate::sentence::pinyin_guess_candidates;
use crate::state::instance_ref;
use crate::test_support::{DEFAULT_SORT, TempUserDir, cstr, open};
use crate::types::{LookupCandidate, PinyinInstance};

/// The current snapshot's candidate texts, in list order.
fn texts(instance: *mut PinyinInstance) -> Vec<String> {
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let inst = unsafe { instance_ref(instance) };
    inst.candidates
        .iter()
        .map(|c| c.text.to_str().expect("UTF-8 candidate").to_owned())
        .collect()
}

/// The snapshot position of the first candidate with `text`.
fn position_of(instance: *mut PinyinInstance, text: &str) -> usize {
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let inst = unsafe { instance_ref(instance) };
    inst.candidates
        .iter()
        .position(|c| c.text.to_str() == Ok(text))
        .unwrap_or_else(|| panic!("{text:?} is offered"))
}

/// Borrowed pointer to the snapshot candidate at `index`.
fn candidate_at(instance: *mut PinyinInstance, index: usize) -> *mut LookupCandidate {
    let mut cand: *mut LookupCandidate = ptr::null_mut();
    assert!(
        pinyin_get_candidate(instance, index as c_uint, &mut cand),
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
    for (input, post_run, zero_start) in [("ni'hao", 3usize, 2usize), ("ni''hao", 4, 2)] {
        let user_dir = TempUserDir::new(&format!("guess-zero-run-{post_run}"));
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
            cursor > post_run as c_int,
            "{input}: choose at the post-separator offset advances (got {cursor})"
        );

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }
}

#[test]
fn guess_at_offset_zero_without_a_separator_is_unchanged() {
    let user_dir = TempUserDir::new("guess-zero-plain");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    assert_eq!(parse(instance, "nihao"), 5);
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    let first = texts(instance);
    assert!(first.iter().any(|t| t == "\u{4f60}\u{597d}"));
    assert!(first.iter().any(|t| t == "\u{4f60}"));

    // Offset 0 with no zero key anywhere is the identity walk: re-running
    // the guess reproduces the same snapshot.
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    assert_eq!(texts(instance), first);

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn a_leading_or_trailing_run_never_walks_off_the_input() {
    let user_dir = TempUserDir::new("guess-zero-edges");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    // Leading run: the walk stops at byte 1, so the offset one past the
    // run stays unnormalized and the check refuses it — upstream's
    // `_check_offset` assert, answered as `false` with an emptied
    // snapshot instead of an abort.
    assert_eq!(parse(instance, "'ni"), 3, "the leading run parses whole");
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    assert!(!pinyin_guess_candidates(instance, 1, DEFAULT_SORT));
    let mut num: c_uint = 77;
    assert!(pinyin_get_n_candidate(instance, &mut num));
    assert_eq!(num, 0, "a refused guess leaves the freed-list shape");
    assert!(
        pinyin_guess_candidates(instance, 0, DEFAULT_SORT),
        "the refusal leaves the instance usable"
    );

    // Trailing run: one past the input's last byte normalizes back to the
    // run without reading past the buffer.
    assert!(parse(instance, "ni'") >= 2);
    assert!(pinyin_guess_candidates(instance, 3, DEFAULT_SORT));

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn an_offset_past_the_parsed_input_is_refused() {
    // Upstream reads its matrix out of bounds for such an offset — no
    // pinned behaviour exists — so the ABI refuses instead of answering
    // candidates anchored at a position that does not exist. The
    // one-past-end offset itself is the matrix's reserved extra slot and
    // stays valid.
    let user_dir = TempUserDir::new("guess-zero-range");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    assert_eq!(parse(instance, "ni'hao"), 6);
    assert!(
        pinyin_guess_candidates(instance, 6, DEFAULT_SORT),
        "the one-past-end offset is the reserved slot, still in range"
    );
    let parsed = pinyin_get_parsed_input_length(instance);
    assert!(
        !pinyin_guess_candidates(instance, parsed + 1, DEFAULT_SORT),
        "an offset past the parsed input is refused"
    );
    let mut num: c_uint = 77;
    assert!(pinyin_get_n_candidate(instance, &mut num));
    assert_eq!(num, 0, "the refusal leaves no candidates behind");
    assert!(
        pinyin_guess_candidates(instance, 0, DEFAULT_SORT),
        "the refusal leaves the instance usable"
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
    assert!(cursor as usize <= parsed, "the cursor never overshoots");
    assert_eq!(
        cursor as usize, parsed,
        "the ibus idiom lands in the commit branch (cursor == length), \
         not a further key_rest"
    );

    // Mid-input the end is below the parse length, so a clamp could not
    // fake it: ni'hao'a, choose 好 at the post-separator offset 3.
    assert_eq!(parse(instance, "ni'hao'a"), 8);
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    let ni = candidate_at(instance, position_of(instance, "\u{4f60}"));
    assert_eq!(pinyin_choose_candidate(instance, 0, ni), 2);
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
        cursor as usize,
        pinyin_get_parsed_input_length(instance),
        "the final choose lands the commit branch"
    );

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}

#[test]
fn a_choose_then_guess_covers_the_remaining_group() {
    // The xiang'a shape over the mini fixture's keys: choose the first
    // group of xian'a, then guess the rest at the choose cursor and at
    // ibus's post-separator begin.
    let user_dir = TempUserDir::new("guess-zero-xian");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    assert_eq!(parse(instance, "xian'a"), 6);
    assert!(pinyin_guess_candidates(instance, 0, DEFAULT_SORT));
    let spans_xian = {
        // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
        let inst = unsafe { instance_ref(instance) };
        inst.candidates
            .iter()
            .position(|c| c.consumed_bytes == 4)
            .expect("a candidate spans the xian group")
    };
    let cand = candidate_at(instance, spans_xian);
    assert_eq!(pinyin_choose_candidate(instance, 0, cand), 4);

    assert!(pinyin_guess_candidates(instance, 4, DEFAULT_SORT));
    let at_cursor = texts(instance);
    assert!(
        !at_cursor.is_empty(),
        "the remaining group offers candidates after the choose"
    );
    assert!(pinyin_guess_candidates(instance, 5, DEFAULT_SORT));
    assert_eq!(
        texts(instance),
        at_cursor,
        "ibus's post-separator begin answers the same remaining group"
    );

    crate::instance::pinyin_free_instance(instance);
    crate::context::pinyin_fini(context);
}
