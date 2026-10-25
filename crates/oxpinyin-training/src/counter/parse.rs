//! Token-stream and dump parsing.
//!
//! [`parse_ngseg`] reads the token fields of `TAGLIB_PARSE_SEGMENTED_LINE`
//! (`utils/utils_helper.h:51-74`); [`parse_interpolation_dump`] reads the
//! value shape of `export_interpolation` (`utils/storage/export_interpolation.cpp`).

use std::collections::BTreeMap;

use crate::segment::NULL_TOKEN;

use crate::counter::counts::Counts;
use crate::counter::error::CounterError;

/// Parses the ngseg segmented-token stream into token IDs, `NULL_TOKEN`
/// marking sentence separators for the counter.
///
/// `gen_ngram` reads the stream with `getline` (one record per `\n`) and
/// feeds each line to `TAGLIB_PARSE_SEGMENTED_LINE`: an empty line stays
/// `null_token`, any other line must split into exactly two `space`/`tab`
/// fields whose first field is the decimal token. A line that does not
/// split into two fields aborts `gen_ngram`; here it is an error.
/// Unlike the pin macro, this helper does not validate nonzero tokens
/// against their phrase text. Numeric-domain and validation follow-ups
/// are classified in `docs/findings/926-tool-tokenizers.md`.
///
/// # Errors
///
/// Returns [`CounterError::MalformedLine`] when a non-empty line lacks the
/// `token phrase` shape.
pub fn parse_ngseg(text: &str) -> Result<Vec<u32>, CounterError> {
    let mut tokens = Vec::new();
    for (index, line) in text.split_inclusive('\n').enumerate() {
        let line = line.strip_suffix('\n').unwrap_or(line);
        tokens.push(parse_segmented_line(line, index + 1)?);
    }
    Ok(tokens)
}

/// One line of the ngseg stream → token ID (`atoi` of the first field).
fn parse_segmented_line(line: &str, number: usize) -> Result<u32, CounterError> {
    // `TAGLIB_PARSE_SEGMENTED_LINE`: `0 == strlen(line)` → `null_token`.
    if line.is_empty() {
        return Ok(NULL_TOKEN);
    }

    // `g_strsplit_set(line, " \t", 2)`; `abort()` unless exactly two parts.
    let Some((token_text, _phrase)) = line.split_once([' ', '\t']) else {
        return Err(CounterError::MalformedLine { line: number });
    };

    // `atoi(strs[0])`: leading decimal digits, `0` when none.
    Ok(atoi(token_text))
}

/// Decimal-prefix parse for unsigned token fields emitted by the segmenter.
/// This is not a general C `atoi`: signed/overflowing inputs are a separate
/// compatibility gap, unchanged by borrowing the prefix instead of copying it.
fn atoi(text: &str) -> u32 {
    let end = text.bytes().take_while(u8::is_ascii_digit).count();
    text[..end].parse::<u32>().unwrap_or(0)
}

/// Parses an `export_interpolation` dump into the same value shape
/// [`Counts`] carries, discarding the phrase-text columns.
///
/// Unigram lines are `\item {token} {phrase} count {freq}`; bigram lines
/// are `\item {t1} {w1} {t2} {w2} count {freq}` (`export_interpolation.cpp:76-143`).
/// Only the token fields and the final `count` are read.
#[must_use]
pub fn parse_interpolation_dump(text: &str) -> Counts {
    let mut unigrams = BTreeMap::new();
    let mut bigrams = BTreeMap::new();
    let mut section = Section::None;

    for line in text.lines() {
        match line {
            "\\1-gram" => {
                section = Section::Unigram;
                continue;
            }
            "\\2-gram" => {
                section = Section::Bigram;
                continue;
            }
            "\\end" => {
                section = Section::None;
                continue;
            }
            _ => {}
        }

        if !line.starts_with("\\item ") {
            continue;
        }

        // `\item` then fields; the count is always the last field, the
        // phrase texts in between are single tokens (no whitespace).
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 2 {
            continue;
        }
        let Some(count) = fields.last().and_then(|c| c.parse::<u64>().ok()) else {
            continue;
        };
        match section {
            Section::Unigram => {
                if let Some(token) = fields.get(1).and_then(|t| t.parse::<u32>().ok()) {
                    unigrams.insert(token, count);
                }
            }
            Section::Bigram => {
                let t1 = fields.get(1).and_then(|t| t.parse::<u32>().ok());
                let t2 = fields.get(3).and_then(|t| t.parse::<u32>().ok());
                if let (Some(t1), Some(t2)) = (t1, t2) {
                    bigrams.insert((t1, t2), count);
                }
            }
            Section::None => {}
        }
    }

    Counts { unigrams, bigrams }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Section {
    None,
    Unigram,
    Bigram,
}

#[cfg(test)]
mod tests {
    use super::{atoi, parse_ngseg};

    #[test]
    fn atoi_matches_c_for_token_fields() {
        assert_eq!(atoi("16817937"), 16_817_937);
        assert_eq!(atoi("0"), 0);
        assert_eq!(atoi(""), 0);
        assert_eq!(atoi("12abc"), 12);
    }

    #[test]
    fn parse_ngseg_reproduces_segmented_lines() {
        let tokens = parse_ngseg("16817937 中国\n0 \n16782711 人\n").unwrap();
        assert_eq!(tokens, vec![16_817_937, 0, 16_782_711]);
    }

    #[test]
    fn parse_ngseg_treats_empty_line_as_null() {
        let tokens = parse_ngseg("16817937 中国\n\n16782711 人\n").unwrap();
        assert_eq!(tokens, vec![16_817_937, 0, 16_782_711]);
    }

    #[test]
    fn parse_ngseg_rejects_missing_second_field() {
        assert!(parse_ngseg("16817937\n").is_err());
    }

    #[test]
    fn segmented_ascii_split_retains_the_phrase_tail() {
        // Zero bypasses the pin's phrase validation, so all these tails are
        // valid, including repeated separators and a literal trailing slash.
        for line in ["0 ", "0  中国", "0\t\t中国", "0 \\", "0 \u{200d}\u{378}"] {
            assert_eq!(parse_ngseg(line).unwrap(), [0], "{line:?}");
        }
        for separator in ['\u{a0}', '\u{2003}', '\u{2028}', '\u{2029}'] {
            assert!(parse_ngseg(&format!("0{separator}中国")).is_err());
        }
        assert_eq!(parse_ngseg("").unwrap(), []);
        assert_eq!(parse_ngseg("\n").unwrap(), [0]);
        assert!(parse_ngseg("\r\n").is_err());
    }

    #[test]
    fn ordinary_segmented_stream_preserves_model_values() {
        let tokens = parse_ngseg("16817937 中国\n16782711\t人\n0 \\\n16817937tail 中国\n").unwrap();
        let counts = crate::counter::Counter::new(true, [16_817_937, 16_782_711]).count(&tokens);
        assert_eq!(counts.unigrams.get(&16_817_937), Some(&3));
        assert_eq!(counts.unigrams.get(&16_782_711), Some(&2));
        assert_eq!(counts.bigrams.get(&(1, 16_817_937)), Some(&2));
        assert_eq!(counts.bigrams.get(&(16_817_937, 16_782_711)), Some(&1));
        assert_eq!(counts.bigrams.len(), 2);
    }
}
