//! Fixture pass rate for the provisional scoring configuration.
//!
//! For every frozen capture record that offers candidates, this takes the
//! pin's *own* selected segmentation, asks the scorer for the best phrase
//! spelling it, and compares that against the pin's first candidate. It
//! reports the rate rather than asserting a target, because the number is
//! evidence about machinery and not about parity:
//!
//! - the vocabulary is 90 authored phrases against the pin's tens of
//!   thousands, so most records have no phrase to find at all;
//! - the weights are provisional (`docs/findings/scoring-spec.md`);
//! - the pin ranks candidates across every segmentation of the input, which
//!   is W4-T4's composition, not this layer's.
//!
//! What it does pin is that where the fixture *does* cover a record, the
//! scorer agrees with the pin's first choice — and that a regression in the
//! weights shows up as a number that moved.

use oxpinyin_core::SyllableKey;
use oxpinyin_core::graph::{EdgeKind, SegmentGraph};
use oxpinyin_core::scoring::{Scorer, ScoringConfig};
use oxpinyin_testsupport::{FixtureDictionary, FixtureLanguageModel};

const F_A: &str = include_str!("../../../fixtures/foundation/f-a.txt");
const F_C: &str = include_str!("../../../fixtures/foundation/f-c.txt");
const VOCAB: &str = include_str!("../../../fixtures/w4/mini-vocab.txt");
const BIGRAM: &str = include_str!("../../../fixtures/w4/mini-bigram.txt");

/// Lowest share of covered records the scorer must agree with the pin on.
///
/// A floor, not a target: it exists so a weight change that breaks the
/// observed orderings fails the build instead of quietly moving a number.
const AGREEMENT_FLOOR: f64 = 0.90;

struct Case {
    input: Vec<u8>,
    keys: Vec<(SyllableKey, usize)>,
    first_candidate: String,
}

fn field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    line.split('\t')
        .filter_map(|field| field.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

fn decode_hex(text: &str) -> String {
    let bytes: Vec<u8> = (0..text.len() / 2)
        .map(|index| {
            u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).expect("capture hex is valid")
        })
        .collect();
    String::from_utf8(bytes).expect("capture candidates are UTF-8")
}

/// Capture records that offer at least one candidate and a usable path.
fn cases() -> Vec<Case> {
    let mut cases = Vec::new();

    for line in F_A.lines().chain(F_C.lines()) {
        let (Some(input), Some(segments), Some(candidates)) = (
            field(line, "input"),
            field(line, "segments"),
            field(line, "candidates_hex"),
        ) else {
            continue;
        };
        if candidates == "-" || segments == "-" {
            continue;
        }
        // Capture inputs are ASCII, and no escape appears in these families.
        if input.contains('\\') {
            continue;
        }

        let mut keys = Vec::new();
        let mut usable = true;
        for entry in segments.split(',') {
            let Some((text, rest)) = entry.rsplit_once('@') else {
                usable = false;
                break;
            };
            let begin: usize = rest
                .split(':')
                .next()
                .and_then(|value| value.parse().ok())
                .unwrap_or(usize::MAX);
            if let Some(key) = SyllableKey::from_text(text) {
                keys.push((key, begin));
            } else {
                usable = false;
                break;
            }
        }
        if !usable || keys.is_empty() {
            continue;
        }

        cases.push(Case {
            input: input.as_bytes().to_vec(),
            keys,
            first_candidate: decode_hex(candidates.split(',').next().expect("one candidate")),
        });
    }

    cases
}

/// The edge kinds the graph gives the pin's selected path.
fn kinds(graph: &SegmentGraph, keys: &[(SyllableKey, usize)]) -> Option<Vec<EdgeKind>> {
    let mut node = 0;
    let mut kinds = Vec::with_capacity(keys.len());
    for (key, begin) in keys {
        let id = graph.find_edge(node, *key, *begin)?;
        let edge = graph.edge(id)?;
        kinds.push(edge.kind());
        node = edge.to();
    }
    Some(kinds)
}

#[test]
fn the_capture_cases_are_readable() {
    let cases = cases();
    assert!(
        cases.len() >= 20,
        "F-A and F-C together offer candidates for more than twenty inputs, saw {}",
        cases.len()
    );
}

