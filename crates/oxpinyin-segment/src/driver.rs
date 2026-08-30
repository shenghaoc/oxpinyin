//! Line-oriented `ngseg` driver (`utils/segment/ngseg.cpp:182-256`).
//!
//! New code for this crate: character-domain run splitting and the
//! `null_token` output format. The Viterbi itself is [`crate::trellis`].

use crate::error::SegmentError;
use crate::lexicon::PhraseLexicon;
use crate::model::{NULL_TOKEN, SegmentModel};
use crate::trellis::get_best_match;

/// One line of `ngseg` stdout.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Emitted {
    /// `0 \n` — empty line, invalid UTF-8, extra enter, or file tail.
    Null,
    /// `0 {raw}\n` — an un-segmentable run (`deal_with_unknown`).
    Unknown(String),
    /// `{token} {phrase}\n` — one dictionary phrase on the best path.
    Phrase {
        /// `phrase_token_t`.
        token: u32,
        /// Phrase text from the phrase index.
        text: String,
    },
}

impl Emitted {
    /// Formats this record the way `ngseg` prints it, including the newline.
    #[must_use]
    pub fn to_ngseg_line(&self) -> String {
        match self {
            Self::Null => format!("{NULL_TOKEN} \n"),
            Self::Unknown(text) => format!("{NULL_TOKEN} {text}\n"),
            Self::Phrase { token, text } => format!("{token} {text}\n"),
        }
    }
}

/// Segments one UTF-8 line (trailing `\n` already stripped).
///
/// # Errors
///
/// Returns [`SegmentError`] only when a bigram table read fails.
pub fn segment_line(
    lexicon: &PhraseLexicon,
    model: &SegmentModel,
    lambda: f32,
    line: &str,
) -> Result<Vec<Emitted>, SegmentError> {
    if line.is_empty() {
        return Ok(vec![Emitted::Null]);
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
        emit_run(
            lexicon,
            model,
            lambda,
            &chars[start..index],
            segmentable,
            &mut emitted,
        )?;
        start = index;
        segmentable = next;
    }
    emit_run(
        lexicon,
        model,
        lambda,
        &chars[start..],
        segmentable,
        &mut emitted,
    )?;
    Ok(emitted)
}

/// Segments a whole file's bytes, including the trailing file-tail
/// `null_token`. Invalid UTF-8 lines become a lone `null_token`, matching
/// the `len != num_of_chars` branch (`ngseg.cpp:192-196`).
///
/// # Errors
///
/// Returns [`SegmentError`] when a bigram table read fails.
pub fn segment_bytes(
    lexicon: &PhraseLexicon,
    model: &SegmentModel,
    lambda: f32,
    input: &[u8],
    extra_enter: bool,
) -> Result<String, SegmentError> {
    let mut output = String::new();
    for line in getline_lines(input) {
        match std::str::from_utf8(line) {
            Ok(text) => {
                for record in segment_line(lexicon, model, lambda, text)? {
                    output.push_str(&record.to_ngseg_line());
                }
                // `--generate-extra-enter` applies after a successfully
                // parsed line only; the invalid-UTF-8 branch below already
                // printed its own null and `continue`s (`ngseg.cpp:192-196`),
                // so it must not get a second one.
                if extra_enter {
                    output.push_str(&Emitted::Null.to_ngseg_line());
                }
            }
            Err(_) => output.push_str(&Emitted::Null.to_ngseg_line()),
        }
    }
    output.push_str(&Emitted::Null.to_ngseg_line());
    Ok(output)
}

fn emit_run(
    lexicon: &PhraseLexicon,
    model: &SegmentModel,
    lambda: f32,
    run: &[char],
    segmentable: bool,
    into: &mut Vec<Emitted>,
) -> Result<(), SegmentError> {
    if run.is_empty() {
        return Ok(());
    }
    if !segmentable {
        into.push(Emitted::Unknown(run.iter().collect()));
        return Ok(());
    }
    // deal_with_segmentable: on failure the caller ignores the return
    // (`ngseg.cpp:226, 240`) and prints nothing for the run.
    if let Some(phrases) = get_best_match(lexicon, model, lambda, run)? {
        for (token, text) in phrases {
            into.push(Emitted::Phrase { token, text });
        }
    }
    Ok(())
}

/// `getline` line splits: strip a single trailing `\n`; a final line
/// without `\n` is still yielded; an empty input yields no lines.
fn getline_lines(input: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut rest = input;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        if let Some(index) = rest.iter().position(|&byte| byte == b'\n') {
            let line = &rest[..index];
            rest = &rest[index + 1..];
            Some(line)
        } else {
            let line = rest;
            rest = &[];
            Some(line)
        }
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{Emitted, segment_bytes, segment_line};
    use crate::lexicon::PhraseLexicon;
    use crate::model::{PINNED_LAMBDA, SegmentModel};

    fn toy() -> (PhraseLexicon, SegmentModel) {
        let lexicon = PhraseLexicon::from_pairs(vec![
            (10, "中".to_owned()),
            (11, "国".to_owned()),
            (12, "中国".to_owned()),
        ]);
        let mut unigrams = HashMap::new();
        unigrams.insert(10, 10);
        unigrams.insert(11, 10);
        unigrams.insert(12, 100);
        let model = SegmentModel::memory(unigrams, HashMap::new(), &lexicon);
        (lexicon, model)
    }

    #[test]
    fn empty_line_is_a_lone_null_token() {
        let (lexicon, model) = toy();
        assert_eq!(
            segment_line(&lexicon, &model, PINNED_LAMBDA, "").expect("no io"),
            vec![Emitted::Null]
        );
    }

    #[test]
    fn unknown_runs_are_emitted_verbatim() {
        let (lexicon, model) = toy();
        let records = segment_line(&lexicon, &model, PINNED_LAMBDA, "a中国b").expect("no io");
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
    fn file_tail_null_token_is_always_emitted() {
        let (lexicon, model) = toy();
        let out = segment_bytes(&lexicon, &model, PINNED_LAMBDA, b"", false).expect("no io");
        assert_eq!(out, "0 \n");

        let out = segment_bytes(&lexicon, &model, PINNED_LAMBDA, b"\n", false).expect("no io");
        assert_eq!(out, "0 \n0 \n");
    }

    #[test]
    fn extra_enter_adds_a_null_token_per_line() {
        let (lexicon, model) = toy();
        let out = segment_bytes(&lexicon, &model, PINNED_LAMBDA, b"\n", true).expect("no io");
        assert_eq!(out, "0 \n0 \n0 \n");
    }

    #[test]
    fn invalid_utf8_is_a_null_token_line() {
        let (lexicon, model) = toy();
        let out =
            segment_bytes(&lexicon, &model, PINNED_LAMBDA, &[0xff, b'\n'], false).expect("no io");
        assert_eq!(out, "0 \n0 \n");
    }

    #[test]
    fn invalid_utf8_skips_extra_enter() {
        let (lexicon, model) = toy();
        let out =
            segment_bytes(&lexicon, &model, PINNED_LAMBDA, &[0xff, b'\n'], true).expect("no io");
        assert_eq!(out, "0 \n0 \n");
    }
}
