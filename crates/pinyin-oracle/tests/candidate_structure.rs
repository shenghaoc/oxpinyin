//! W2-CAND: `fixtures/w4/oracle-candidate-structure.txt`.
//!
//! Two tiers, mirroring `real_tables_integration.rs`:
//!
//! - **Portable** (`structure_fixture_matches_sister_triples`): no `oracle-ffi`.
//!   Asserts the new fixture's `(input, rank, candidate_text)` columns are
//!   byte-identical to `oracle-candidates.txt`, and that every `type` /
//!   `nbest_index` value is well-formed and within the set the `0x1e`
//!   guess-candidates path can emit (`candidate-construction.md` §1.6).
//!
//! - **Freshness** (`structure_fixture_is_fresh`, `oracle-ffi` only): re-runs
//!   the capture over the whole W2 corpus and asserts the frozen file matches.
//!   Regenerate with
//!   `cargo run -p pinyin-oracle --features oracle-ffi --bin oracle-candidate-structure`.

use std::path::PathBuf;

const SISTER_FIXTURE: &str = "fixtures/w4/oracle-candidates.txt";
const STRUCTURE_FIXTURE: &str = "fixtures/w4/oracle-candidate-structure.txt";
#[cfg(feature = "oracle-ffi")]
const CAPTURE_DEPTH: usize = 10;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Reads a committed fixture, or `None` (a skip) when it is absent — a checkout
/// without the W4 fixtures still builds, and the freshness test regenerates it.
fn load(relative: &str) -> Option<String> {
    let path = repo_root().join(relative);
    match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("fixture not found at {}; skipping", path.display());
            None
        }
        Err(error) => panic!(
            "fixture at {} is present but unreadable: {error}",
            path.display()
        ),
    }
}

/// Payload (non-comment, non-empty) lines of a fixture, in file order.
fn payload(raw: &str) -> impl Iterator<Item = &str> {
    raw.lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
}

/// The pin reference stamped in a fixture's `# pin_ref=` header.
fn pin_ref(raw: &str) -> Option<&str> {
    raw.lines().find_map(|line| line.strip_prefix("# pin_ref="))
}

#[test]
fn structure_fixture_matches_sister_triples() {
    let Some(sister) = load(SISTER_FIXTURE) else {
        return;
    };
    let Some(structure) = load(STRUCTURE_FIXTURE) else {
        return;
    };

    // Both fixtures must be stamped by the same pin, and it must be the frozen
    // one; otherwise the two are not comparable.
    let sister_pin = pin_ref(&sister).expect("sister fixture has a # pin_ref header");
    let structure_pin = pin_ref(&structure).expect("structure fixture has a # pin_ref header");
    assert_eq!(
        sister_pin, structure_pin,
        "the two fixtures were captured under different pins"
    );
    assert_eq!(
        structure_pin,
        pinyin_oracle::EXPECTED_PIN_REF,
        "structure fixture pin_ref does not match the frozen pin"
    );

    // Sister triples: input <TAB> rank <TAB> candidate_text.
    let sister_triples: Vec<(&str, &str, &str)> = payload(&sister)
        .map(|line| {
            let mut parts = line.split('\t');
            let input = parts.next().unwrap_or_default();
            let rank = parts.next().expect("sister line has a rank field");
            let text = parts.next().expect("sister line has a candidate field");
            assert!(
                parts.next().is_none(),
                "sister line has extra tabs: {line:?}"
            );
            (input, rank, text)
        })
        .collect();

    // Structure records: input <TAB> rank <TAB> text <TAB> type <TAB> nbest_index.
    // Project to the triple for the equality check, and validate the two new
    // columns while we walk.
    let mut structure_triples: Vec<(&str, &str, &str)> = Vec::with_capacity(sister_triples.len());
    for line in payload(&structure) {
        let mut parts = line.split('\t');
        let input = parts.next().unwrap_or_default();
        let rank = parts.next().expect("structure line has a rank field");
        let text = parts.next().expect("structure line has a candidate field");
        let cand_type = parts.next().expect("structure line has a type field");
        let nbest = parts
            .next()
            .expect("structure line has an nbest_index field");
        assert!(
            parts.next().is_none(),
            "structure line has extra tabs: {line:?}"
        );

        // `type` is a frozen public enum name, and one the 0x1e path can emit:
        // LONGER is excluded by SORT_WITHOUT_LONGER_CANDIDATE and no PREDICTED_*
        // is produced without a guess_predicted call (§1.6).
        let parsed = pinyin_oracle::OracleCandidateType::from_wire(cand_type);
        assert!(
            parsed.is_some(),
            "unknown candidate type {cand_type:?} in {line:?}"
        );
        assert_ne!(
            cand_type, "LONGER_CANDIDATE",
            "LONGER must be sorted out: {line:?}"
        );
        assert!(
            !cand_type.starts_with("PREDICTED_"),
            "predicted types cannot appear on the guess_candidates path: {line:?}"
        );

        // `nbest_index` is `-` or a u8.
        assert!(
            nbest == "-" || nbest.parse::<u8>().is_ok(),
            "malformed nbest_index {nbest:?} in {line:?}"
        );
        // §7.2: the numeric index is present iff the candidate is NBEST_MATCH.
        assert_eq!(
            nbest != "-",
            cand_type == "NBEST_MATCH_CANDIDATE",
            "nbest_index is defined iff the candidate is NBEST_MATCH (§7.2): {line:?}"
        );

        structure_triples.push((input, rank, text));
    }

    assert_eq!(
        structure_triples.len(),
        sister_triples.len(),
        "triple counts differ: structure has {}, sister has {}",
        structure_triples.len(),
        sister_triples.len()
    );
    // The construction guarantee: the two share the same input set and the same
    // per-input candidate order, so the projected triples are identical. Walk in
    // step so a divergence names the first bad line instead of dumping ~97k rows.
    for (index, (structure_triple, sister_triple)) in
        structure_triples.iter().zip(&sister_triples).enumerate()
    {
        assert_eq!(
            structure_triple, sister_triple,
            "structure fixture (input, rank, text) diverges from the sister at \
             0-based line {index}: structure {structure_triple:?} vs sister {sister_triple:?}"
        );
    }
}