#[test]
fn the_scorer_agrees_with_the_pin_where_the_fixture_covers_it() {
    let dictionary = FixtureDictionary::parse(VOCAB).expect("committed fixture");
    let model = FixtureLanguageModel::parse(VOCAB, BIGRAM).expect("committed fixtures");
    let scorer = Scorer::new(ScoringConfig::default(), &dictionary, &model).expect("fixtures load");

    let mut total = 0_usize;
    let mut covered = 0_usize;
    let mut agreed = 0_usize;
    let mut cross_segmentation = 0_usize;
    let mut misses: Vec<(String, String, String)> = Vec::new();

    for case in cases() {
        total += 1;
        let graph = SegmentGraph::build(&case.input).expect("capture inputs are short");
        let Some(kinds) = kinds(&graph, &case.keys) else {
            continue;
        };
        let keys: Vec<SyllableKey> = case.keys.iter().map(|(key, _)| *key).collect();

        // The pin offers candidates for prefixes of the input as well as the
        // whole of it, so take the best over every prefix of its path.
        let mut best: Option<(String, i64)> = None;
        for length in 1..=keys.len() {
            let ranked = scorer
                .rank_phrases(&[], &keys[..length], &kinds[..length])
                .expect("the fixture backends do not fail");
            if let Some((entry, cost)) = ranked.first()
                && best.as_ref().is_none_or(|(_, seen)| cost < seen)
            {
                best = Some((entry.text().to_owned(), *cost));
            }
        }

        let Some((text, _)) = best else {
            continue;
        };
        covered += 1;
        if text == case.first_candidate {
            agreed += 1;
        } else if reachable_only_by_another_segmentation(&case, &keys) {
            cross_segmentation += 1;
        } else {
            misses.push((
                String::from_utf8_lossy(&case.input).into_owned(),
                case.first_candidate.clone(),
                text,
            ));
        }
    }

    let comparable = covered - cross_segmentation;
    let rate = if comparable == 0 {
        0.0
    } else {
        f64::from(u32::try_from(agreed).unwrap_or(u32::MAX))
            / f64::from(u32::try_from(comparable).unwrap_or(u32::MAX))
    };
    println!("scoring fixture pass rate");
    println!("  capture records with candidates    {total}");
    println!("  covered by the mini vocabulary     {covered}");
    println!("  needing another segmentation (T4)  {cross_segmentation}");
    println!("  comparable at this layer           {comparable}");
    println!("  top-1 agreeing with the pin        {agreed}");
    println!("  rate                               {rate:.3}");
    for (input, theirs, ours) in &misses {
        println!("  miss  {input:16} theirs={theirs} ours={ours}");
    }

    assert!(
        covered >= 8,
        "the fixture must cover something, saw {covered}"
    );
    assert!(
        misses.is_empty(),
        "the scorer disagrees with the pin on comparable records: {misses:?}"
    );
    assert!(
        rate >= AGREEMENT_FLOOR,
        "agreement fell to {rate:.3}, below the {AGREEMENT_FLOOR} floor"
    );
}

/// Whether the pin's first candidate is in the fixture, but only under a key
/// sequence this layer cannot reach.
///
/// The pin ranks candidates across every segmentation its graph admits: for
/// `xian` it opens with `西安` (`xi` + `an`) while its *selected* path is the
/// single key `xian`. Scoring one path cannot produce that, and it is not
/// meant to — W4-T4's composition ranks across the graph. Counting these as
/// failures of the scoring configuration would blame the wrong layer.
fn reachable_only_by_another_segmentation(case: &Case, path: &[SyllableKey]) -> bool {
    let prefixes: Vec<Vec<SyllableKey>> = (1..=path.len()).map(|n| path[..n].to_vec()).collect();

    VOCAB
        .lines()
        .filter(|line| line.contains('\t'))
        .filter(|line| field(line, "text") == Some(case.first_candidate.as_str()))
        .any(|line| {
            let keys: Option<Vec<SyllableKey>> = field(line, "keys")
                .expect("keys")
                .split(',')
                .map(SyllableKey::from_text)
                .collect();
            keys.is_some_and(|keys| !prefixes.contains(&keys))
        })
}
