//! Kyoto Cabinet `#bnum` time-and-space measurement (shenghaoc/oxpinyin#402).
//!
//! The issue's arithmetic gives the space side deterministically. At the
//! class default `#bnum = 65,536` each open TreeDB reserves a leaf-cache
//! bucket array of `16 slots × nearbyprime(bnum/16+1) × 8 B` for the hot
//! half, the same for the warm half, and `16 × nearbyprime(bnum/AVGWAY/16+1)
//! × 8 B` for the inner cache — 1,082,240 B (1,057 KiB) per TreeDB. Drop
//! `#bnum` to 4,096 and the same arithmetic gives 82,048 B (80 KiB) — a
//! 1,000,192-byte saving per read-only TreeDB, or ~4.77 MiB across the five
//! system tables the engine holds open.
//!
//! What the issue leaves unmeasured is the time side. Fewer buckets means
//! longer hash chains inside each `LinkedHashMap` slot, so lookup cost may
//! rise from a page-cache hit. This bench is that time-side measurement.
//!
//! # Not a local-change proposal
//!
//! oxpinyin passes the same KC open parameters libpinyin does (no
//! `#bnum=`, `#msiz=`, `#pccap=` etc.), and that alignment is the
//! standing policy — external-library handling matches upstream. Tuning
//! `#bnum` locally would be a case-3 configuration divergence outside
//! `docs/findings/compatibility-policy.md`'s three exception classes and
//! is not on the table for this repo's shipping open path. The audience
//! for this bench's numbers is an upstream report — Kyoto Cabinet, or
//! libpinyin itself, which carries the same untuned default.
//!
//! # Design
//!
//! One criterion group per `#bnum` candidate, one bench inside it per
//! read-only system TreeDB. Each bench:
//!
//! 1. Opens the TreeDB at the group's `#bnum`.
//! 2. Walks a fixed number of keys from the file's ordering (evenly spaced
//!    strides so the sample covers the file), retained as a `Vec<Vec<u8>>`
//!    — the query set for this file.
//! 3. Warmup pass: iterates the query set once via `get`, discarding
//!    results, so the leaf-page cache is populated with the pages the
//!    timed pass will hit. This isolates the `#bnum` effect (bucket-array
//!    hash-chain length) from cold I/O.
//! 4. Timed pass (`iter_custom`): iterates the query set again, calling
//!    `KcStore::get_raw` on each key and summing wall time.
//!
//! `#pccap` — the eviction cap for cached leaf pages — is left at its
//! 64 MiB default throughout. The issue's Phase 2 diagnosis showed neither
//! engine comes near that cap, so `#pccap` is not the bytes lever the
//! shipping resident set spends; `#bnum` is. If a follow-up measurement
//! wants the `#pccap` axis, add it as a second `_TUNING` list.
//!
//! # What is not measured here
//!
//! * **HashDB (`bigram.db`).** `#bnum` on the hash database is a different
//!   arithmetic (the file-level buckets, not a `PlantDB` page cache), and
//!   the issue is scoped to the TreeDB page cache. HashDB is skipped.
//! * **Engine-level latency.** This is a backend-tier bench; `ChewingTable`
//!   and the engine's search matrix sit above the raw `get`. The store-tier
//!   number is the input to a full-engine bench, not a replacement for it.
//! * **The shipped default (tkrzw).** The `PlantDB` page cache is a Kyoto
//!   Cabinet structure; there is no reason for its arithmetic or its cost
//!   to transfer. This bench is Kyoto-Cabinet-scoped.
//!
//! # Reproducing
//!
//! Requires the `bench-internal` feature so the tuning helpers on
//! `KcStore` are visible. The data dimension the bench measures is
//! selected by an environment variable:
//!
//! Meaningful numbers need a Kyoto Cabinet-format libpinyin `data/`
//! directory containing all five system TreeDBs at production scale
//! (~10^4–10^5 records each). Fedora arm64's `libpinyin-data` is the
//! only shipping distro that packages it in KC format today — Debian
//! testing packages the same version against tkrzw — and lands the data
//! at `/usr/lib64/libpinyin/data`. The checked-in `fixtures/w3/kct/` has
//! ~10^2 records per TreeDB and is a harness smoke test only.
//!
//! ```sh
//! OXPINYIN_BNUM_DATA_DIR=/usr/lib64/libpinyin/data \
//!     cargo bench \
//!         --no-default-features --features kyotocabinet,bench-internal \
//!         --bench kyotocabinet_bnum
//! ```
//!
//! Do not pass `-- --bench` — `cargo bench` already appends `--bench` to
//! the criterion binary, and criterion 0.8's clap rejects the flag when
//! specified twice. To shorten a run, add `-- --sample-size N` etc.
//!
//! Runbook (containerised recipe, exact `apt` set, and where the machine
//! that has libpinyin-data lives): `docs/runbooks/benches.md`.
//!
//! Linux-only in practice: the resident-heap column reads
//! `/proc/self/status` VmHWM after opening all five system TreeDBs at each
//! candidate `#bnum` in a fresh child process, so on non-Linux hosts that
//! column reports `unavailable`. The timing column still runs; the
//! header arithmetic is authoritative on the space side either way, so
//! the missing resident column doesn't gate the measurement.
//!
//! No capture is committed. The commands above are what regenerate every
//! figure this bench produces.

