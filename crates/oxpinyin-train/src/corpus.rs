//! The raw-corpus index (`title#textpath` lines) and its deterministic
//! traversal (`generate.py`/`segment.py`'s `for oneline in indexfile`).
//!
//! Each index line names one corpus document as `title#textpath`; the stages
//! walk the index in file order, resolving each `textpath` under a text root.
//! Reproduced here as a typed [`CorpusIndex`] of [`IndexEntry`] so traversal
//! is a plain iterator over typed records, not a re-split of raw lines at
//! every stage.

use std::path::{Path, PathBuf};

use crate::error::TrainError;

/// One corpus document: the human `title` and the `text_path` (relative to
/// the text root) upstream splits a `title#textpath` line into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexEntry {
    /// The document title (the part before `#`).
    pub title: String,
    /// The document path (the part after `#`), joined under the text root.
    pub text_path: String,
}

impl IndexEntry {
    /// The raw-corpus file for this entry under `text_dir`
    /// (`config.getTextDir() + textpath`).
    #[must_use]
    pub fn raw_path(&self, text_dir: &Path) -> PathBuf {
        join_text(text_dir, &self.text_path)
    }

    /// The segmented file for this entry (`… + getSegmentPostfix()`).
    #[must_use]
    pub fn segmented_path(&self, text_dir: &Path) -> PathBuf {
        let mut path = self.raw_path(text_dir).into_os_string();
        path.push(crate::config::SEGMENT_POSTFIX);
        PathBuf::from(path)
    }
}

/// Upstream concatenates `getTextDir()` and the `textpath` as strings, and
/// every `textpath` starts with a separator — so the join is a plain string
/// concatenation, not a `Path::join` (which would discard the text root for
/// an absolute `textpath`). Reproduce that by trimming a single leading
/// separator and joining as a child.
fn join_text(text_dir: &Path, text_path: &str) -> PathBuf {
    let trimmed = text_path.trim_start_matches(['/', '\\']);
    text_dir.join(trimmed)
}

/// A parsed corpus index: the entries in file order (the deterministic
/// traversal order every stage uses).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CorpusIndex {
    entries: Vec<IndexEntry>,
}

impl CorpusIndex {
    /// Parses index text: one `title#textpath` per non-empty line, in order.
    ///
    /// # Errors
    ///
    /// Returns [`TrainError::Malformed`] for a line without a `#`.
    pub fn parse(text: &str) -> Result<Self, TrainError> {
        let mut entries = Vec::new();
        for line in text.lines() {
            // `rstrip(os.linesep)` then `split('#')`; a blank line is skipped
            // (upstream would raise on a blank split, but real index files
            // have no blank lines — tolerating them is strictly safer).
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                continue;
            }
            let (title, text_path) = line.split_once('#').ok_or_else(|| TrainError::Malformed {
                detail: format!("index line has no '#': {line:?}"),
            })?;
            entries.push(IndexEntry {
                title: title.to_owned(),
                text_path: text_path.to_owned(),
            });
        }
        Ok(Self { entries })
    }

    /// Loads and parses an index file.
    ///
    /// # Errors
    ///
    /// Returns [`TrainError`] when the file cannot be read or a line is
    /// malformed.
    pub fn load(path: &Path) -> Result<Self, TrainError> {
        let text = std::fs::read_to_string(path).map_err(|error| TrainError::io(path, error))?;
        Self::parse(&text)
    }

    /// The entries, in deterministic file order.
    #[must_use]
    pub fn entries(&self) -> &[IndexEntry] {
        &self.entries
    }

    /// Number of documents in the index.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the index is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::CorpusIndex;
    use std::path::Path;

    #[test]
    fn parses_title_hash_path_in_order() {
        let index = CorpusIndex::parse("novel A#/a/1.text\nnovel B#/b/2.text\n").expect("parse");
        assert_eq!(index.len(), 2);
        assert_eq!(index.entries()[0].title, "novel A");
        assert_eq!(index.entries()[0].text_path, "/a/1.text");
        assert_eq!(index.entries()[1].text_path, "/b/2.text");
    }

    #[test]
    fn resolves_raw_and_segmented_paths_under_the_text_root() {
        let index = CorpusIndex::parse("t#/sub/doc.text\n").expect("parse");
        let entry = &index.entries()[0];
        let text_dir = Path::new("/corpus/texts");
        assert_eq!(
            entry.raw_path(text_dir),
            Path::new("/corpus/texts/sub/doc.text")
        );
        assert_eq!(
            entry.segmented_path(text_dir),
            Path::new("/corpus/texts/sub/doc.text.segmented")
        );
    }

    #[test]
    fn a_line_without_a_hash_is_an_error() {
        assert!(CorpusIndex::parse("no hash here\n").is_err());
    }

    #[test]
    fn blank_lines_are_skipped() {
        let index = CorpusIndex::parse("a#/1.text\n\nb#/2.text\n").expect("parse");
        assert_eq!(index.len(), 2);
    }
}
