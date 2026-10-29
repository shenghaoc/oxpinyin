//! The W4 structure capture and its candidate-only projection.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use crate::OracleCandidateType;

/// The single committed candidate capture, relative to the repository root.
pub const STRUCTURE_FIXTURE: &str = "fixtures/w4/oracle-candidate-structure.txt";

/// One structure row, retaining the input verbatim and its captured rank.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StructuredCandidateRow {
    /// Exact corpus input, including punctuation and whitespace.
    pub input: String,
    /// One-based rank in the captured list.
    pub rank: usize,
    /// Candidate text; repeated text is retained.
    pub text: String,
    /// Public candidate type reported by the oracle.
    pub candidate_type: OracleCandidateType,
    /// Captured index, present only for an n-best candidate.
    pub nbest_index: Option<u8>,
}

/// Parsed structure capture. Rows retain file order and multiplicity.
#[derive(Clone, Debug)]
pub struct CandidateFixture {
    /// Pin identity copied from the capture header.
    pub pin_ref: String,
    /// Structured rows in capture order.
    pub rows: Vec<StructuredCandidateRow>,
}

impl CandidateFixture {
    /// Read a structure capture, reporting I/O or malformed-data errors.
    pub fn read(path: impl AsRef<Path>) -> io::Result<Self> {
        Self::parse(&std::fs::read_to_string(path)?)
    }

    /// Parse the five-column structure format without normalizing inputs.
    pub fn parse(raw: &str) -> io::Result<Self> {
        let invalid = |message| io::Error::new(io::ErrorKind::InvalidData, message);
        if raw.lines().next() != Some("# pinyin-oracle-candidate-structure-v1") {
            return Err(invalid("missing candidate structure v1 header"));
        }
        let pin_ref = raw
            .lines()
            .find_map(|line| line.strip_prefix("# pin_ref="))
            .filter(|pin| !pin.is_empty())
            .ok_or_else(|| invalid("missing pin_ref header"))?
            .to_owned();
        let mut rows = Vec::new();
        for (index, line) in raw.lines().enumerate() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let bad_row = || {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("malformed candidate structure row {}: {line:?}", index + 1),
                )
            };
            let fields: Vec<_> = line.split('\t').collect();
            let [input, rank, text, kind, nbest] = fields.as_slice() else {
                return Err(bad_row());
            };
            let rank = rank
                .parse::<usize>()
                .ok()
                .filter(|rank| *rank > 0)
                .ok_or_else(bad_row)?;
            let candidate_type = OracleCandidateType::from_wire(kind).ok_or_else(bad_row)?;
            let nbest_index = if *nbest == "-" {
                None
            } else {
                Some(nbest.parse::<u8>().map_err(|_| bad_row())?)
            };
            if nbest_index.is_some() != (candidate_type == OracleCandidateType::NbestMatch) {
                return Err(bad_row());
            }
            rows.push(StructuredCandidateRow {
                input: (*input).to_owned(),
                rank,
                text: (*text).to_owned(),
                candidate_type,
                nbest_index,
            });
        }
        Ok(Self { pin_ref, rows })
    }

    /// Candidate-only view in exactly the structure capture's order.
    pub fn candidates(&self) -> impl Iterator<Item = (&str, usize, &str)> {
        self.rows
            .iter()
            .map(|row| (row.input.as_str(), row.rank, row.text.as_str()))
    }

    /// Group the candidate-only view by exact input, retaining list order and duplicates.
    #[must_use]
    pub fn candidate_lists(&self) -> BTreeMap<String, Vec<String>> {
        let mut lists: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (input, _, text) in self.candidates() {
            lists
                .entry(input.to_owned())
                .or_default()
                .push(text.to_owned());
        }
        lists
    }
}
