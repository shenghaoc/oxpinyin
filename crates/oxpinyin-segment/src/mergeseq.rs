//! `mergeseq` — merge adjacent phrase runs into longer dictionary phrases
//! (`utils/segment/mergeseq.cpp`).
//!
//! Consumes a segmented token stream (`{token} {phrase}` lines, `0 …`
//! separators — the [`crate::driver::Emitted`] grammar `ngseg`/`spseg`
//! emit) and re-emits it with maximal adjacent runs replaced by the single
//! dictionary phrase they form, up to [`MAX_PHRASE_LENGTH`] characters
//! (`mergeseq.cpp:75-194`). It needs only the phrase table
//! ([`PhraseLexicon`]); no bigram.
//!
//! Upstream re-derives each token's characters from the phrase index
//! (`get_phrase_item`, `mergeseq.cpp:172-183`), ignoring the input line's
//! phrase field except for validation; we mirror that by taking the run
//! characters from [`PhraseLexicon::text`]. A `null_token` line flushes the
//! pending queue and is echoed verbatim; EOF feeds a synthetic `"0 "` line
//! so a trailing separator is always emitted (`mergeseq.cpp:272-275`).

use crate::driver::getline_lines;
use crate::error::SegmentError;
use crate::lexicon::{MAX_PHRASE_LENGTH, PhraseLexicon};
use crate::model::NULL_TOKEN;

/// A queued token and its length in characters (`TokenInfo`).
#[derive(Clone, Copy)]
struct TokenInfo {
    token: u32,
    char_len: usize,
}

/// Rolling merge state: the concatenated characters of the queued tokens
/// and the queue itself, kept in lock-step.
struct Merger {
    unichars: Vec<char>,
    queue: Vec<TokenInfo>,
    output: String,
}

impl Merger {
    fn new() -> Self {
        Self {
            unichars: Vec::new(),
            queue: Vec::new(),
            output: String::new(),
        }
    }

    /// Total characters currently queued (`calculate_sequence_length`).
    fn sequence_len(&self) -> usize {
        self.queue.iter().map(|info| info.char_len).sum()
    }

    /// If a maximal front prefix of the queue forms one dictionary phrase,
    /// replace that prefix with the single merged token
    /// (`mergeseq.cpp:75-123`). The longest prefix is tried first, shrinking
    /// from the end until a phrase-table hit; the first (lowest) token id of
    /// the hit is used, matching `get_first_token` over the sorted tokens.
    fn merge_sequence(&mut self, lexicon: &PhraseLexicon) {
        if self.queue.is_empty() {
            return;
        }
        let mut index = self.queue.len();
        let mut seq_len = self.sequence_len();
        let mut merged = None;

        while seq_len > 0 {
            let span: String = self.unichars[..seq_len].iter().collect();
            let (ok, _continued, tokens) = lexicon.search(&span);
            if ok {
                // A `SEARCH_OK` span always carries at least one token.
                if let Some(&token) = tokens.first() {
                    merged = Some((token, seq_len));
                    break;
                }
            }
            index -= 1;
            seq_len -= self.queue[index].char_len;
        }

        if let Some((token, char_len)) = merged {
            self.queue.drain(0..index);
            self.queue.insert(0, TokenInfo { token, char_len });
        }
    }

    /// Emit and drop the front token (`pop_first_token`, `mergeseq.cpp:125-145`).
    fn pop_first_token(&mut self) {
        let Some(info) = self.queue.first().copied() else {
            return;
        };
        let text: String = self.unichars[..info.char_len].iter().collect();
        self.output.push_str(&format!("{} {text}\n", info.token));
        self.unichars.drain(0..info.char_len);
        self.queue.remove(0);
    }

    /// Process one input line (`feed_line`, `mergeseq.cpp:147-194`).
    fn feed_line(&mut self, lexicon: &PhraseLexicon, line: &str) -> Result<(), SegmentError> {
        let token = parse_token(line)?;

        if token == NULL_TOKEN {
            while !self.queue.is_empty() {
                self.merge_sequence(lexicon);
                self.pop_first_token();
            }
            // Restore the separator line verbatim.
            self.output.push_str(line);
            self.output.push('\n');
            return Ok(());
        }

        let text = lexicon
            .text(token)
            .ok_or_else(|| SegmentError::MalformedLine {
                detail: format!("token {token} is not in the phrase table"),
            })?;
        let chars: Vec<char> = text.chars().collect();
        let char_len = chars.len();
        self.unichars.extend_from_slice(&chars);
        self.queue.push(TokenInfo { token, char_len });

        while self.sequence_len() >= MAX_PHRASE_LENGTH {
            self.merge_sequence(lexicon);
            self.pop_first_token();
        }
        Ok(())
    }
}

