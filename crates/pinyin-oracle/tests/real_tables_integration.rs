//! Integration: the exported tables through the real Session, over the
//! W2 parity corpus.
//!
//! Constructs `Session<SystemDictionary, BigramLanguageModel>` from the
//! tables `pinyin-migrate export` writes to `/tmp/pinyin-rs-export`, loads the
//! real unigram counts from `interpolation2.text` in the fetched model cache
//! (`tools/model/fetch-model.sh`), and compares candidates with the oracle's
//! **frozen** candidate lists at `fixtures/w4/oracle-candidates.txt`. Without
//! the model cache the parity measurement skips with a diagnostic — the
//! reproduced construction ranks by the real counts, so there is nothing
//! faithful to measure without them.
//!
//! Two tiers:
//!
//! - **Portable** (`real_tables_session_reports_parity`): no `oracle-ffi`,
//!   no Linux requirement. Loads `oracle-candidates.txt` (pin-stamped,
//!   depth 10, sorted) and reports top-1, top-5-set, prefix-10 and absent.
//!   This is the fast parity check every scoring change re-runs.
//!
//! - **Freshness** (`oracle_candidates_fixture_is_fresh`, `oracle-ffi`
//!   only): re-queries the live pin-built oracle over the whole corpus and
//!   asserts the frozen file matches exactly. Re-run
//!   `cargo run -p pinyin-oracle --features oracle-ffi --bin oracle_candidates`
//!   when the pin changes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pinyin_data::{BigramLanguageModel, SystemDictionary};
use pinyin_engine::{EmptyConfigSource, Session, StoragePaths};
use pinyin_oracle::corpus;

fn assert_sync_send<T: Sync + Send>() {}

/// Depth the capture protocol records candidates to.
const CAPTURE_DEPTH: usize = 10;

