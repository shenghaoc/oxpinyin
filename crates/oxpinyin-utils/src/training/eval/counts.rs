//! Integer n-gram counts and the `export_interpolation` dump that carries them.
//!
//! The stored value is the raw integer count, never a probability
//! (`src/storage/ngram.cpp:33,145-146` at libpinyin 074a2219).

use std::collections::BTreeMap;

/// Raw unigram and bigram counts.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Counts {
    /// `token → count`.
    pub unigrams: BTreeMap<u32, u64>,
    /// `(prev, cur) → count`. `prev` is `sentence_start` at boundaries.
    pub bigrams: BTreeMap<(u32, u32), u64>,
}

/// Parses an `export_interpolation` dump, discarding the phrase-text columns.
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