/// The leading token id of a segmented line (`TAGLIB_PARSE_SEGMENTED_LINE`).
fn parse_token(line: &str) -> Result<u32, SegmentError> {
    let (head, _rest) =
        line.split_once([' ', '\t'])
            .ok_or_else(|| SegmentError::MalformedLine {
                detail: format!("no separator in {line:?}"),
            })?;
    head.parse::<u32>()
        .map_err(|_| SegmentError::MalformedLine {
            detail: format!("token field {head:?} is not an integer"),
        })
}

/// Merges a whole segmented stream, returning the re-emitted stream.
///
/// # Errors
///
/// Returns [`SegmentError::MalformedLine`] when a non-empty line is not a
/// `token phrase` record, or names a token the phrase table does not carry.
pub fn merge_bytes(lexicon: &PhraseLexicon, input: &[u8]) -> Result<String, SegmentError> {
    let mut merger = Merger::new();
    for line in getline_lines(input) {
        // Non-UTF-8 lines cannot carry a valid token record.
        let text = std::str::from_utf8(line).map_err(|_| SegmentError::MalformedLine {
            detail: "line is not UTF-8".to_owned(),
        })?;
        if text.is_empty() {
            continue;
        }
        merger.feed_line(lexicon, text)?;
    }
    // Append one null token for EOF (`mergeseq.cpp:272-275`).
    merger.feed_line(lexicon, "0 ")?;
    Ok(merger.output)
}

#[cfg(test)]
mod tests {
    use super::merge_bytes;
    use crate::lexicon::PhraseLexicon;

    fn lexicon() -> PhraseLexicon {
        PhraseLexicon::from_pairs(vec![
            (10, "中".to_owned()),
            (11, "国".to_owned()),
            (12, "中国".to_owned()),
            (13, "人".to_owned()),
            (14, "人民".to_owned()),
            (15, "民".to_owned()),
        ])
    }

    #[test]
    fn merges_two_singles_into_a_phrase() {
        // "中" then "国" merge into "中国" (token 12) at the flush.
        let input = b"10 \xe4\xb8\xad\n11 \xe5\x9b\xbd\n0 \n";
        let out = merge_bytes(&lexicon(), input).expect("merge");
        // Flushed as one phrase, then the echoed separator, then the EOF
        // synthetic separator.
        assert_eq!(out, "12 中国\n0 \n0 \n");
    }

    #[test]
    fn non_mergeable_runs_pass_through() {
        // "国" then "中" do not form a phrase: emitted unchanged.
        let input = "11 国\n10 中\n0 \n".as_bytes();
        let out = merge_bytes(&lexicon(), input).expect("merge");
        assert_eq!(out, "11 国\n10 中\n0 \n0 \n");
    }

    #[test]
    fn separator_lines_are_echoed_verbatim() {
        // An unknown run `0 abc` is a null-token line: flush, echo verbatim.
        let input = "10 中\n0 abc\n11 国\n0 \n".as_bytes();
        let out = merge_bytes(&lexicon(), input).expect("merge");
        assert_eq!(out, "10 中\n0 abc\n11 国\n0 \n0 \n");
    }

    #[test]
    fn merges_across_a_longer_run() {
        // 人 民 → 人民 (token 14); 中 国 → 中国 (token 12).
        let input = "10 中\n11 国\n13 人\n15 民\n0 \n".as_bytes();
        let out = merge_bytes(&lexicon(), input).expect("merge");
        assert_eq!(out, "12 中国\n14 人民\n0 \n0 \n");
    }

    #[test]
    fn unknown_token_is_an_error_not_a_panic() {
        let input = "999 x\n0 \n".as_bytes();
        let err = merge_bytes(&lexicon(), input).unwrap_err();
        assert!(matches!(
            err,
            crate::error::SegmentError::MalformedLine { .. }
        ));
    }

    #[test]
    fn missing_separator_is_an_error() {
        let input = "10\n".as_bytes();
        let err = merge_bytes(&lexicon(), input).unwrap_err();
        assert!(matches!(
            err,
            crate::error::SegmentError::MalformedLine { .. }
        ));
    }
}
