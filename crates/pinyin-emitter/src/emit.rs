//! Textual `interpolation2.text` writer.
//!
//! Reproduces `utils/storage/export_interpolation.cpp` (`:33-40` header /
//! footer, `:75-104` unigrams, `:106-143` bigrams). The grammar is the
//! same one `pinyin_data::parse_interpolation2` reads for the `\1-gram`
//! section (`crates/pinyin-data/src/interp.rs`). Counts only — no λ
//! (PR #55: λ lives in `table.conf`). See `docs/findings/emitter-port.md`.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use pinyin_counter::Counts;
use pinyin_segment::{PhraseLexicon, SENTENCE_START};

use crate::error::EmitterError;

/// Phrase text `export_interpolation` prints for `sentence_start`
/// (`src/storage/tag_utility.cpp:368-370`).
pub const SENTENCE_START_TEXT: &str = "<start>";

/// Header `export_interpolation.cpp:34` / `import_interpolation.cpp:90-93`.
const DATA_HEADER: &str = "\\data model interpolation";

/// Resolves the phrase-text column `export_interpolation` prints for `token`.
///
/// Library-0 token `sentence_start` is the special `"<start>"` string
/// (`tag_utility.cpp:391-393`); every other token is the phrase-index
/// text. `None` matches `taglib_token_to_string` returning `NULL`, which
/// makes `export_interpolation` skip the record (`:97-98`, `:130-132`).
#[must_use]
pub fn phrase_text(token: u32, lexicon: &PhraseLexicon) -> Option<&str> {
    if token == SENTENCE_START {
        Some(SENTENCE_START_TEXT)
    } else {
        lexicon.text(token)
    }
}

/// Renders `counts` as `interpolation2.text`.
///
/// Deterministic: unigrams and bigrams walk the already-sorted [`Counts`]
/// maps. Records with a zero count, or whose phrase text cannot be
/// resolved, are skipped — the same filters as `gen_unigram` / `gen_bigram`
/// in `export_interpolation.cpp`. No λ line is written.
#[must_use]
pub fn emit_interpolation2(counts: &Counts, lexicon: &PhraseLexicon) -> String {
    let mut out = String::new();
    out.push_str(DATA_HEADER);
    out.push('\n');
    out.push_str("\\1-gram\n");
    for (&token, &count) in &counts.unigrams {
        if count == 0 {
            continue;
        }
        if let Some(text) = phrase_text(token, lexicon) {
            let _ = writeln!(out, "\\item {token} {text} count {count}");
        }
    }
    out.push_str("\\2-gram\n");
    for (&(prev, cur), &count) in &counts.bigrams {
        if count == 0 {
            continue;
        }
        let Some(word1) = phrase_text(prev, lexicon) else {
            continue;
        };
        let Some(word2) = phrase_text(cur, lexicon) else {
            continue;
        };
        let _ = writeln!(out, "\\item {prev} {word1} {cur} {word2} count {count}");
    }
    out.push_str("\\end\n");
    out
}

/// Writes [`emit_interpolation2`] to `path`.
///
/// # Errors
///
/// Returns [`EmitterError::Io`] when the path cannot be written.
pub fn write_interpolation2(
    path: &Path,
    counts: &Counts,
    lexicon: &PhraseLexicon,
) -> Result<(), EmitterError> {
    let text = emit_interpolation2(counts, lexicon);
    fs::write(path, text.as_bytes()).map_err(|source| EmitterError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use pinyin_counter::Counts;
    use pinyin_segment::PhraseLexicon;

    use super::{SENTENCE_START_TEXT, emit_interpolation2, phrase_text};

    fn sample() -> (Counts, PhraseLexicon) {
        let mut counts = Counts::default();
        counts.unigrams.insert(10, 5);
        counts.unigrams.insert(20, 7);
        counts.bigrams.insert((1, 10), 2);
        counts.bigrams.insert((10, 20), 3);
        let lexicon = PhraseLexicon::from_pairs(vec![(10, "甲".to_owned()), (20, "乙".to_owned())]);
        (counts, lexicon)
    }

    #[test]
    fn grammar_matches_export_interpolation() {
        let (counts, lexicon) = sample();
        let text = emit_interpolation2(&counts, &lexicon);
        assert_eq!(
            text,
            "\\data model interpolation\n\
             \\1-gram\n\
             \\item 10 甲 count 5\n\
             \\item 20 乙 count 7\n\
             \\2-gram\n\
             \\item 1 <start> 10 甲 count 2\n\
             \\item 10 甲 20 乙 count 3\n\
             \\end\n"
        );
    }

    #[test]
    fn lambda_is_not_written() {
        let (counts, lexicon) = sample();
        let text = emit_interpolation2(&counts, &lexicon);
        assert!(
            !text.to_ascii_lowercase().contains("lambda"),
            "emitter must not write λ (PR #55: λ ∈ table.conf): {text}"
        );
    }

    #[test]
    fn sentence_start_is_the_special_start_token() {
        let lexicon = PhraseLexicon::from_pairs(vec![(10, "甲".to_owned())]);
        assert_eq!(phrase_text(1, &lexicon), Some(SENTENCE_START_TEXT));
        assert_eq!(phrase_text(10, &lexicon), Some("甲"));
        assert_eq!(phrase_text(99, &lexicon), None);
    }

    #[test]
    fn zero_counts_and_missing_text_are_skipped() {
        let mut counts = Counts::default();
        counts.unigrams.insert(10, 0);
        counts.unigrams.insert(11, 4);
        counts.unigrams.insert(99, 8);
        counts.bigrams.insert((10, 11), 1);
        counts.bigrams.insert((99, 11), 1);
        let lexicon = PhraseLexicon::from_pairs(vec![(10, "甲".to_owned()), (11, "乙".to_owned())]);
        let text = emit_interpolation2(&counts, &lexicon);
        assert!(
            !text.contains("\\item 10 甲 count "),
            "zero-count unigram must be skipped"
        );
        assert!(text.contains("\\item 11 乙 count 4\n"));
        assert!(!text.contains("\\item 99 "), "missing-text unigram skipped");
        assert!(text.contains("\\item 10 甲 11 乙 count 1\n"));
        assert!(
            !text.contains("\\item 99 ") && !text.contains(" 99 "),
            "missing-text bigram endpoint skipped"
        );
    }

    #[test]
    fn parse_interpolation_dump_round_trips_values() {
        let (counts, lexicon) = sample();
        let text = emit_interpolation2(&counts, &lexicon);
        let parsed = pinyin_counter::parse_interpolation_dump(&text);
        assert_eq!(parsed, counts);
    }
}