#![allow(missing_docs)]
// The gate mirrors this target's `required-features`: cargo already skips
// the bench outside a `kyotocabinet` + `bench-internal` selection, but
// rust-analyzer analyzes required-features targets regardless and would
// flag the `#[cfg(feature = "bench-internal")]` helpers and the KC imports
// as unresolved under every other backend's feature set.
#![cfg(all(feature = "kyotocabinet", feature = "bench-internal"))]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, criterion_group};

use oxpinyin_store::{DEFAULT_STORE_EXT, KcStore};

/// `#bnum` values this bench measures. `None` opens without a `#bnum=`
/// override — the class default `65,536`.
const BNUM_CANDIDATES: &[Option<u32>] = &[None, Some(16_384), Some(4_096), Some(1_024)];

/// Keys sampled per system TreeDB. Small enough that the whole bench runs
/// under a minute per group, large enough to average out per-lookup jitter.
const KEYS_PER_DB: usize = 512;

/// The five read-only system TreeDBs, in libpinyin's own naming (see
/// `backend_matrix_*`'s `SYSTEM_DBMS` for the same list). `bigram.db` is
/// a HashDB and is not part of the `#bnum` measurement.
const TREE_DBMS: [(&str, &str); 5] = [
    ("pinyin_index", "pinyin_index.bin"),
    ("phrase_index", "phrase_index.bin"),
    ("punct", "punct.bin"),
    ("addon_pinyin_index", "addon_pinyin_index.bin"),
    ("addon_phrase_index", "addon_phrase_index.bin"),
];

/// The data directory the bench opens tables from.
///
/// - `OXPINYIN_BNUM_DATA_DIR` (env), if set, points at an installed
///   libpinyin `data/` — e.g. `/usr/lib/libpinyin/data` on Debian —
///   which is the *right* input for a #bnum measurement: production
///   files have ~10^4–10^5 records per TreeDB, so the leaf-cache
///   hash-chain length actually varies with `#bnum`. This is the input
///   the numbers that inform the #402 decision must come from.
/// - Otherwise the checked-in w3 fixture (`fixtures/w3/kct/`) is used,
///   which is fine for a smoke test of the bench harness but has far
///   too few records to exercise the `#bnum` axis. Runs against it are
///   not evidence for anything about the shipping open path.
fn fixture_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("OXPINYIN_BNUM_DATA_DIR") {
        PathBuf::from(dir)
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/w3")
            .join(DEFAULT_STORE_EXT)
    }
}