/// Relative path from the repository root to the frozen candidate fixture.
const CANDIDATES_FIXTURE: &str = "fixtures/w4/oracle-candidates.txt";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn export_dir() -> Option<PathBuf> {
    // `PINYIN_EXPORT_DIR` overrides the default so sandboxed runners can keep
    // the export inside a writable, persistent directory; the default matches
    // the documented `pinyin-migrate export` target.
    let dir = std::env::var_os("PINYIN_EXPORT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new("/tmp/pinyin-rs-export").to_path_buf());
    if ["pinyin_index.redb", "phrase_index.redb", "bigram.redb"]
        .iter()
        .all(|name| dir.join(name).exists())
    {
        Some(dir)
    } else {
        eprintln!(
            "exported tables not found at /tmp/pinyin-rs-export; skipping \
             (run pinyin-migrate export first)"
        );
        None
    }
}

/// Load every input from the committed W2 parity corpus strata.
///
/// Returns `None` when the corpus directory is missing or no stratum file
/// can be read — the caller skips rather than inventing a stand-in set.
fn load_w2_corpus() -> Option<Vec<String>> {
    let corpus_dir = repo_root().join(corpus::CORPUS_DIR);
    if !corpus_dir.is_dir() {
        eprintln!(
            "W2 parity corpus not found at {}; skipping real_tables integration",
            corpus_dir.display()
        );
        return None;
    }

    let mut all_inputs: Vec<String> = Vec::new();
    for stratum in corpus::generate() {
        let path = corpus_dir.join(stratum.file_name);
        match std::fs::read(&path) {
            Ok(bytes) => all_inputs.extend(corpus::Stratum::parse_file_bytes(&bytes)),
            Err(error) => {
                eprintln!(
                    "cannot read corpus stratum {}: {error}; skipping real_tables integration",
                    path.display()
                );
                return None;
            }
        }
    }

    if all_inputs.is_empty() {
        eprintln!(
            "W2 parity corpus at {} is empty; skipping real_tables integration",
            corpus_dir.display()
        );
        return None;
    }

    Some(all_inputs)
}

/// Parsed contents of `oracle-candidates.txt`.
struct CandidateFixture {
    /// Pin reference stamped in the header (`# pin_ref=...`).
    pin_ref: String,
    /// `input -> candidates` in rank order (1..n, n ≤ 10), deduplicated and sorted.
    by_input: BTreeMap<String, Vec<String>>,
    /// Raw file text (for freshness byte-for-byte check if desired).
    _raw: String,
}

fn load_candidate_fixture() -> Option<CandidateFixture> {
    let path = repo_root().join(CANDIDATES_FIXTURE);
    let raw = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        // A genuinely absent fixture is a skip: a checkout without the W4
        // fixtures still builds, and the freshness test regenerates it.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "candidate fixture not found at {}; skipping \
                 (run cargo run -p pinyin-oracle --features oracle-ffi --bin oracle_candidates)",
                path.display()
            );
            return None;
        }
        // Present but unreadable is a hard failure, not a silent skip.
        Err(error) => panic!(
            "candidate fixture at {} is present but unreadable: {error}",
            path.display()
        ),
    };

    let mut pin_ref = String::new();
    let mut by_input: BTreeMap<String, Vec<String>> = BTreeMap::new();

    // The fixture is a committed ~97k-line file. Once it is present it must
    // parse: a corruption that returned None would silently pass CI as a
    // skipped test, so every malformed-content case below panics.
    for line in raw.lines() {
        if let Some(value) = line.strip_prefix("# pin_ref=") {
            pin_ref = value.to_owned();
            continue;
        }
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        // input<TAB>rank<TAB>candidate_text ; input may be empty -> leading TAB
        let mut parts = line.split('\t');
        let input = parts.next().unwrap_or("").to_owned();
        let Some(rank_str) = parts.next() else {
            panic!("malformed fixture line (missing rank): {line:?}");
        };
        let Some(candidate) = parts.next() else {
            panic!("malformed fixture line (missing candidate): {line:?}");
        };
        let candidate = candidate.to_owned();
        assert!(
            parts.next().is_none(),
            "malformed fixture line (extra tab): {line:?}"
        );
        let Ok(rank) = rank_str.parse::<usize>() else {
            panic!("malformed rank {rank_str:?} in {line:?}");
        };
        let entry = by_input.entry(input.clone()).or_default();
        // File is sorted by input then rank ascending, so each push is next rank.
        assert_eq!(
            rank,
            entry.len() + 1,
            "fixture not sorted or rank gap for input {input:?} in {line:?}"
        );
        entry.push(candidate);
    }

    assert!(
        !pin_ref.is_empty(),
        "candidate fixture at {} is present but has no # pin_ref header",
        path.display()
    );

    Some(CandidateFixture {
        pin_ref,
        by_input,
        _raw: raw,
    })
}

fn real_candidates(session: &Session<&SystemDictionary, &BigramLanguageModel>) -> Vec<String> {
    session
        .candidates()
        .iter()
        .map(|c| c.text().to_owned())
        .collect()
}

fn type_input(session: &mut Session<&SystemDictionary, &BigramLanguageModel>, input: &str) {
    session.reset();
    // One refresh for the whole string: final candidates match per-keystroke
    // typing when nothing is selected mid-composition, and the full corpus
    // finishes in seconds rather than tens of minutes.
    let _ = session.type_pinyin(input);
}

#[derive(Default, Clone, Copy)]
struct Counts {
    total: usize,
    top1: usize,
    top5: usize,
    absent: usize,
    prefix_overlap: usize,
    prefix_depth: usize,
    /// Inputs whose emitted list contains at least one adjacent pair that ties
    /// on all three sort keys (the tie budget the stable sort absorbs).
    tie_inputs: usize,
    /// Total adjacent fully-tied pairs across all inputs.
    tie_pairs: usize,
    /// Inputs whose depth-10 candidate *set* equals the oracle's but whose
    /// order differs — the observable tie-swaps, where collection order (not
    /// the sort keys) decided the ranking.
    order_only: usize,
}

