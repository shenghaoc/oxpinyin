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
//! The model archive is fetched at build time into an ignored cache
//! (`tools/model/fetch-model.sh`); nothing in this module bakes frequencies in
//! or discovers the cache path — the caller passes the extracted file.

use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Marker line that opens the section this reader consumes.
const ONE_GRAM_HEADER: &str = "\\1-gram";

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
                write!(formatter, "cannot read {path:?}: {source}")
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

    /// Installs records from a caller-built map, sorted by token for lookup.
    ///
    /// Used by [`crate::BigramLanguageModel`] to accept the existing
    /// `BTreeMap`-shaped `set_unigrams` API without keeping two storage shapes.
    pub(crate) fn from_map(records: std::collections::BTreeMap<u32, u64>) -> Self {
        let mut entries: Vec<(u32, u64)> = records.into_iter().collect();
        entries.sort_unstable_by_key(|&(token, _)| token);
        let total = entries
            .iter()
            .fold(0_u64, |sum, &(_, count)| sum.saturating_add(count));
        Self {
            records: entries.into_boxed_slice(),
            total,
        }
    }
}

/// Parses the `\1-gram` section of an `interpolation2.text` model export.
///
/// Single pass over the file, one allocation per record and one for the final
/// boxed slice; the `\2-gram` section is skipped entirely. Borrows nothing
/// from the file: the table is a compact `(u32, u64)` array sized for the
/// ~64k-phrase model.
///
/// # Errors
///
/// Returns [`InterpolationError`] for an unreadable file, a malformed item
/// line inside the section, a duplicate token, or a file that ends before the
/// `\1-gram` header.
pub fn parse_interpolation2(path: &Path) -> Result<UnigramTable, InterpolationError> {
    let file = File::open(path).map_err(|source| InterpolationError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse_interpolation2_from_reader(path, BufReader::new(file))
}

/// Parses the `\1-gram` section from an already-open reader.
///
/// Same validation as [`parse_interpolation2`]: missing `\1-gram`, zero
/// counts, and duplicate tokens are errors. `path` is only used in
/// [`InterpolationError::Read`].
///
/// # Errors
///
/// Returns [`InterpolationError`] for a read failure, a malformed item
/// line, a duplicate token, a zero count, or a stream that ends before
/// the `\1-gram` header.
pub fn parse_interpolation2_from_reader<R: BufRead>(
    path: &Path,
    mut reader: R,
) -> Result<UnigramTable, InterpolationError> {
    let mut buffer = String::new();
    let mut line_number = 0_usize;
    let mut in_section = false;
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

        if !in_section {
            in_section = line == ONE_GRAM_HEADER;
            continue;
        }
        // `\2-gram`, `\end` or any other section line ends the payload;
        // `\item` lines are the payload itself.
        if line.starts_with('\\') && !line.starts_with("\\item") {
            break;
        }
        if line.is_empty() {
            continue;
        }

        // `\item <id> <text...> count <count>`; the text is ignored because
        // the token already identifies the phrase. Fields are split so a
        // future phrase text containing spaces still parses.
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 5 || fields[0] != "\\item" || fields[fields.len() - 2] != "count" {
            return Err(InterpolationError::Parse {
                line: line_number,
                detail: format!("expected `\\item <id> <text> count <count>`, got {line:?}"),
            });
        }
        let token = fields[1]
            .parse::<u32>()
            .map_err(|_| InterpolationError::Parse {
                line: line_number,
                detail: format!("phrase token {:?} is not a u32", fields[1]),
            })?;
        let count =
            fields[fields.len() - 1]
                .parse::<u64>()
                .map_err(|_| InterpolationError::Parse {
                    line: line_number,
                    detail: format!("unigram count {:?} is not a u64", fields[fields.len() - 1]),
                })?;
        if count == 0 {
            return Err(InterpolationError::Parse {
                line: line_number,
                detail: "unigram count is zero".to_owned(),
            });
        }
        records.push((token, count));
    }

    if !in_section {
        return Err(InterpolationError::MissingOneGram);
    }

    records.sort_unstable_by_key(|&(token, _)| token);
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
}