/// Returns the extra-tuning suffix for a `#bnum` choice, or `""` for the
/// default (which reproduces the shipping open path byte-for-byte).
fn tuning_for(bnum: Option<u32>) -> String {
    bnum.map(|n| format!("#bnum={n}")).unwrap_or_default()
}

/// Human-readable label for a `#bnum` choice.
fn label_for(bnum: Option<u32>) -> String {
    match bnum {
        None => "default_65536".to_string(),
        Some(n) => format!("bnum_{n}"),
    }
}

/// Walks the file's raw keyspace and samples up to `KEYS_PER_DB` keys with
/// a stride derived from the record count, so the sample covers the file
/// evenly rather than clustering at the front. The keys are copied out —
/// the cursor is closed before the sample is returned.
fn sample_keys(store: &KcStore) -> Vec<Vec<u8>> {
    // Collect every key first — the read-only KC benches already tolerate
    // this cost, and it lets `sample_keys` pick evenly across the file
    // without a two-pass walk. The five system TreeDBs are small enough
    // (~5 MB largest) that this fits comfortably.
    let mut all: Vec<Vec<u8>> = Vec::new();
    store
        .walk_raw_keys(|key| {
            all.push(key.to_vec());
            Ok(())
        })
        .expect("walk raw keys");
    if all.is_empty() {
        return Vec::new();
    }
    if all.len() <= KEYS_PER_DB {
        return all;
    }
    let stride = all.len() / KEYS_PER_DB;
    (0..KEYS_PER_DB).map(|i| all[i * stride].clone()).collect()
}

/// Reads `VmHWM` from `/proc/self/status`. Linux-only.
#[cfg(target_os = "linux")]
fn vm_hwm_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|kib| kib.parse().ok())
}

#[cfg(not(target_os = "linux"))]
fn vm_hwm_kib() -> Option<u64> {
    None
}

/// Opens all five read-only system TreeDBs at the given `#bnum`, prints
/// `VmHWM` on stdout, then exits. Runs as its own process so the reading
/// is *this* group's peak, not the whole bench binary's cumulative peak.
fn run_resident_child(bnum: Option<u32>) {
    let fixture = fixture_dir();
    let tuning = tuning_for(bnum);
    let stores: Vec<KcStore> = TREE_DBMS
        .iter()
        .map(|(_stem, file)| {
            let path = fixture.join(file);
            KcStore::open_read_only_tuned(&path, &tuning).unwrap_or_else(|error| {
                panic!(
                    "open {file} with tuning {tuning:?}: {error}",
                    file = path.display()
                )
            })
        })
        .collect();
    let hwm = vm_hwm_kib();
    // Keep every handle live through the VmHWM read: production holds
    // all six system DBMs at once, so closing them before the read
    // would measure a different quantity.
    let hwm_txt = match hwm {
        Some(k) => format!("{k}"),
        None => "unavailable".to_string(),
    };
    println!("vmhwm_kib={hwm_txt}");
    drop(stores);
}