impl Counts {
    fn merge(a: Self, b: Self) -> Self {
        Self {
            total: a.total + b.total,
            top1: a.top1 + b.top1,
            top5: a.top5 + b.top5,
            absent: a.absent + b.absent,
            prefix_overlap: a.prefix_overlap + b.prefix_overlap,
            prefix_depth: a.prefix_depth + b.prefix_depth,
            tie_inputs: a.tie_inputs + b.tie_inputs,
            tie_pairs: a.tie_pairs + b.tie_pairs,
            order_only: a.order_only + b.order_only,
        }
    }
}

/// Counts how often the three-key sort had to fall back on collection order.
///
/// After the sort and dedup, two adjacent candidates tie on all three keys
/// exactly where the stable sort's collection-order rule decided their order.
/// `tie_pairs` counts those positions; `tie_inputs` counts inputs with at
/// least one. This is the measurable stand-in for the comparator-equal events
/// the construction's stable sort absorbs.
fn count_ties(
    session: &Session<&SystemDictionary, &BigramLanguageModel>,
    lm: &BigramLanguageModel,
) -> (bool, usize) {
    let candidates = session.candidates();
    let mut pairs = 0_usize;
    let keys: Vec<(usize, usize, u64)> = candidates
        .iter()
        .map(|candidate| {
            let length = candidate.text().chars().count();
            let span = candidate.consumed_bytes();
            let frequency = candidate
                .token()
                .and_then(|token| lm.unigram_count(token.value()))
                .unwrap_or(0);
            (length, span, frequency)
        })
        .collect();
    for pair in keys.windows(2) {
        if pair[0] == pair[1] {
            pairs += 1;
        }
    }
    (pairs > 0, pairs)
}

