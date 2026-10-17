//! Building the profile fixtures by valid use of the public API.
//!
//! A populated profile is made the way a frontend makes one: phrases are imported
//! with `*_begin_add_phrases` / `*_iterator_add_phrase`, the data is saved
//! explicitly with `*_save`, and the context is closed. The pin is the producer;
//! the benchmark then opens a private copy of the result with each subject, and
//! the verification phase compares what each subject does with it.
//!
//! A fixture is accepted only if it was **shown to be loaded**: some input is
//! answered differently by a library opened on it than by one opened on the empty
//! profile. A profile that changes no answer proves nothing about restoration and
//! is refused.
//!
//! What this does not produce: selection-driven learning. The pin trains only
//! positions that carry a chosen-candidate constraint
//! (`PhoneticLookup::train_result3`, `src/lookup/phonetic_lookup.h` at 074a2219),
//! so a profile of *chosen and trained* candidates needs `*_choose_candidate`
//! driven first. The profiles here are imports through the public import API.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::digest::sha256_hex;
use crate::error::BenchError;
use crate::observe::Line;
use crate::workload::{Driver, PhraseRow};

/// The runbook's registered digest of the 10,000-row phrase input (count 5).
pub const REGISTERED_PHRASE_INPUT_SHA256: &str =
    "22637aee560c86761bb6d9d187d9af8b9fea970a6690bc21e21e5b6fc1cbb117";

/// Frequency each imported phrase carries in the `Phrases` and `Journals`
/// fixtures: large enough that a reopened profile answers differently (at 5 the
/// import changes no answer). It is a per-row frequency, not a number of rows;
/// both fixtures hold 10,000 rows.
pub const FIXTURE_FREQUENCY: i32 = 20_000;

/// Phrases per library in the balanced selection (libraries 1 to 4). The fourth
/// library has only 1,000 phrases the others lack; the total is the 10,000 rows
/// of the registered input.
pub const BALANCED_QUOTA: [usize; 4] = [3000, 3000, 3000, 1000];

/// A fixture level.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Level {
    /// A fresh, writable, empty directory.
    Empty,
    /// 80 import calls of only two distinct phrases (two phrases into each of
    /// the four system libraries, ten times each), frequency 5.
    Modest,
    /// 10,000 distinct system phrases taken in export order, frequency
    /// [`FIXTURE_FREQUENCY`].
    Phrases,
    /// 10,000 distinct system phrases drawn from all four libraries by
    /// [`BALANCED_QUOTA`], frequency [`FIXTURE_FREQUENCY`].
    Journals,
}

impl Level {
    /// Every level, the empty one first.
    pub const ALL: [Self; 4] = [Self::Empty, Self::Modest, Self::Phrases, Self::Journals];

    /// The fixture's directory name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Modest => "modest",
            Self::Phrases => "phrases",
            Self::Journals => "journals",
        }
    }
}

/// A row and the library it belongs to.
pub type Placed = (u8, PhraseRow);

/// The `modest` profile's rows.
#[must_use]
pub fn modest_rows() -> Vec<Placed> {
    let mut rows = Vec::new();
    for library in 1..=4_u8 {
        for _ in 0..10 {
            rows.push((
                library,
                PhraseRow {
                    phrase: "测试".into(),
                    pinyin: "ce4'shi4".into(),
                    count: 5,
                },
            ));
            rows.push((
                library,
                PhraseRow {
                    phrase: "测侧".into(),
                    pinyin: "ce4'ce4".into(),
                    count: 5,
                },
            ));
        }
    }
    rows
}

fn with_count(library: u32, row: &PhraseRow, count: i32) -> Result<Placed, BenchError> {
    let library = u8::try_from(library)
        .map_err(|_| BenchError::invalid(format!("library {library} out of range")))?;
    Ok((
        library,
        PhraseRow {
            phrase: row.phrase.clone(),
            pinyin: row.pinyin.clone(),
            count,
        },
    ))
}

/// The first pronunciation of the first 10,000 distinct phrases, in export
/// order, each at `count`.
///
/// # Errors
///
/// Fewer than 10,000 distinct phrases.
pub fn select_sequential(
    export: &[(u32, PhraseRow)],
    count: i32,
) -> Result<Vec<Placed>, BenchError> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (library, row) in export {
        if seen.insert(row.phrase.clone()) {
            out.push(with_count(*library, row, count)?);
            if out.len() == 10_000 {
                return Ok(out);
            }
        }
    }
    Err(BenchError::invalid(format!(
        "the export holds only {} distinct phrases",
        out.len()
    )))
}

