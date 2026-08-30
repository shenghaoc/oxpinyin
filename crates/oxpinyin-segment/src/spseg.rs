//! `spseg` — fewest-words shortest-path segmenter
//! (`utils/segment/spseg.cpp`).
//!
//! Same line framing and character-run grouping as `ngseg`
//! ([`crate::driver`]) — three-state INIT/SEGMENTABLE/UNKNOWN grouping by
//! [`PhraseLexicon::is_char_segmentable`], the same `null_token` output
//! grammar, the same trailing file-tail enter and `--generate-extra-enter`
//! — but a *segmentable* run is split by an O(n²) dynamic program that
//! minimises a word count (`spseg.cpp:83-138`), not the bigram-scored
//! Viterbi. No bigram or λ is consulted, so this needs only the lexicon.
//!
//! The DP is reproduced faithfully, including the upstream quirk that the
//! running `nword` is incremented *inside* the inner `k` loop
//! (`spseg.cpp:121`), so the cost charged to an edge `i→k` is
//! `steps[i] + (number of non-skipped spans from i+1 to k)`, not a flat
//! `+1`. This is not "cleaned up": it is the behaviour the differential
//! parity target exhibits (`AGENTS.md` source policy; task §4, §18).

use crate::driver::{Emitted, getline_lines};
use crate::lexicon::PhraseLexicon;
use crate::model::NULL_TOKEN;

/// One cell of the shortest-path DP over a segmentable run.
#[derive(Clone)]
struct Step {
    /// Word count to reach this boundary; [`u32::MAX`] means unreachable
    /// (upstream `UINT_MAX`, `spseg.cpp:75`).
    nword: u32,
    /// `phrase_token_t` of the phrase ending here (`m_handle`).
    token: u32,
    /// Start offset of that phrase within the run.
    phrase_start: usize,
    /// Length of that phrase, in characters.
    phrase_len: usize,
    /// Index of the previous boundary (upstream stores `i - k`; we store
    /// the absolute predecessor `i`).
    backward: usize,
}

impl Step {
    const UNREACHABLE: Self = Self {
        nword: u32::MAX,
        token: NULL_TOKEN,
        phrase_start: 0,
        phrase_len: 0,
        backward: 0,
    };
}

/// Fewest-words split of one segmentable run into `(token, text)` phrases,
/// left to right (`spseg.cpp:83-186`).
fn segment_run(lexicon: &PhraseLexicon, run: &[char]) -> Vec<(u32, String)> {
    let n = run.len();
    let mut steps = vec![Step::UNREACHABLE; n + 1];
    steps[0].nword = 0;

    for i in 0..n {
        let base = steps[i].nword;
        // A truly unreachable start cannot extend a path; skipping it also
        // avoids the `UINT_MAX + 1` wrap upstream relies on never mattering
        // (every 1-char span in a segmentable run is `SEARCH_OK`, so every
        // prefix boundary is in fact reachable).
        if base == u32::MAX {
            continue;
        }
        // `nword` accumulates across the inner loop, matching `spseg.cpp`.
        let mut nword = base;
        let mut span = String::new();
        for k in (i + 1)..=n {
            span.push(run[k - 1]);
            let len = k - i;
            let (ok, continued, tokens) = lexicon.search(&span);
            let token = if ok { tokens[0] } else { NULL_TOKEN };

            // `!SEARCH_OK` at length > 1: no edge, and the `continue`
            // skips the `++nword` and the `SEARCH_CONTINUED` break check.
            if !ok && len != 1 {
                continue;
            }

            nword = nword.saturating_add(1);
            if nword < steps[k].nword {
                steps[k] = Step {
                    nword,
                    token,
                    phrase_start: i,
                    phrase_len: len,
                    backward: i,
                };
            }

            if !continued {
                break;
            }
        }
    }

    backtrace(&steps, run)
}

/// Reconstruct the chosen path from the final boundary (`spseg.cpp:140-165`).
fn backtrace(steps: &[Step], run: &[char]) -> Vec<(u32, String)> {
    let mut phrases = Vec::new();
    let mut cursor = run.len();
    // `backward` is strictly less than the current index for every relaxed
    // cell, so `cursor` decreases monotonically toward 0 and the walk
    // terminates without a visited-set.
    while cursor != 0 {
        let step = &steps[cursor];
        let text: String = run[step.phrase_start..step.phrase_start + step.phrase_len]
            .iter()
            .collect();
        phrases.push((step.token, text));
        if step.backward >= cursor {
            // Defensive: an unreachable tail (cannot occur for a
            // segmentable run) would otherwise stall. Stop cleanly.
            break;
        }
        cursor = step.backward;
    }
    phrases.reverse();
    phrases
}

/// Segments one UTF-8 line (trailing `\n` already stripped) the `spseg`
/// way. Never fails: no bigram is consulted.
#[must_use]
pub fn segment_line(lexicon: &PhraseLexicon, line: &str) -> Vec<Emitted> {
    if line.is_empty() {
        return vec![Emitted::Null];
    }

    let chars: Vec<char> = line.chars().collect();
    let mut emitted = Vec::new();
    let mut start = 0_usize;
    let mut segmentable = lexicon.is_char_segmentable(chars[0]);

    for index in 1..chars.len() {
        let next = lexicon.is_char_segmentable(chars[index]);
        if next == segmentable {
            continue;
        }
        emit_run(lexicon, &chars[start..index], segmentable, &mut emitted);
        start = index;
        segmentable = next;
    }
    emit_run(lexicon, &chars[start..], segmentable, &mut emitted);
    emitted
}

