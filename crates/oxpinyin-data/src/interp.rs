//! Reader for the libpinyin `interpolation2.text` n-gram model export.
//!
//! The pinned model archive (`docs/findings/model-provenance.md`) carries the
//! phrase index's real unigram counts in `interpolation2.text`, a
//! CMU-Cambridge-style text n-gram file:
//!
//! ```text
//! \data model interpolation
//! \1-gram
//! \item <phrase-token> <text> count <count>
//! ...
//! \2-gram
//! \item <token> <text> <token> <text> count <count>
//! ...
//! \end
//! ```
//!
//! The phrase token is the same 32-bit `phrase_token_t` the exported tables
//! use, so a parsed record joins directly onto the dictionary entries. This
//! module reads only the `\1-gram` section: the system bigram already arrives
//! verbatim in `bigram.redb` (`docs/findings/data-layer-export.md`), and the
//! `\2-gram` section of the text export adds nothing the decoder does not
//! already have.
//!
//! # Grammar, and the one place this reader is stricter than upstream
//!
//! Lines are read through [`oxpinyin_data::interp_grammar`](crate::interp_grammar),
//! the shared port of libpinyin's `taglib_read` — the same module
//! `oxpinyin-datagen`'s model20 compile reads them through, so the two
//! cannot drift on what a line *means*. What each does with a
//! well-formed-but-contradictory record is deliberately different, and the
//! difference is the recorded one:
//!
//! * `import_interpolation` sums a repeated `\1-gram` token
//!   (`add_unigram_frequency` is `freq += delta`) and accepts a zero
//!   count. `oxpinyin-datagen`, which is that tool's port, reproduces
//!   both.
//! * This reader **refuses** both. It is not a port of the tool; it is the
//!   decoder's loader, and no conforming producer of this format can emit
//!   either record — `export_interpolation` walks tokens ascending and
//!   skips zero frequencies (`:85-95`), `k_mixture_model_to_interpolation`
//!   skips them too (`:132`), and `oxpinyin-emitter` reproduces both
//!   filters. A file carrying one is corrupt, and summing a corrupt model
//!   into the decoder loses the evidence that it was corrupt.
//!
//! `docs/findings/interpolation2-grammar.md` carries the measurements and
//! the rationale.
//!
//! The model archive is fetched at build time into an ignored cache
//! (`tools/model/fetch-model.sh`); nothing in this module bakes frequencies in
//! or discovers the cache path — the caller passes the extracted file.

use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use crate::interp_grammar::{self, Line, UNIGRAM_VALUES};

/// Why an interpolation2.text read failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum InterpolationError {
    /// The file could not be opened or read.
    Read {
        /// Path that was opened.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// A line did not parse under the frozen text format.
    Parse {
        /// 1-based line number inside the file.
        line: usize,
        /// What was wrong with the line.
        detail: String,
    },
    /// The `\1-gram` section was not found before the file ended.
    MissingOneGram,
}

impl fmt::Display for InterpolationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(formatter, "cannot read {}: {source}", path.display())
            }
            Self::Parse { line, detail } => {
                write!(formatter, "interpolation2.text line {line}: {detail}")
            }
            Self::MissingOneGram => {
                write!(formatter, "interpolation2.text has no \\1-gram section")
            }
        }
    }
}

impl std::error::Error for InterpolationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// The real unigram counts of the phrase index, sorted by phrase token.
///
/// Counts are the phrase's frequency in the corpus the model was estimated
/// from; `total` is the sum over every phrase, the phrase-index total the
/// decoder divides by. Both are read from the fetched archive, never baked in.
#[derive(Clone, Debug)]
pub struct UnigramTable {
    /// `(phrase_token, count)` sorted by token ascending. A token appears at
    /// most once; the parser rejects duplicates rather than silently picking
    /// one.
    records: Box<[(u32, u64)]>,
    /// Sum of all counts in `records`.
    total: u64,
}

impl UnigramTable {
    /// Number of phrases the table carries a real count for.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the table carries no phrases.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The phrase-index total: the sum of every phrase count.
    #[must_use]
    pub const fn total(&self) -> u64 {
        self.total
    }