#[test]
fn real_tables_session_reports_parity() {
    let Some(dir) = export_dir() else { return };
    let Some(all_inputs) = load_w2_corpus() else {
        return;
    };
    let Some(fixture) = load_candidate_fixture() else {
        return;
    };

    if fixture.pin_ref != pinyin_oracle::EXPECTED_PIN_REF {
        eprintln!(
            "candidate fixture pin_ref {} does not match expected {}; \
             re-run oracle_candidates with the pinned oracle",
            fixture.pin_ref,
            pinyin_oracle::EXPECTED_PIN_REF
        );
    }

    eprintln!(
        "running real session over the W2 corpus ({} inputs) with {} against {}",
        all_inputs.len(),
        dir.display(),
        Path::new(CANDIDATES_FIXTURE).display()
    );

    // Shared read-only tables: open and prime once, borrow from every worker.
    assert_sync_send::<SystemDictionary>();
    assert_sync_send::<BigramLanguageModel>();
    let dict = SystemDictionary::open(
        &dir.join("pinyin_index.redb"),
        &dir.join("phrase_index.redb"),
    )
    .expect("SystemDictionary opens from the export");
    let mut lm =
        BigramLanguageModel::open(&dir.join("bigram.redb")).expect("BigramLanguageModel opens");

    // The pinned construction ranks candidates by the phrase index's *real*
    // unigram counts, which the export ABI does not carry (it reports a flat
    // 100 for every multi-character phrase). They live in interpolation2.text
    // in the fetched model cache. Without the cache there is nothing faithful
    // to measure against the real-frequency pins, so the test skips with a
    // diagnostic rather than fabricating numbers — the session itself keeps
    // working unchanged on the export-ABI counts, which is what the engine
    // tests cover.
    let model_dir = match pinyin_oracle::model_cache::locate_model_dir() {
        Ok(Some(model_dir)) => model_dir,
        Ok(None) => {
            eprintln!(
                "model cache absent: no interpolation2.text; skipping the \
                 real-frequency parity measurement (run tools/model/fetch-model.sh)"
            );
            return;
        }
        Err(error) => {
            panic!(
                "PINYIN_MODEL_DIR is set but unusable: {error}; \
                 run tools/model/fetch-model.sh"
            );
        }
    };
    {
        let interp = model_dir.join("interpolation2.text");
        lm.set_unigrams_from_interpolation2(&interp)
            .expect("interpolation2.text in the verified model cache parses");
        eprintln!("real unigram frequencies loaded from {}", interp.display());
    }
    let lm = lm; // freeze: init phase over, read-only from here
    let dict = &dict;
    let lm = &lm;

    let started = std::time::Instant::now();
    // Split the corpus across scoped threads: one Session per thread over the
    // shared tables, then fold the per-thread Counts with the
    // associative-commutative merge.
    // PARITY_SERIAL=1 forces a single worker so a pin mismatch can be
    // reproduced without the thread machinery in the picture.
    let n_threads = if std::env::var_os("PARITY_SERIAL").is_some() {
        1
    } else {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .min(all_inputs.len().max(1))
    };
    let chunk_size = all_inputs.len().div_ceil(n_threads);
    let fixture = &fixture;
    let Counts {
        total,
        top1,
        top5,
        absent,
        prefix_overlap,
        prefix_depth,
        tie_inputs,
        tie_pairs,
        order_only,
    } = std::thread::scope(|scope| {
        let handles: Vec<_> = all_inputs
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    let mut session =
                        Session::new(&EmptyConfigSource, StoragePaths::new("user"), dict, lm)
                            .expect("Session::new with shared tables");

                    let mut acc = Counts::default();
                    for input in chunk {
                        let Some(oracle_cands) =
                            fixture.by_input.get(input).filter(|l| !l.is_empty())
                        else {
                            continue;
                        };
                        type_input(&mut session, input);
                        let real_cands = real_candidates(&session);
                        let oracle_top = &oracle_cands[0];
                        let mut c = Counts {
                            total: 1,
                            ..Counts::default()
                        };
                        if real_cands.first() == Some(oracle_top) {
                            c.top1 = 1;
                        }
                        if real_cands.iter().take(5).any(|t| t == oracle_top) {
                            c.top5 = 1;
                        }
                        let real_prefix = &real_cands[..real_cands.len().min(CAPTURE_DEPTH)];
                        let oracle_prefix = &oracle_cands[..oracle_cands.len().min(CAPTURE_DEPTH)];
                        for cand in oracle_prefix {
                            c.prefix_depth += 1;
                            if real_prefix.contains(cand) {
                                c.prefix_overlap += 1;
                            }
                        }
                        // Same set, different order: the ranking difference is
                        // entirely a collection-order tie resolution.
                        let real_set: std::collections::HashSet<&str> =
                            real_prefix.iter().map(String::as_str).collect();
                        let oracle_set: std::collections::HashSet<&str> =
                            oracle_prefix.iter().map(String::as_str).collect();
                        if real_set == oracle_set && real_prefix != oracle_prefix {
                            c.order_only = 1;
                        }
                        if !real_cands.is_empty() && !real_cands.iter().any(|t| t == oracle_top) {
                            c.absent = 1;
                        }
                        let (tied_input, tied_pairs) = count_ties(&session, lm);
                        if tied_input {
                            c.tie_inputs = 1;
                        }
                        c.tie_pairs = tied_pairs;
                        acc = Counts::merge(acc, c);
                    }
                    acc
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("worker thread panicked"))
            .fold(Counts::default(), Counts::merge)
    });
    eprintln!("parallel decode wall-clock: {:?}", started.elapsed());

    let top1_pct = (top1 * 100).checked_div(total).unwrap_or(0);
    let top5_pct = (top5 * 100).checked_div(total).unwrap_or(0);
    let prefix_pct = (prefix_overlap * 100)
        .checked_div(prefix_depth)
        .unwrap_or(0);

    eprintln!("real tables parity — candidates, W2 parity corpus (fixture)");
    eprintln!("  fixture pin_ref         {}", fixture.pin_ref);
    eprintln!("  compared                {total}");
    eprintln!("  top-1                   {top1:>6}  {top1_pct}%");
    eprintln!("  top-5-set               {top5:>6}  {top5_pct}%");
    eprintln!("  prefix-10 overlap       {prefix_overlap:>6} of {prefix_depth}  {prefix_pct}%");
    eprintln!("  absent                  {absent:>6}");
    eprintln!("  three-key ties          {tie_pairs:>6} pairs across {tie_inputs} inputs");
    eprintln!("  tie-swaps (order-only) {order_only:>6} inputs of the depth-10 set");

    assert!(
        total > 0,
        "fixture produced no candidates over the W2 corpus; cannot report parity"
    );
    // Pinned to the real-frequency candidate construction: the expanding-
    // window scan over the parser-shaped key set (selected parse plus the
    // resplit/divided additions), the three-key order (text length, pinyin
    // span, real unigram count), and keep-first dedup. Measured release and
    // debug, serial and parallel — all bit-identical.
    assert_eq!(
        top1, 10136,
        "top-1 must be bit-identical to the serial baseline"
    );
    assert_eq!(
        absent, 1,
        "absent must be bit-identical to the serial baseline"
    );
    assert_eq!(
        top5, 10182,
        "top-5-set must be bit-identical to the serial baseline"
    );
    assert_eq!(
        prefix_overlap, 94456,
        "prefix-10 overlap numerator must match"
    );
    assert_eq!(
        prefix_depth, 98930,
        "prefix-10 overlap denominator must match"
    );
    assert!(
        top1 * 100 >= total * 55,
        "top-1 fell to {top1_pct}% ({top1}/{total}); expected >= 55%"
    );
    assert!(
        top5 * 100 >= total * 80,
        "top-5-set fell to {top5_pct}% ({top5}/{total}); expected >= 80%"
    );
    assert!(
        absent * 100 <= total * 4,
        "absent rose to {absent}/{total}; expected <= 4%"
    );
}