/// Runs one `report_resident` child per `#bnum` and prints a small
/// resident table on stderr before criterion begins. Each child sees only
/// its own opens' resident footprint — no leakage across configurations.
fn report_resident_all(bnum_values: &[Option<u32>]) {
    let exe = std::env::current_exe().expect("current_exe");
    eprintln!("kyotocabinet_bnum resident: five-treedbs VmHWM per process");
    eprintln!("  bnum              vmhwm_kib");
    for &bnum in bnum_values {
        let out = std::process::Command::new(&exe)
            .env("OXPINYIN_BNUM_DATA_DIR", fixture_dir())
            .arg("--resident-child")
            .arg(match bnum {
                None => "default".to_string(),
                Some(n) => n.to_string(),
            })
            .output()
            .expect("spawn resident child");
        assert!(
            out.status.success(),
            "resident child for bnum={bnum:?} failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let vmhwm = String::from_utf8_lossy(&out.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("vmhwm_kib="))
            .unwrap_or("?")
            .to_string();
        eprintln!("  {:<16}  {}", label_for(bnum), vmhwm);
    }
}

fn bench_bnum(c: &mut Criterion) {
    let fixture = fixture_dir();
    assert!(
        fixture.is_dir(),
        "missing fixture dir {}",
        fixture.display()
    );

    // Space column first: spawn a child of ourselves per `#bnum` so each
    // reading is that group's peak in a fresh process, not the parent's
    // cumulative peak across every group and criterion's own bookkeeping.
    // Runs before the timing groups so a long timing pass cannot
    // contaminate the resident reading with its own working set.
    report_resident_all(BNUM_CANDIDATES);

    // Time column: one group per `#bnum` candidate, one bench per system
    // TreeDB. Iterations run `KEYS_PER_DB` calls of `get_raw` over the
    // pre-sampled key set, timed as one custom iteration.
    for &bnum in BNUM_CANDIDATES {
        let tuning = tuning_for(bnum);
        let group_name = format!("kyotocabinet_bnum/{}", label_for(bnum));
        let mut group = c.benchmark_group(&group_name);
        for (stem, file) in TREE_DBMS {
            let path = fixture.join(file);
            // Sample keys from a default-tuned open — the sample is not
            // sensitive to `#bnum`, only to the file's contents, and using
            // the default here keeps the sample identical across groups.
            let sample = {
                let sampler = KcStore::open_read_only_tuned(&path, "").expect("open sampler");
                sample_keys(&sampler)
            };
            assert!(!sample.is_empty(), "empty sample from {}", path.display());

            group.bench_with_input(
                BenchmarkId::new(stem, sample.len()),
                &sample,
                |b, sample| {
                    b.iter_custom(|iters| {
                        // Open once per criterion batch: the leaf-cache
                        // bucket array is what `#bnum` sizes, and it is
                        // allocated at open. Reopening per iteration would
                        // add malloc overhead to the timing signal.
                        let store =
                            KcStore::open_read_only_tuned(&path, &tuning).expect("open tuned");

                        // Warmup: page every leaf we're about to probe
                        // into KC's page cache, so the timed pass measures
                        // hash-chain traversal (the `#bnum` effect) rather
                        // than disk I/O.
                        for key in sample {
                            let _ = store.get_raw_bench(key);
                        }

                        let mut total = Duration::ZERO;
                        for _ in 0..iters {
                            let started = Instant::now();
                            for key in sample {
                                std::hint::black_box(store.get_raw_bench(key));
                            }
                            total += started.elapsed();
                        }
                        total
                    });
                },
            );
        }
        group.finish();
    }
}

/// Wraps `RawReadStore::get_raw` in a form the bench can call directly.
///
/// `KcStore` implements `RawReadStore`, but the trait method is `unsafe`
/// to bring into scope in a bench file (it needs the trait imported). The
/// wrapper's only purpose is to give the bench a straight method call.
trait BenchGet {
    fn get_raw_bench(&self, key: &[u8]) -> Option<Vec<u8>>;
}

impl BenchGet for KcStore {
    fn get_raw_bench(&self, key: &[u8]) -> Option<Vec<u8>> {
        use oxpinyin_store::RawReadStore;
        RawReadStore::get_raw(self, key).expect("get_raw")
    }
}

criterion_group!(benches, bench_bnum);

/// Wrapper `main` that handles the `--resident-child <bnum>` self-exec
/// before falling through to criterion. The child mode is used by
/// [`report_resident_all`] to measure each `#bnum`'s resident footprint
/// in a fresh process; anything else runs the normal criterion suite via
/// [`criterion_main`]-generated body.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--resident-child") {
        let arg = args
            .get(pos + 1)
            .expect("--resident-child needs a bnum argument");
        let bnum = match arg.as_str() {
            "default" => None,
            n => Some(
                n.parse()
                    .expect("--resident-child bnum must be a u32 or 'default'"),
            ),
        };
        run_resident_child(bnum);
        return;
    }
    // criterion's own dispatch — same code criterion_main! would expand to.
    benches();
    Criterion::default().configure_from_args().final_summary();
}