    /// The real unigram count of `token`, or `None` when the phrase index has
    /// no such token.
    ///
    /// A `Some(0)` is impossible for a token in the file — counts are positive
    /// — and a token the file does not mention simply has no count. The caller
    /// decides how to rank a phrase the n-gram corpus never saw.
    #[must_use]
    pub fn count(&self, token: u32) -> Option<u64> {
        self.records
            .binary_search_by_key(&token, |&(token, _)| token)
            .ok()
            .map(|index| self.records[index].1)
    }

    /// The `(phrase_token, count)` records, sorted by token ascending.
    #[must_use]
    pub fn records(&self) -> &[(u32, u64)] {
        &self.records
    }
}

/// Parses the `\1-gram` section of an `interpolation2.text` model export.
///
/// Single pass over the file; the `\2-gram` section is skipped entirely.
/// Borrows nothing from the file: the table is a compact `(u32, u64)`
/// array sized for the ~64k-phrase model.
///
/// # Errors
///
/// Returns [`InterpolationError`] for an unreadable file, a malformed item
/// line inside the section, a duplicate token, or a file that ends before
/// the `\1-gram` header.
pub fn parse_interpolation2(path: &Path) -> Result<UnigramTable, InterpolationError> {
    let file = File::open(path).map_err(|source| InterpolationError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse_interpolation2_from_reader(path, BufReader::new(file))
}

/// Sorts the records by phrase token.
///
/// The section is not token-ordered, so a sort is unavoidable; sorting
/// 8-byte `(token, index)` pairs and gathering once moves less data
/// through the same comparison count than sorting the 16-byte records in
/// place.
fn sort_records(records: &mut [(u32, u64)]) {
    let count = u32::try_from(records.len()).unwrap_or(u32::MAX);
    let mut order: Vec<u32> = (0..count).collect();
    order.sort_unstable_by_key(|&index| records[index as usize].0);
    let source = records.to_vec();
    for (position, index) in order.into_iter().enumerate() {
        records[position] = source[index as usize];
    }
}

/// Which section of the export the reader is inside.
///
/// Mirrors `import_interpolation`'s `parse_body` / `parse_unigram` pair
/// (`utils/storage/import_interpolation.cpp:91-156`): an `\item` before
/// any `\N-gram` header is a line `parse_body` has no tag registered for,
/// which upstream answers with an `assert` abort.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Section {
    /// Before the first `\N-gram` header.
    Header,
    /// Inside `\1-gram` — the payload this reader consumes.
    Unigram,
    /// Inside `\2-gram` before ever seeing `\1-gram`. Reached only by a
    /// file that orders the sections the other way round, which no
    /// producer emits; its `\item` lines are skipped without being read,
    /// because this reader does not consume bigram records.
    Bigram,
}