/// The scan's split parts are measured from the syllable text, not from the
/// byte the apostrophe rides on: `bu'tian` divides `tian` into `ti` + `an`,
/// so `补体` must consume exactly `bu'ti` (5 bytes) and leave `an`.
#[test]
fn scan_divided_key_consumes_the_apostrophe_span() {
    use pinyin_engine::Selection;

    let Some(dir) = export_dir() else {
        return;
    };
    let Ok(Some(model_dir)) = pinyin_oracle::model_cache::locate_model_dir() else {
        return;
    };

    let dict = SystemDictionary::open(
        &dir.join("pinyin_index.redb"),
        &dir.join("phrase_index.redb"),
    )
    .expect("SystemDictionary opens");
    let mut lm =
        BigramLanguageModel::open(&dir.join("bigram.redb")).expect("BigramLanguageModel opens");
    lm.set_unigrams_from_interpolation2(&model_dir.join("interpolation2.text"))
        .expect("interpolation2 parses");

    let mut session = Session::new(&EmptyConfigSource, StoragePaths::new("user"), &dict, &lm)
        .expect("Session::new");
    let _ = session.type_pinyin("bu'tian");

    let candidate = session
        .candidates()
        .iter()
        .find(|candidate| candidate.text() == "\u{8865}\u{4f53}")
        .expect("the divided `ti` offers `补体`");
    assert_eq!(candidate.consumed_bytes(), "bu'ti".len());

    let position = session
        .candidates()
        .iter()
        .position(|candidate| candidate.text() == "\u{8865}\u{4f53}")
        .expect("补体 is in the list");
    assert_eq!(
        session.select(position).expect("the index is live"),
        Selection::Continued
    );
    assert_eq!(session.preedit().text(), "\u{8865}\u{4f53}an");
}

