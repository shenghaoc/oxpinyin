//! Token-stream and dump parsing.
//!
//! [`parse_ngseg`] reads the token fields of `TAGLIB_PARSE_SEGMENTED_LINE`
//! (`utils/utils_helper.h:51-74`); [`parse_interpolation_dump`] reads the
//! value shape of `export_interpolation` (`utils/storage/export_interpolation.cpp`).

use std::collections::BTreeMap;

use crate::segment::{NULL_TOKEN, PhraseLexicon};

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
/// Records are bytes: NUL terminates each physical line, and arbitrary bytes
/// are allowed in ignored fields. Nonzero tokens must name their exact phrase.
///
/// # Errors
///
/// Returns [`CounterError::MalformedLine`] when a non-empty line lacks the
/// `token phrase` shape.
pub fn parse_ngseg(
    lexicon: &PhraseLexicon,
    text: impl AsRef<[u8]>,
) -> Result<Vec<u32>, CounterError> {
    let mut tokens = Vec::new();
    for (index, line) in text
        .as_ref()
        .split_inclusive(|byte| *byte == b'\n')
        .enumerate()
    {
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        tokens.push(parse_segmented_line(lexicon, line, index + 1)?);
    }
    Ok(tokens)
}

/// One line of the ngseg stream → a decimal token ID.
fn parse_segmented_line(
    lexicon: &PhraseLexicon,
    line: &[u8],
    number: usize,
) -> Result<u32, CounterError> {
    let line = line.split(|byte| *byte == 0).next().unwrap_or_default();
    // `TAGLIB_PARSE_SEGMENTED_LINE`: `0 == strlen(line)` → `null_token`.
    if line.is_empty() {
        return Ok(NULL_TOKEN);
    }

    // `g_strsplit_set(line, " \t", 2)`; `abort()` unless exactly two parts.
    let Some(separator) = line.iter().position(|byte| matches!(byte, b' ' | b'\t')) else {
        return Err(CounterError::MalformedLine { line: number });
    };
    let token: u32 = crate::numeric::decimal_field(&line[..separator])
        .map_err(|_| CounterError::MalformedLine { line: number })?;
    if token != NULL_TOKEN {
        let expected = if token == 1 {
            Some("<start>")
        } else {
            lexicon.text(token)
        };
        if expected.map(|text| text.split('\0').next().unwrap_or_default().as_bytes())
            != Some(&line[separator + 1..])
        {
            return Err(CounterError::InvalidPhrase {
                line: number,
                token,
            });
        }
    }
    Ok(token)
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
    use super::parse_ngseg;
    use crate::segment::PhraseLexicon;

    fn lexicon() -> PhraseLexicon {
        PhraseLexicon::from_pairs(vec![(16_817_937, "中国".into()), (16_782_711, "人".into())])
    }

    #[test]
    fn segmented_tokens_require_complete_decimals() {
        for field in ["+16817937", "0016817937"] {
            assert_eq!(
                parse_ngseg(&lexicon(), format!("{field} 中国")).unwrap(),
                [16_817_937]
            );
        }
        for field in ["16817937x", "4311785233", "-1", "abc", "+", "\r16817937"] {
            assert!(matches!(
                parse_ngseg(&lexicon(), format!("{field} 中国")),
                Err(crate::counter::CounterError::MalformedLine { line: 1 })
            ));
        }
    }

    #[test]
    fn parse_ngseg_reproduces_segmented_lines() {
        let tokens = parse_ngseg(&lexicon(), "16817937 中国\n0 \n16782711 人\n").unwrap();
        assert_eq!(tokens, vec![16_817_937, 0, 16_782_711]);
    }

    #[test]
    fn parse_ngseg_treats_empty_line_as_null() {
        let tokens = parse_ngseg(&lexicon(), "16817937 中国\n\n16782711 人\n").unwrap();
        assert_eq!(tokens, vec![16_817_937, 0, 16_782_711]);
    }

    #[test]
    fn parse_ngseg_rejects_missing_second_field() {
        assert!(parse_ngseg(&lexicon(), "16817937\n").is_err());
    }

    #[test]
    fn segmented_ascii_split_retains_the_phrase_tail() {
        // Zero bypasses the pin's phrase validation, so all these tails are
        // valid, including repeated separators and a literal trailing slash.
        for line in ["0 ", "0  中国", "0\t\t中国", "0 \\", "0 \u{200d}\u{378}"] {
            assert_eq!(parse_ngseg(&lexicon(), line).unwrap(), [0], "{line:?}");
        }
        for separator in ['\u{a0}', '\u{2003}', '\u{2028}', '\u{2029}'] {
            assert!(parse_ngseg(&lexicon(), format!("0{separator}中国")).is_err());
        }
        assert_eq!(parse_ngseg(&lexicon(), "").unwrap(), []);
        assert_eq!(parse_ngseg(&lexicon(), "\n").unwrap(), [0]);
        assert!(parse_ngseg(&lexicon(), "\r\n").is_err());
    }

    #[test]
    fn ordinary_segmented_stream_preserves_model_values() {
        let tokens = parse_ngseg(
            &lexicon(),
            "16817937 中国\n16782711\t人\n0 \\\n16817937 中国\n",
        )
        .unwrap();
        let counts = crate::counter::Counter::new(true, [16_817_937, 16_782_711]).count(&tokens);
        assert_eq!(counts.unigrams.get(&16_817_937), Some(&3));
        assert_eq!(counts.unigrams.get(&16_782_711), Some(&2));
        assert_eq!(counts.bigrams.get(&(1, 16_817_937)), Some(&2));
        assert_eq!(counts.bigrams.get(&(16_817_937, 16_782_711)), Some(&1));
        assert_eq!(counts.bigrams.len(), 2);
    }
    #[test]
    fn byte_records_validate_exact_phrase_and_nul_boundaries() {
        let lexicon = lexicon();
        let text = b"16817937 \xe4\xb8\xad\xe5\x9b\xbd\0\xff\n0 \xff\n\0\xff\n1 <start>";
        assert_eq!(parse_ngseg(&lexicon, text).unwrap(), [16_817_937, 0, 0, 1]);
        for text in [
            "16817937 wrong",
            "16817937  中国",
            "16817937 中国\r",
            "16817937 中\0国",
            "99 unknown",
            "1 wrong",
        ] {
            let error = parse_ngseg(&lexicon, text).unwrap_err();
            assert!(matches!(
                error,
                crate::counter::CounterError::InvalidPhrase { line: 1, .. }
            ));
            assert!(error.to_string().contains("unknown or mismatched phrase"));
            assert!(std::error::Error::source(&error).is_none());
        }
        assert!(parse_ngseg(&lexicon, "16817937\0 中国").is_err());
        assert!(parse_ngseg(&lexicon, b"16817937\xff \xe4\xb8\xad\xe5\x9b\xbd").is_err());
        let lexicon = PhraseLexicon::from_pairs(vec![(16_817_937, "中国\0hidden".into())]);
        assert_eq!(
            parse_ngseg(&lexicon, "16817937 中国").unwrap(),
            [16_817_937]
        );
    }

    #[test]
    fn validated_streams_feed_both_model_consumers() {
        let lexicon = lexicon();
        let input = b"16817937 \xe4\xb8\xad\xe5\x9b\xbd\0\xff\n16782711 \xe4\xba\xba\n";
        let counts = crate::counter::count_ngseg(&lexicon, input, true).unwrap();
        assert_eq!(counts.unigrams[&16_817_937], 2);
        assert_eq!(counts.bigrams[&(1, 16_817_937)], 1);
        assert_eq!(counts.bigrams[&(16_817_937, 16_782_711)], 1);
        let deleted = crate::lambda::count_deleted(&lexicon, input, true).unwrap();
        assert_eq!(deleted.bigrams[&(16_817_937, 16_782_711)], 1);
        assert_eq!(deleted.bigrams[&(1, 16_817_937)], 1);
        for bad in ["16817937 wrong", "99 unknown"] {
            assert!(crate::counter::count_ngseg(&lexicon, bad, true).is_err());
            assert!(crate::lambda::count_deleted(&lexicon, bad, true).is_err());
        }
    }
}
