//! The sentence-surface fixtures: the pin's answers and the exact ruled
//! subject residuals that `docs/findings/sentence-surface.md` §12 states.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The pin's surface for the sampled inputs, relative to the repository root.
pub const SENTENCE_FIXTURE: &str = "fixtures/w4/oracle-sentence-surface.txt";

/// Exact subject surfaces admitted by sentence-surface.md §12, in the same
/// four-column format as the unchanged pin fixture.
const RESIDUAL_FIXTURE: &str =
    "crates/oxpinyin-runtime/tests/fixtures/sentence-surface/residuals.txt";

/// Repository root, from this crate's manifest directory.
#[must_use]
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// One frozen oracle line: the ibus surface for one sampled input.
#[derive(Debug, PartialEq, Eq)]
pub struct OracleLine {
    pub input: String,
    pub guessed: bool,
    /// `pinyin_get_sentence(0..=proven)`; `-` marks a `None` read.
    pub sentences: Vec<String>,
    /// First-6 candidates as `type-letter/nbest/text`.
    pub rows: Vec<String>,
}

pub fn load_fixture(path: &Path) -> Result<Vec<OracleLine>, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read fixture {}: {error}", path.display()))?;
    let mut lines = Vec::new();
    for line in raw.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let parts: Vec<_> = line.split('\t').collect();
        let [input, guessed, sentences, rows] = parts.as_slice() else {
            return Err(format!("expected four fixture columns: {line:?}"));
        };
        let input = (*input).to_owned();
        let guessed = match *guessed {
            "true" => true,
            "false" => false,
            _ => return Err(format!("invalid fixture guess status: {line:?}")),
        };
        let sentences = split_field(sentences);
        let rows = split_field(rows);
        lines.push(OracleLine {
            input,
            guessed,
            sentences,
            rows,
        });
    }
    Ok(lines)
}

pub fn load_residuals(
    root: &Path,
    pin: &[OracleLine],
) -> Result<BTreeMap<String, OracleLine>, String> {
    let mut residuals = BTreeMap::new();
    for residual in load_fixture(&root.join(RESIDUAL_FIXTURE))? {
        let Some(original) = pin.iter().find(|line| line.input == residual.input) else {
            return Err(format!(
                "residual input absent from pin: {:?}",
                residual.input
            ));
        };
        if original.sentences.is_empty() || original == &residual {
            return Err(format!(
                "residual must differ on a comparable input: {:?}",
                residual.input
            ));
        }
        let input = residual.input.clone();
        if residuals.insert(input.clone(), residual).is_some() {
            return Err(format!("duplicate sentence residual: {input:?}"));
        }
    }
    Ok(residuals)
}

/// A `\x01`-joined fixture field; empty string means an empty list.
fn split_field(field: &str) -> Vec<String> {
    if field.is_empty() {
        Vec::new()
    } else {
        field.split('\u{1}').map(str::to_owned).collect()
    }
}

// The §12 current selection-residual table is the membership source;
// this supplementary gate must not introduce another list of names.
fn declared_sentence_residuals(root: &std::path::Path) -> std::collections::BTreeSet<String> {
    let doc = std::fs::read_to_string(root.join("docs/findings/sentence-surface.md"))
        .expect("the frozen sentence-surface declaration must be present");
    let section = doc
        .split_once("### #1378 current sentence-selection residuals")
        .expect("the current selection residual declaration must be present")
        .1;
    let table = section
        .split_once("| Input | Disposition |\n")
        .expect("the current residual table must be present")
        .1;
    let mut lines = table.lines();
    assert!(
        lines
            .next()
            .expect("table separator")
            .replace(' ', "")
            .starts_with("|---")
    );
    let mut names = std::collections::BTreeSet::new();
    for line in lines.take_while(|line| line.starts_with('|')) {
        let columns: Vec<_> = line.trim_matches('|').split('|').collect();
        assert_eq!(columns.len(), 2, "residual declaration format changed");
        let name = columns[0].trim().trim_matches('`');
        assert!(!name.is_empty() && names.insert(name.to_owned()));
    }
    assert_eq!(names.len(), 156, "declared residual membership count moved");
    names
}