/// Live-oracle freshness check for `fixtures/w4/oracle-candidates.txt`.
///
/// Re-queries the pin-built oracle over the whole W2 corpus (~3 hours) and
/// asserts the frozen file matches. Ignored by default; re-run with:
///
/// ```text
/// cargo test -p pinyin-oracle --features oracle-ffi \
///     --test real_tables_integration \
///     oracle_candidates_fixture_is_fresh -- --ignored --nocapture
/// ```
///
/// When the pin changes, regenerate first:
///
/// ```text
/// cargo run -p pinyin-oracle --features oracle-ffi --bin oracle-candidates
/// ```
#[cfg(feature = "oracle-ffi")]
#[test]
#[ignore = "full-corpus live oracle run (~3h); regenerate with bin oracle-candidates"]
fn oracle_candidates_fixture_is_fresh() {
    use pinyin_oracle::{Oracle, OracleFlags, OraclePrefix};

    let fixture =
        load_candidate_fixture().expect("candidate fixture missing; cannot check freshness");

    assert_eq!(
        fixture.pin_ref,
        pinyin_oracle::EXPECTED_PIN_REF,
        "fixture pin_ref {} does not match expected pin {}",
        fixture.pin_ref,
        pinyin_oracle::EXPECTED_PIN_REF
    );

    // Header total_triples must match the parsed payload.
    let path = repo_root().join(CANDIDATES_FIXTURE);
    let raw = std::fs::read_to_string(&path).expect("fixture readable");
    let mut header_triples: Option<usize> = None;
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("# total_triples=") {
            // header is "# total_triples=97442 (distinct ...)"
            let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            header_triples = num.parse().ok();
        }
    }
    let actual_triples: usize = fixture.by_input.values().map(Vec::len).sum();
    if let Some(expected) = header_triples {
        assert_eq!(
            expected, actual_triples,
            "header total_triples {expected} does not match actual {actual_triples}"
        );
    }

    // Re-query the live oracle for every distinct corpus input and compare.
    let prefix = OraclePrefix::locate().expect("oracle prefix");
    let mut oracle = Oracle::open_with_temp_user_dir(prefix).expect("oracle opens");
    let mut oracle_session = oracle
        .session(OracleFlags::DEFAULT)
        .expect("oracle session");

    let all_inputs = load_w2_corpus().expect("W2 corpus present for freshness check");
    let mut distinct: BTreeMap<String, ()> = BTreeMap::new();
    for input in all_inputs {
        distinct.entry(input).or_insert(());
    }

    let mut sample_mismatches: Vec<String> = Vec::new();
    let mut mismatch_count = 0_usize;
    let mut checked = 0_usize;
    let mut live_triples = 0_usize;
    let mut live_map: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for input in distinct.keys() {
        let obs = oracle_session
            .observe(input.as_bytes())
            .unwrap_or_else(|e| panic!("oracle observe failed for {input:?}: {e}"));
        let live: Vec<String> = obs.candidates.into_iter().take(CAPTURE_DEPTH).collect();
        live_triples += live.len();
        let frozen_list: &[String] = fixture
            .by_input
            .get(input)
            .map(Vec::as_slice)
            .unwrap_or(&[]);

        if live != frozen_list {
            mismatch_count += 1;
            if sample_mismatches.len() < 20 {
                sample_mismatches.push(format!(
                    "input {input:?}: live {live:?} vs frozen {frozen_list:?}"
                ));
            }
        }
        if !live.is_empty() {
            live_map.insert(input.clone(), live);
        }
        checked += 1;
    }

    eprintln!("freshness check — {checked} distinct inputs, {live_triples} live triples");
    eprintln!("  fixture pin_ref {}", fixture.pin_ref);
    eprintln!("  fixture triples {actual_triples}");

    assert!(
        mismatch_count == 0,
        "candidate fixture is stale ({mismatch_count} mismatches, showing {}): {}\n\
         re-run: cargo run -p pinyin-oracle --features oracle-ffi --bin oracle-candidates -- {}",
        sample_mismatches.len(),
        sample_mismatches.join("\n"),
        path.display()
    );

    // Payload-only regeneration check from the live map already collected above
    // (no second oracle pass). Header drift alone is not fatal once map equality
    // has passed.
    let mut regenerated_payload: Vec<String> = Vec::new();
    for (input, cands) in &live_map {
        for (idx, cand) in cands.iter().enumerate() {
            regenerated_payload.push(format!("{input}\t{}\t{cand}", idx + 1));
        }
    }
    let committed_payload: Vec<&str> = raw
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .collect();
    assert_eq!(
        committed_payload,
        regenerated_payload
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        "candidate payload differs; fixture must be regenerated"
    );
}
