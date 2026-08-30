//! Integer n-gram counts and their canonical text serialization.
//!
//! The stored value is the raw integer count, never a probability
//! (`src/storage/ngram.cpp:33,145-146`; see `docs/findings/counter-port.md`).
//! Sorted maps make the dump deterministic byte-for-byte.

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Raw unigram and bigram counts, value-level parity target.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Counts {
    /// `token → count` (the `gen_unigram` freq-1 floor included).
    pub unigrams: BTreeMap<u32, u64>,
    /// `(prev, cur) → count`. `prev` is `sentence_start` at boundaries.
    pub bigrams: BTreeMap<(u32, u32), u64>,
}

impl Counts {
    /// Renders the canonical, value-only dump (sorted, deterministic).
    ///
    /// Mirrors the value shape of `export_interpolation`
    /// (`\1-gram` / `\2-gram` / `\item …`) but omits the phrase-text
    /// columns, which are display detail the counter does not reproduce.
    #[must_use]
    pub fn dump(&self) -> String {
        let mut out = String::new();
        out.push_str("\\data pinyin-counter\n");
        out.push_str("\\1-gram\n");
        for (token, count) in &self.unigrams {
            let _ = writeln!(out, "{token} {count}");
        }
        out.push_str("\\2-gram\n");
        for ((prev, cur), count) in &self.bigrams {
            let _ = writeln!(out, "{prev} {cur} {count}");
        }
        out.push_str("\\end\n");
        out
    }

    /// Parses the canonical dump back into counts.
    ///
    /// Lenient on unknown/`\item` lines (they are skipped) so the golden
    /// reader and the export reader stay in lockstep.
    #[must_use]
    pub fn parse_counter_dump(text: &str) -> Self {
        let mut counts = Self::default();
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

            let fields: Vec<&str> = line.split_whitespace().collect();
            let Some(count) = fields.last().and_then(|c| c.parse::<u64>().ok()) else {
                continue;
            };
            match section {
                Section::Unigram => {
                    if let Some(token) = fields.first().and_then(|t| t.parse::<u32>().ok()) {
                        counts.unigrams.insert(token, count);
                    }
                }
                Section::Bigram => {
                    let prev = fields.first().and_then(|t| t.parse::<u32>().ok());
                    let cur = fields.get(1).and_then(|t| t.parse::<u32>().ok());
                    if let (Some(prev), Some(cur)) = (prev, cur) {
                        counts.bigrams.insert((prev, cur), count);
                    }
                }
                Section::None => {}
            }
        }

        counts
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Section {
    None,
    Unigram,
    Bigram,
}

#[cfg(test)]
mod tests {
    use super::Counts;

    #[test]
    fn dump_round_trips() {
        let mut counts = Counts::default();
        counts.unigrams.insert(16_817_937, 5);
        counts.unigrams.insert(16_782_711, 3);
        counts.bigrams.insert((16_817_937, 16_782_711), 2);
        let text = counts.dump();
        assert_eq!(Counts::parse_counter_dump(&text), counts);
    }
}