/// Distinct phrases from each library by [`BALANCED_QUOTA`], each at `count`.
///
/// # Errors
///
/// A library with fewer new phrases than its quota.
pub fn select_balanced(export: &[(u32, PhraseRow)], count: i32) -> Result<Vec<Placed>, BenchError> {
    let mut seen = BTreeSet::new();
    let mut taken = [0_usize; 4];
    let mut per: [Vec<Placed>; 4] = Default::default();
    for (library, row) in export {
        let Some(slot) = usize::try_from(*library)
            .ok()
            .and_then(|l| l.checked_sub(1))
            .filter(|s| *s < 4)
        else {
            continue;
        };
        if taken[slot] >= BALANCED_QUOTA[slot] || !seen.insert(row.phrase.clone()) {
            continue;
        }
        taken[slot] += 1;
        per[slot].push(with_count(*library, row, count)?);
    }
    if (0..4).any(|i| taken[i] < BALANCED_QUOTA[i]) {
        return Err(BenchError::invalid(format!(
            "too few distinct phrases per library: {taken:?}"
        )));
    }
    Ok(per.into_iter().flatten().collect())
}

/// The tab-separated phrase input of the runbook (`library\tphrase\tpinyin\tcount`),
/// for its digest.
#[must_use]
pub fn phrase_input_text(rows: &[Placed]) -> String {
    rows.iter()
        .map(|(l, r)| format!("{l}\t{}\t{}\t{}\n", r.phrase, r.pinyin, r.count))
        .collect()
}

/// SHA-256 of [`phrase_input_text`].
#[must_use]
pub fn phrase_input_digest(rows: &[Placed]) -> String {
    sha256_hex(phrase_input_text(rows).as_bytes())
}

/// Exports every row of libraries 1 to 4 of the loaded system data.
///
/// # Errors
///
/// A failing library step.
pub fn export_system<D: Driver>(
    d: &mut D,
    system: &Path,
    scratch_user: &Path,
) -> Result<Vec<(u32, PhraseRow)>, BenchError> {
    if !d.init(system, scratch_user)? {
        return Err(BenchError::Call {
            function: "init (returned NULL)",
        });
    }
    let mut rows = Vec::new();
    for library in 1..=4_u32 {
        let loaded = u8::try_from(library).map_or(Ok(false), |l| d.load_library(l))?;
        let _ = loaded; // the C driver ignores this answer too
        for row in d.export_phrases(library)? {
            rows.push((library, row));
        }
    }
    d.fini()?;
    Ok(rows)
}

/// Imports `rows` into a fresh user directory, saves, and closes.
///
/// # Errors
///
/// A refused row, a `false` save, or any failed step.
pub fn populate<D: Driver>(
    d: &mut D,
    system: &Path,
    user: &Path,
    rows: &[Placed],
) -> Result<(), BenchError> {
    if !d.init(system, user)? {
        return Err(BenchError::Call {
            function: "init (returned NULL)",
        });
    }
    let mut start = 0;
    while start < rows.len() {
        let library = rows[start].0;
        let end = start
            + rows[start..]
                .iter()
                .take_while(|(l, _)| *l == library)
                .count();
        let run: Vec<PhraseRow> = rows[start..end].iter().map(|(_, r)| r.clone()).collect();
        let _ = d.load_library(library)?;
        let answers = d.add_phrases(library, &run)?;
        if let Some(i) = answers.iter().position(|ok| !ok) {
            return Err(BenchError::invalid(format!(
                "library {library} refused row {} ({:?})",
                start + i,
                run[i].phrase
            )));
        }
        start = end;
    }
    if !d.save()? {
        return Err(BenchError::Call {
            function: "save (false after imports)",
        });
    }
    d.fini()
}

/// Pinyin spellings worth trying as witnesses: the tone-stripped pronunciations
/// of the rows, shortest first, deduplicated, at most `limit`.
#[must_use]
pub fn candidate_inputs(rows: &[Placed], limit: usize) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (_, row) in rows {
        let text: String = row.pinyin.chars().filter(|c| !c.is_ascii_digit()).collect();
        if !text.is_empty() && text.len() <= 24 && seen.insert(text.clone()) {
            out.push(text);
            if out.len() == limit {
                break;
            }
        }
    }
    out
}