/// Segments a whole file the `spseg` way, including the trailing file-tail
/// `null_token` (`spseg.cpp:339`). Invalid-UTF-8 lines become a lone
/// `null_token` (`spseg.cpp:273-277`).
#[must_use]
pub fn segment_bytes(lexicon: &PhraseLexicon, input: &[u8], extra_enter: bool) -> String {
    let mut output = String::new();
    for line in getline_lines(input) {
        match std::str::from_utf8(line) {
            Ok(text) => {
                for record in segment_line(lexicon, text) {
                    output.push_str(&record.to_ngseg_line());
                }
                if extra_enter {
                    output.push_str(&Emitted::Null.to_ngseg_line());
                }
            }
            Err(_) => output.push_str(&Emitted::Null.to_ngseg_line()),
        }
    }
    output.push_str(&Emitted::Null.to_ngseg_line());
    output
}

fn emit_run(lexicon: &PhraseLexicon, run: &[char], segmentable: bool, into: &mut Vec<Emitted>) {
    if run.is_empty() {
        return;
    }
    if !segmentable {
        into.push(Emitted::Unknown(run.iter().collect()));
        return;
    }
    for (token, text) in segment_run(lexicon, run) {
        into.push(Emitted::Phrase { token, text });
    }
}

#[cfg(test)]
mod tests {
    use super::{segment_bytes, segment_line, segment_run};
    use crate::driver::Emitted;
    use crate::lexicon::PhraseLexicon;

    fn lexicon() -> PhraseLexicon {
        PhraseLexicon::from_pairs(vec![
            (10, "中".to_owned()),
            (11, "国".to_owned()),
            (12, "中国".to_owned()),
            (13, "人".to_owned()),
            (14, "中国人".to_owned()),
        ])
    }

    #[test]
    fn prefers_the_single_longest_phrase() {
        // "中国" is one phrase (1 word) vs "中"+"国" (2 words): fewest
        // words picks the single phrase, token 12.
        let phrases = segment_run(&lexicon(), &['中', '国']);
        assert_eq!(phrases, vec![(12, "中国".to_owned())]);
    }

    #[test]
    fn three_char_phrase_beats_phrase_plus_char() {
        // "中国人" as one 3-char phrase (token 14) is fewer words than
        // "中国"+"人".
        let phrases = segment_run(&lexicon(), &['中', '国', '人']);
        assert_eq!(phrases, vec![(14, "中国人".to_owned())]);
    }

    #[test]
    fn tie_break_prefers_the_path_whose_last_segment_starts_earliest() {
        // A B C with phrases {A, B, C, AB, BC} but not ABC. Both "A|BC" and
        // "AB|C" are 2-word covers. Upstream's `++nword`-in-loop DP relaxes
        // step[3] first via BC (from start i=1, processed before i=2's C
        // edge), so "A | BC" wins, not the leftmost-longest "AB | C". This
        // pins that quirk (`spseg.cpp:104-133`).
        let lex = PhraseLexicon::from_pairs(vec![
            (1, "甲".to_owned()),
            (2, "乙".to_owned()),
            (3, "丙".to_owned()),
            (12, "甲乙".to_owned()),
            (23, "乙丙".to_owned()),
        ]);
        let phrases = segment_run(&lex, &['甲', '乙', '丙']);
        assert_eq!(phrases, vec![(1, "甲".to_owned()), (23, "乙丙".to_owned())]);
    }

    #[test]
    fn falls_back_to_single_characters() {
        // "国中" is not a phrase; the only cover is two single chars.
        let phrases = segment_run(&lexicon(), &['国', '中']);
        assert_eq!(phrases, vec![(11, "国".to_owned()), (10, "中".to_owned())]);
    }

    #[test]
    fn unknown_and_segmentable_runs_interleave() {
        let records = segment_line(&lexicon(), "a中国b");
        assert_eq!(
            records,
            vec![
                Emitted::Unknown("a".to_owned()),
                Emitted::Phrase {
                    token: 12,
                    text: "中国".to_owned(),
                },
                Emitted::Unknown("b".to_owned()),
            ]
        );
    }

    #[test]
    fn empty_line_is_a_lone_null() {
        assert_eq!(segment_line(&lexicon(), ""), vec![Emitted::Null]);
    }

    #[test]
    fn file_framing_matches_ngseg() {
        let lex = lexicon();
        assert_eq!(segment_bytes(&lex, b"", false), "0 \n");
        assert_eq!(segment_bytes(&lex, b"\n", false), "0 \n0 \n");
        assert_eq!(segment_bytes(&lex, b"\n", true), "0 \n0 \n0 \n");
        assert_eq!(segment_bytes(&lex, &[0xff, b'\n'], false), "0 \n0 \n");
    }

    #[test]
    fn full_line_renders_ngseg_grammar() {
        let out = segment_bytes(&lexicon(), "中国人".as_bytes(), false);
        assert_eq!(out, "14 中国人\n0 \n");
    }
}