#[derive(Debug, PartialEq, Eq)]
pub struct TieSurface {
    pub indexed: Vec<String>,
    // Every row has NBEST type 1; keep its original tail index and text.
    pub rows: Vec<(u8, String)>,
}

pub struct TieCase {
    pub input: String,
    pub guessed: bool,
    pub pin: TieSurface,
    pub residual: Option<TieSurface>,
}

fn decode_tie_surface(pool: &[&str], indexed: &str, rows: &str) -> TieSurface {
    let text = |id: &str| {
        pool.get(id.parse::<usize>().expect("numeric pool ID"))
            .expect("pool ID must be in range")
            .to_string()
    };
    let indexed: Vec<_> = if indexed.is_empty() {
        Vec::new()
    } else {
        indexed.split(',').map(text).collect()
    };
    assert!(indexed.len() <= 3, "pinyin n-best cap moved");
    let rows: Vec<_> = if rows.is_empty() {
        Vec::new()
    } else {
        rows.split(',')
            .map(|row| {
                let (rank, id) = row.split_once(':').expect("rank:pool-ID row");
                (rank.parse::<u8>().expect("numeric tail rank"), text(id))
            })
            .collect()
    };
    let mut seen = std::collections::BTreeSet::new();
    let complete: Vec<_> = indexed
        .iter()
        .enumerate()
        .filter(|(_, value)| seen.insert((*value).clone()))
        .map(|(rank, value)| (u8::try_from(rank).expect("n-best rank fits"), value.clone()))
        .collect();
    assert_eq!(
        rows, complete,
        "fixture must retain the complete NBEST surface"
    );
    TieSurface { indexed, rows }
}

pub fn load_tie_cases(root: &std::path::Path) -> Vec<TieCase> {
    let raw = std::fs::read_to_string(root.join("fixtures/w4/oracle-sentence-tie-order.txt"))
        .expect("the supplementary tie fixture must be present");
    assert!(
        raw.lines()
            .any(|line| line == "# format=sentence-tie-order-v1")
    );
    let declared = declared_sentence_residuals(root);
    let mut names = std::collections::BTreeSet::new();
    let mut recorded_residuals = std::collections::BTreeSet::new();
    let mut cases = Vec::new();
    for line in raw.lines().filter(|line| !line.starts_with('#')) {
        let columns: Vec<_> = line.split('\t').collect();
        assert_eq!(columns.len(), 7, "compact fixture column count moved");
        let input = columns[0].to_owned();
        assert!(names.insert(input.clone()), "duplicate fixture input");
        let guessed = match columns[1] {
            "0" => false,
            "1" => true,
            _ => panic!("fixture guess status must be 0 or 1"),
        };
        let pool: Vec<_> = columns[2].split('\u{1}').collect();
        let pin = decode_tie_surface(&pool, columns[3], columns[4]);
        let residual = if declared.contains(&input) {
            assert!(!columns[5].is_empty() && !columns[6].is_empty());
            assert!(recorded_residuals.insert(input.clone()));
            let surface = decode_tie_surface(&pool, columns[5], columns[6]);
            assert_ne!(
                surface, pin,
                "a declared residual silently became pin-equal"
            );
            Some(surface)
        } else {
            assert!(
                matches!(columns[5], "" | "-") && matches!(columns[6], "" | "-"),
                "an undeclared subject expectation was added for {input}"
            );
            None
        };
        cases.push(TieCase {
            input,
            guessed,
            pin,
            residual,
        });
    }
    assert_eq!(
        recorded_residuals, declared,
        "declared inputs must all be checked"
    );
    assert_eq!(cases.len(), 218, "selected distinct-input count moved");
    assert_eq!(
        cases.iter().map(|case| case.pin.rows.len()).sum::<usize>(),
        567
    );
    cases
}