/// Groups a transcript by input (everything before the last `: ` of an `op`).
fn by_input(lines: &[Line]) -> BTreeMap<String, Vec<(&str, &str)>> {
    let mut map: BTreeMap<String, Vec<(&str, &str)>> = BTreeMap::new();
    for l in lines {
        if let Some((tag, what)) = l.op.rsplit_once(": ") {
            map.entry(tag.to_owned())
                .or_default()
                .push((what, l.value.as_str()));
        }
    }
    map
}

/// The inputs whose answers differ between two transcripts of the same script:
/// the evidence that a profile was loaded.
#[must_use]
pub fn discriminating(empty: &[Line], populated: &[Line]) -> Vec<String> {
    let (a, b) = (by_input(empty), by_input(populated));
    let mut out = Vec::new();
    for (tag, left) in &a {
        if b.get(tag) != Some(left) {
            out.push(tag.clone());
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::workload::testing::{Mock, Script};

    fn export(n: usize) -> Vec<(u32, PhraseRow)> {
        (0..n)
            .map(|i| {
                let library = u32::try_from(i * 4 / n).unwrap() + 1;
                (
                    library,
                    PhraseRow {
                        phrase: format!("p{i}"),
                        pinyin: format!("a{i}'b"),
                        count: -1,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn modest_is_eighty_imports_two_per_library_ten_times() {
        let rows = modest_rows();
        assert_eq!(rows.len(), 80);
        for library in 1..=4_u8 {
            assert_eq!(rows.iter().filter(|(l, _)| *l == library).count(), 20);
        }
        assert!(rows.iter().all(|(_, r)| r.count == 5));
    }

    #[test]
    fn sequential_takes_ten_thousand_distinct_phrases_first_pronunciation() {
        let mut ex = export(12_000);
        // A second pronunciation of an earlier phrase must be skipped.
        ex.insert(
            5,
            (
                1,
                PhraseRow {
                    phrase: "p0".into(),
                    pinyin: "other".into(),
                    count: 1,
                },
            ),
        );
        let rows = select_sequential(&ex, 7).unwrap();
        assert_eq!(rows.len(), 10_000);
        assert_eq!(
            rows.iter()
                .map(|(_, r)| r.phrase.clone())
                .collect::<BTreeSet<_>>()
                .len(),
            10_000
        );
        assert!(
            rows.iter()
                .all(|(_, r)| r.count == 7 && r.pinyin != "other")
        );
        assert!(select_sequential(&export(9_999), 7).is_err());
    }

    #[test]
    fn balanced_follows_the_quota_and_refuses_a_short_library() {
        let ex = export(40_000);
        let rows = select_balanced(&ex, 9).unwrap();
        assert_eq!(rows.len(), 10_000);
        for (i, q) in BALANCED_QUOTA.iter().enumerate() {
            assert_eq!(
                rows.iter()
                    .filter(|(l, _)| usize::from(*l) == i + 1)
                    .count(),
                *q
            );
        }
        let short: Vec<_> = export(40_000)
            .into_iter()
            .filter(|(l, _)| *l != 4)
            .collect();
        assert!(select_balanced(&short, 9).is_err());
    }

    #[test]
    fn the_phrase_input_digest_follows_the_runbook_recipe() {
        let rows = vec![(
            1_u8,
            PhraseRow {
                phrase: "锕".into(),
                pinyin: "a".into(),
                count: 5,
            },
        )];
        assert_eq!(phrase_input_text(&rows), "1\t锕\ta\t5\n");
        assert_eq!(
            phrase_input_digest(&rows),
            sha256_hex("1\t锕\ta\t5\n".as_bytes())
        );
    }

    #[test]
    fn populate_loads_each_library_imports_in_runs_saves_then_closes() {
        let script = Script::default();
        let log = script.log.clone();
        let mut m = Mock::new(script);
        let rows = vec![
            (
                1_u8,
                PhraseRow {
                    phrase: "a".into(),
                    pinyin: "a".into(),
                    count: 1,
                },
            ),
            (
                1,
                PhraseRow {
                    phrase: "b".into(),
                    pinyin: "b".into(),
                    count: 1,
                },
            ),
            (
                2,
                PhraseRow {
                    phrase: "c".into(),
                    pinyin: "c".into(),
                    count: 1,
                },
            ),
        ];
        populate(&mut m, Path::new("s"), Path::new("u"), &rows).unwrap();
        assert_eq!(
            *log.borrow(),
            ["init", "fini"],
            "the mock logs only the lifecycle calls"
        );
    }

    #[test]
    fn a_refused_row_or_an_unsaved_profile_is_an_error() {
        struct Refusing(Mock, bool);
        impl Driver for Refusing {
            fn facade(&self) -> crate::workload::Facade {
                self.0.facade()
            }
            fn init(&mut self, a: &Path, b: &Path) -> Result<bool, BenchError> {
                self.0.init(a, b)
            }
            fn set_options(&mut self, x: u32) -> Result<bool, BenchError> {
                self.0.set_options(x)
            }
            fn alloc_instance(&mut self) -> Result<bool, BenchError> {
                self.0.alloc_instance()
            }
            fn reset(&mut self) -> Result<bool, BenchError> {
                self.0.reset()
            }
            fn parse(
                &mut self,
                k: crate::workload::ParseKind,
                i: &str,
            ) -> Result<usize, BenchError> {
                self.0.parse(k, i)
            }
            fn parsed_length(&mut self) -> Result<usize, BenchError> {
                self.0.parsed_length()
            }
            fn guess_sentence(&mut self) -> Result<bool, BenchError> {
                self.0.guess_sentence()
            }
            fn sentence(&mut self, i: u8) -> Result<Option<String>, BenchError> {
                self.0.sentence(i)
            }
            fn guess_candidates(&mut self, o: usize, s: u32) -> Result<bool, BenchError> {
                self.0.guess_candidates(o, s)
            }
            fn candidate_count(&mut self) -> Result<Option<u32>, BenchError> {
                self.0.candidate_count()
            }
            fn candidate(
                &mut self,
                i: u32,
            ) -> Result<Option<crate::workload::Candidate>, BenchError> {
                self.0.candidate(i)
            }
            fn save(&mut self) -> Result<bool, BenchError> {
                Ok(self.1)
            }
            fn load_library(&mut self, l: u8) -> Result<bool, BenchError> {
                self.0.load_library(l)
            }
            fn add_phrases(&mut self, _: u8, rows: &[PhraseRow]) -> Result<Vec<bool>, BenchError> {
                Ok(vec![self.1; rows.len()])
            }
            fn export_phrases(&mut self, l: u32) -> Result<Vec<PhraseRow>, BenchError> {
                self.0.export_phrases(l)
            }
            fn free_instance(&mut self) -> Result<(), BenchError> {
                self.0.free_instance()
            }
            fn fini(&mut self) -> Result<(), BenchError> {
                self.0.fini()
            }
        }
        let rows = vec![(
            1_u8,
            PhraseRow {
                phrase: "a".into(),
                pinyin: "a".into(),
                count: 1,
            },
        )];
        let mut refusing = Refusing(Mock::new(Script::default()), false);
        let e = populate(&mut refusing, Path::new("s"), Path::new("u"), &rows).unwrap_err();
        assert!(e.to_string().contains("refused row 0"), "{e}");
    }

    #[test]
    fn witness_candidates_strip_tones_dedupe_and_respect_the_limit() {
        let rows = vec![
            (
                1_u8,
                PhraseRow {
                    phrase: "x".into(),
                    pinyin: "ce4'shi4".into(),
                    count: 1,
                },
            ),
            (
                1,
                PhraseRow {
                    phrase: "y".into(),
                    pinyin: "ce'shi".into(),
                    count: 1,
                },
            ),
            (
                1,
                PhraseRow {
                    phrase: "z".into(),
                    pinyin: "ai'he".into(),
                    count: 1,
                },
            ),
        ];
        assert_eq!(candidate_inputs(&rows, 10), ["ce'shi", "ai'he"]);
        assert_eq!(candidate_inputs(&rows, 1), ["ce'shi"]);
    }

    fn l(op: &str, value: &str) -> Line {
        Line {
            op: op.into(),
            value: value.into(),
        }
    }

    #[test]
    fn only_inputs_answered_differently_prove_the_profile_was_loaded() {
        let empty = vec![
            l("FullPinyin(\"ni\"): parse", "2"),
            l("FullPinyin(\"ni\"): candidate", "0:type=2:text=你"),
            l("FullPinyin(\"wo\"): parse", "2"),
        ];
        let same = empty.clone();
        assert!(discriminating(&empty, &same).is_empty());
        let mut other = empty.clone();
        other[1].value = "0:type=2:text=泥".into();
        assert_eq!(discriminating(&empty, &other), ["FullPinyin(\"ni\")"]);
        let mut shorter = empty.clone();
        shorter.pop();
        assert_eq!(discriminating(&empty, &shorter), ["FullPinyin(\"wo\")"]);
    }
}