/// Parses the `\1-gram` section from an already-open reader.
///
/// Same grammar as [`parse_interpolation2`], read through
/// [`crate::interp_grammar`]; same policy on duplicate tokens and zero
/// counts, which this reader refuses (see the module docs). `path` is
/// only used in [`InterpolationError::Read`].
///
/// # Errors
///
/// Returns [`InterpolationError`] for a read failure, a line the pinned
/// taglib grammar refuses, a duplicate token, a zero count, or a stream
/// that ends before the `\1-gram` header.
pub fn parse_interpolation2_from_reader<R: BufRead>(
    path: &Path,
    mut reader: R,
) -> Result<UnigramTable, InterpolationError> {
    let mut buffer = String::new();
    let mut line_number = 0_usize;
    let mut section = Section::Header;
    let mut saw_unigram = false;
    let mut records: Vec<(u32, u64)> = Vec::new();

    loop {
        buffer.clear();
        let read = reader
            .read_line(&mut buffer)
            .map_err(|source| InterpolationError::Read {
                path: path.to_path_buf(),
                source,
            })?;
        if read == 0 {
            break;
        }
        line_number += 1;
        let line = buffer.trim_end_matches(['\r', '\n']);

        // Bigram-before-unigram: only the next section tag matters, so
        // the ~1.9M item lines of that section are never field-parsed.
        // This is the one state where a line is not read through the
        // grammar, and it costs a `starts_with` rather than a walk.
        if section == Section::Bigram && !line.starts_with('\\') {
            continue;
        }

        let mut values = [""; UNIGRAM_VALUES];
        let parsed =
            interp_grammar::read(line, &mut values).map_err(|error| InterpolationError::Parse {
                line: line_number,
                detail: error.to_string(),
            })?;

        match parsed {
            // `parse_unigram` returns at `\2-gram` and `\end`
            // (`import_interpolation.cpp:144-147`). With the payload
            // already read there is nothing further this reader wants.
            Line::TwoGram | Line::End if saw_unigram => break,
            Line::TwoGram => section = Section::Bigram,
            Line::End => break,
            Line::OneGram => {
                section = Section::Unigram;
                saw_unigram = true;
            }
            // A `\data` line inside a section is `BEGIN_LINE` reaching
            // `parse_unigram`'s `default: abort()` (`:148-149`); only the
            // header line, consumed by `parse_headline` before the body
            // starts, is in the grammar.
            Line::Data { model } => {
                if section != Section::Header {
                    return Err(InterpolationError::Parse {
                        line: line_number,
                        detail: "repeated \\data header".to_owned(),
                    });
                }
                if model != "interpolation" {
                    return Err(InterpolationError::Parse {
                        line: line_number,
                        detail: format!("expected `model interpolation`, got {model:?}"),
                    });
                }
            }
            Line::Item { count } => {
                match section {
                    Section::Unigram => {}
                    Section::Bigram => continue,
                    Section::Header => {
                        return Err(InterpolationError::Parse {
                            line: line_number,
                            detail: "\\item before a \\N-gram header".to_owned(),
                        });
                    }
                }
                let token = values[0]
                    .parse::<u32>()
                    .map_err(|_| InterpolationError::Parse {
                        line: line_number,
                        detail: format!("phrase token {:?} is not a u32", values[0]),
                    })?;
                let count = count
                    .parse::<u64>()
                    .map_err(|_| InterpolationError::Parse {
                        line: line_number,
                        detail: format!("unigram count {count:?} is not a u64"),
                    })?;
                // Policy, not grammar: `import_interpolation` accepts a
                // zero count and `oxpinyin-datagen` reproduces that. See
                // the module docs for why the decoder's loader does not.
                if count == 0 {
                    return Err(InterpolationError::Parse {
                        line: line_number,
                        detail: "unigram count is zero".to_owned(),
                    });
                }
                records.push((token, count));
            }
        }
    }

    if !saw_unigram {
        return Err(InterpolationError::MissingOneGram);
    }

    sort_records(&mut records);
    // Policy, not grammar: upstream sums a repeated token
    // (`add_unigram_frequency` is `freq += delta`, `phrase_index.cpp:173`).
    if let Some(pair) = records.windows(2).find(|pair| pair[0].0 == pair[1].0) {
        return Err(InterpolationError::Parse {
            line: line_number,
            detail: format!("duplicate phrase token {}", pair[0].0),
        });
    }

    let total = records
        .iter()
        .fold(0_u64, |sum, &(_, count)| sum.saturating_add(count));

    Ok(UnigramTable {
        records: records.into_boxed_slice(),
        total,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::{InterpolationError, parse_interpolation2};

    /// One distinct name per test: tests run in parallel and each file is
    /// written once, read once, and removed, so the name is the collision
    /// domain.
    fn temp_file(name: &str, contents: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("pinyin-interp-{name}.text"));
        let mut file = std::fs::File::create(&path).expect("temp file");
        file.write_all(contents.as_bytes())
            .expect("write temp file");
        path
    }

    #[test]
    fn parses_the_one_gram_section_and_skips_the_rest() {
        let path = temp_file(
            "parses",
            "\\data model interpolation\n\
             \\1-gram\n\
             \\item 10 甲 count 5\n\
             \\item 20 乙 count 7\n\
             \\2-gram\n\
             \\item 10 甲 20 乙 count 3\n\
             \\end\n",
        );
        let table = parse_interpolation2(&path).expect("parses");
        std::fs::remove_file(&path).ok();
        assert_eq!(table.len(), 2);
        assert_eq!(table.total(), 12);
        assert_eq!(table.count(10), Some(5));
        assert_eq!(table.count(20), Some(7));
        assert_eq!(table.count(30), None);
    }

    #[test]
    fn a_missing_one_gram_section_is_an_error() {
        let path = temp_file(
            "missing-section",
            "\\data model interpolation\n\\2-gram\n\\end\n",
        );
        let result = parse_interpolation2(&path);
        std::fs::remove_file(&path).ok();
        assert!(matches!(result, Err(InterpolationError::MissingOneGram)));
    }

    #[test]
    fn a_malformed_item_line_is_an_error() {
        let path = temp_file(
            "malformed",
            "\\data model interpolation\n\\1-gram\n\\item 10 甲 5\n",
        );
        let result = parse_interpolation2(&path);
        std::fs::remove_file(&path).ok();
        assert!(matches!(
            result,
            Err(InterpolationError::Parse { line: 3, .. })
        ));
    }

    #[test]
    fn duplicate_tokens_are_rejected() {
        let path = temp_file(
            "duplicates",
            "\\data model interpolation\n\\1-gram\n\
             \\item 10 甲 count 5\n\\item 10 乙 count 7\n",
        );
        let result = parse_interpolation2(&path);
        std::fs::remove_file(&path).ok();
        assert!(matches!(result, Err(InterpolationError::Parse { .. })));
    }

    #[test]
    fn from_reader_rejects_a_zero_count() {
        let bytes = b"\\1-gram\n\\item 10 x count 0\n";
        let result = super::parse_interpolation2_from_reader(
            std::path::Path::new("memory"),
            std::io::Cursor::new(&bytes[..]),
        );
        assert!(matches!(
            result,
            Err(InterpolationError::Parse { line: 2, .. })
        ));
    }

    #[test]
    fn whitespace_in_the_phrase_text_follows_the_taglib_parity_rule() {
        // Before this reader shared `interp_grammar`, it took the last
        // two fields as `count <value>` and accepted both of these. The
        // pin refuses both (measured; see the findings document), and so
        // does `oxpinyin-datagen`.
        for line in [
            "\\item 10 \u{7532} \u{4e59} count 5",
            "\\item 10 x y z w count 5",
        ] {
            let text = format!("\\1-gram\n{line}\n");
            let result = super::parse_interpolation2_from_reader(
                std::path::Path::new("memory"),
                std::io::Cursor::new(text.as_bytes()),
            );
            assert!(
                matches!(result, Err(InterpolationError::Parse { line: 2, .. })),
                "expected {line:?} to be refused, got {result:?}"
            );
        }
        // The even-tail shape the pin accepts is accepted here too: the
        // second positional value is `a`, and `b`/`c` are dropped.
        let table = super::parse_interpolation2_from_reader(
            std::path::Path::new("memory"),
            std::io::Cursor::new(&b"\\1-gram\n\\item 10 a b c count 5\n"[..]),
        )
        .expect("even tail reads");
        assert_eq!(table.count(10), Some(5));
    }

    #[test]
    fn an_unrecognised_line_is_an_error_wherever_it_sits() {
        // Upstream's `taglib_read` refuses it and `check_result` aborts.
        // This reader used to ignore anything before `\1-gram` outright.
        for text in [
            "junk\n\\1-gram\n\\item 10 \u{7532} count 5\n",
            "\\1-gram\n\\item 10 \u{7532} count 5\njunk\n",
        ] {
            let result = super::parse_interpolation2_from_reader(
                std::path::Path::new("memory"),
                std::io::Cursor::new(text.as_bytes()),
            );
            assert!(
                matches!(result, Err(InterpolationError::Parse { .. })),
                "expected {text:?} to be refused, got {result:?}"
            );
        }
    }

    #[test]
    fn an_item_before_any_section_header_is_an_error() {
        let result = super::parse_interpolation2_from_reader(
            std::path::Path::new("memory"),
            std::io::Cursor::new(&b"\\item 10 x count 5\n\\1-gram\n"[..]),
        );
        assert!(matches!(
            result,
            Err(InterpolationError::Parse { line: 1, .. })
        ));
    }

    #[test]
    fn a_blank_line_is_refused_rather_than_dereferenced() {
        // `taglib_read` takes `tokens[0]` — NULL for a blank line — and
        // hands it to `strcmp`; the pin-built tool segfaults. Constitution
        // item 4: a typed error, never a crash.
        let result = super::parse_interpolation2_from_reader(
            std::path::Path::new("memory"),
            std::io::Cursor::new(&b"\\1-gram\n\n\\item 10 x count 5\n"[..]),
        );
        assert!(matches!(
            result,
            Err(InterpolationError::Parse { line: 2, .. })
        ));
    }
}