/// Live-oracle freshness check for the structure fixture.
///
/// ```text
/// cargo test -p pinyin-oracle --features oracle-ffi \
///     --test candidate_structure structure_fixture_is_fresh -- --ignored --nocapture
/// ```
#[cfg(feature = "oracle-ffi")]
#[test]
#[ignore = "full-corpus live oracle run; regenerate with bin oracle-candidate-structure"]
fn structure_fixture_is_fresh() {
    use std::collections::BTreeMap;

    use pinyin_oracle::corpus;
    use pinyin_oracle::{Oracle, OracleFlags, OraclePrefix};

    let Some(structure) = load(STRUCTURE_FIXTURE) else {
        panic!("structure fixture missing; cannot check freshness");
    };
    assert_eq!(
        pin_ref(&structure).expect("pin_ref header"),
        pinyin_oracle::EXPECTED_PIN_REF,
        "structure fixture pin_ref does not match the frozen pin"
    );

    // Committed payload, verbatim, in file order.
    let committed: Vec<&str> = payload(&structure).collect();

    let prefix = OraclePrefix::locate().expect("oracle prefix");
    let mut oracle = Oracle::open_with_temp_user_dir(prefix).expect("oracle opens");
    let mut session = oracle
        .session(OracleFlags::DEFAULT)
        .expect("oracle session");

    // Distinct corpus inputs, sorted bytewise, exactly as the capture dedups.
    let mut distinct: BTreeMap<String, ()> = BTreeMap::new();
    for stratum in corpus::generate() {
        for input in stratum.inputs {
            distinct.entry(input).or_insert(());
        }
    }

    let mut regenerated: Vec<String> = Vec::new();
    for input in distinct.keys() {
        let infos = session
            .observe_candidate_infos(input.as_bytes())
            .unwrap_or_else(|e| panic!("observe {input:?}: {e}"));
        for (idx, info) in infos.iter().take(CAPTURE_DEPTH).enumerate() {
            let nbest = info
                .nbest_index
                .map_or_else(|| "-".to_owned(), |index| index.to_string());
            regenerated.push(format!(
                "{input}\t{}\t{}\t{}\t{nbest}",
                idx + 1,
                info.text,
                info.candidate_type.as_wire()
            ));
        }
    }

    assert_eq!(
        committed,
        regenerated.iter().map(String::as_str).collect::<Vec<_>>(),
        "structure fixture payload differs from a fresh capture; regenerate it"
    );
}
