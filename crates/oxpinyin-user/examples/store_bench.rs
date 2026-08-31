//! End-to-end `GenericUserStore` bench for the compiled peer backend.
//!
//! Measurement only; consumes public APIs and touches no parity code.
//! Under the exactly-one-backend invariant one peer is compiled per
//! build; this bench measures that peer through
//! [`GenericUserStore::create_standalone`](oxpinyin_user::GenericUserStore::create_standalone),
//! which bypasses the process-global handle registry.  A KC-vs-<peer>
//! comparison is done by running the same bench in two fresh builds
//! and reading the two output tables — the cross-peer comparison
//! historically done in-process is no longer possible in one build.
//!
//! Run in release under each peer selection to compare them:
//!
//! ```text
//! cargo run -p oxpinyin-user --release --example store_bench
//! cargo run -p oxpinyin-user --release --example store_bench \
//!     --no-default-features --features lmdb
//! ```
//!
//! Each scenario runs in a child process (a re-exec of this binary) so
//! VmHWM measures that scenario alone; the child baseline is common to
//! every scenario.  Training pairs derive deterministically from the
//! seed, and every scenario body is one generic function over
//! `S: WriteStore` — the peer set differs only in `S`.
//!
//! The point of interest: the decode-time count memo should flatten the
//! raw storage delta measured by `backend_bench` (point_get /
//! prefix_scan in `crates/oxpinyin-store/examples/backend_bench.rs`).
//! Compare `first_query_ms` and `us_per_cached_query` against
//! `backend_bench`'s `us_per_get`.
//!
//! On Unix, on-disk sizes report both apparent length (`st_size`) and
//! allocated blocks (`st_blocks` × 512) of the data file only; allocated is
//! the comparison figure, and apparent >> allocated reveals a sparse file.
//! On other platforms, allocated bytes are reported as zero (unavailable).
//!
//! Sizes via env vars (defaults in parentheses): `STORE_BENCH_TRAIN`
//! (`5_000` `observe_selection` calls — each is its own committed write
//! transaction at the backend's default durability), `STORE_BENCH_READS`
//! (`50_000` `count_delta` queries), `STORE_BENCH_PREDICTED` (200
//! `observe_predicted` acceptances), `STORE_BENCH_SEED`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use oxpinyin_store::{DefaultStore, WriteStore};
use oxpinyin_user::GenericUserStore;

const SCENARIOS: [&str; 3] = ["train", "save", "read"];

// Token domains: a sentence-local predecessor set and a candidate
// vocabulary, shaped like a decode session hitting the user store.
const TOKEN_BASE: u32 = 0x0100_0000;
const PREV_DOMAIN: u32 = 512;
const CUR_DOMAIN: u32 = 8_192;

/// The peer this build is measuring, taken from the store's own
/// extension — one authoritative name (`kct` / `redb` / `lmdb` / `tkt`).
const PEER_LABEL: &str = oxpinyin_store::DEFAULT_STORE_EXT;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |n: usize| args.get(n).map(String::as_str);
    match (arg(1), arg(2)) {
        (Some("--child"), Some(scenario)) => run_child(scenario),
        _ => parent(),
    }
}

// ── parent: spawn one child per scenario, print the table ─────────

fn parent() {
    let cfg = config();
    println!("store_bench — GenericUserStore end-to-end, peer = {PEER_LABEL}");
    println!(
        "train={}  reads={}  predicted={}  seed={:#x}",
        cfg.train, cfg.reads, cfg.predicted, cfg.seed
    );
    println!("one fresh process per scenario; VmHWM per scenario");
    println!();

    for scenario in SCENARIOS {
        let rows = spawn_child(scenario);
        print_block(scenario, &rows);
    }
}

fn spawn_child(scenario: &str) -> Vec<(String, String)> {
    let exe = std::env::current_exe().expect("current exe");
    let output = Command::new(exe)
        .args(["--child", scenario])
        .output()
        .expect("spawn bench child");
    if !output.status.success() {
        eprintln!(
            "child {PEER_LABEL}/{scenario} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::process::exit(1);
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

fn print_block(scenario: &str, rows: &[(String, String)]) {
    println!("── {scenario} ──");
    println!("  {:<26} {:>14}", "metric", PEER_LABEL);
    for (key, value) in rows {
        println!("  {key:<26} {value:>14}");
    }
    println!();
}

// ── child: one scenario in this process against the compiled peer ─

fn run_child(scenario: &str) {
    dispatch::<DefaultStore>(scenario);
}

fn dispatch<S: WriteStore>(scenario: &str) {
    match scenario {
        "train" => train::<S>(),
        "save" => save::<S>(),
        "read" => read::<S>(),
        other => {
            eprintln!("unknown scenario {other:?}");
            std::process::exit(2);
        }
    }
}

fn train<S: WriteStore>() {
    let cfg = config();
    let path = work_path("train");
    let mut store = open::<S>(&path);

    let started = Instant::now();
    for i in 0..cfg.train as u64 {
        let (last, cur) = training_pair(cfg.seed, i);
        store
            .observe_selection(last, cur)
            .expect("observe_selection");
    }
    let training = started.elapsed();

    emit("calls", cfg.train);
    emit_ms("train_ms", training);
    emit(
        "ms_per_call",
        training.as_secs_f64() * 1000.0 / f64::from(u32::try_from(cfg.train).unwrap_or(u32::MAX)),
    );
    let (live_apparent, live_alloc) = file_sizes(&path);
    emit("live_apparent_bytes", live_apparent);
    emit("live_alloc_bytes", live_alloc);
    finish();
    drop(store);
    remove_db(&path);
}

fn save<S: WriteStore>() {
    let cfg = config();
    let path = work_path("save");
    let mut store = train_store::<S>(&cfg, &path);

    let (live_apparent, live_alloc) = file_sizes(&path);
    let started = Instant::now();
    let saved = store.save().expect("save");
    let saving = started.elapsed();
    let (compacted_apparent, compacted_alloc) = file_sizes(&path);

    emit("saved", saved);
    emit("live_apparent_bytes", live_apparent);
    emit("live_alloc_bytes", live_alloc);
    emit("compacted_apparent_bytes", compacted_apparent);
    emit("compacted_alloc_bytes", compacted_alloc);
    emit_ms("save_ms", saving);
    finish();
    drop(store);
    remove_db(&path);
}

fn read<S: WriteStore>() {
    let cfg = config();
    let path = work_path("read");
    let mut store = train_store::<S>(&cfg, &path);

    // The first count_delta after training builds the count memo for the
    // current write generation and fills it from storage.
    let started = Instant::now();
    let first = query(&store, cfg.seed, 0);
    let first_query = started.elapsed();

    std::hint::black_box(first);

    // The rest are served from the memo for every key it has already seen;
    // only a key first asked for here still touches storage. A lone first
    // query has no cached remainder, so do not report a zero-denominator rate.
    let cached = if cfg.reads >= 2 {
        let started = Instant::now();
        let mut sum = 0_u64;
        for i in 1..cfg.reads as u64 {
            sum += query(&store, cfg.seed, i);
        }
        std::hint::black_box(sum);
        Some(started.elapsed())
    } else {
        None
    };

    // Predicted-candidate acceptances: committed writes that retire the
    // memo.  Each iteration first re-warms it with an untimed query, then
    // times exactly one invalidating write, so predicted_ms measures one
    // write against a warm memo instead of a burst of writes whose refill
    // cost lands outside the interval.
    let mut predicted = Duration::ZERO;
    let mut seeds = 0_u64;
    for i in 0..cfg.predicted as u64 {
        std::hint::black_box(query(&store, cfg.seed, cfg.reads as u64 + i));
        let h = row_hash(cfg.seed ^ 0xD1CE_D000, i);
        let last = TOKEN_BASE + u32::try_from(h % u64::from(PREV_DOMAIN)).unwrap();
        let cur = TOKEN_BASE + u32::try_from((h >> 16) % u64::from(CUR_DOMAIN)).unwrap();
        let started = Instant::now();
        seeds += store
            .observe_predicted(last, cur)
            .expect("observe_predicted");
        predicted += started.elapsed();
    }
    std::hint::black_box(seeds);

    emit("cached_queries", cfg.reads.saturating_sub(1));
    emit_ms("first_query_ms", first_query);
    if let Some(cached) = cached {
        emit_ms("cached_reads_ms", cached);
        emit(
            "us_per_cached_query",
            cached.as_secs_f64() * 1e6
                / f64::from(u32::try_from(cfg.reads - 1).unwrap_or(u32::MAX)),
        );
    }
    emit("predicted", cfg.predicted);
    emit_ms("predicted_ms", predicted);
    let (live_apparent, live_alloc) = file_sizes(&path);
    emit("live_apparent_bytes", live_apparent);
    emit("live_alloc_bytes", live_alloc);
    finish();
    drop(store);
    remove_db(&path);
}

// ── shared setup ──────────────────────────────────────────────────

fn open<S: WriteStore>(path: &Path) -> GenericUserStore<S> {
    remove_db(path);
    GenericUserStore::<S>::create_standalone(path).expect("create_standalone")
}

fn train_store<S: WriteStore>(cfg: &Config, path: &Path) -> GenericUserStore<S> {
    let mut store = open::<S>(path);
    for i in 0..cfg.train as u64 {
        let (last, cur) = training_pair(cfg.seed, i);
        store
            .observe_selection(last, cur)
            .expect("observe_selection");
    }
    store
}

fn training_pair(seed: u64, i: u64) -> (u32, u32) {
    let h = row_hash(seed, i);
    let last = TOKEN_BASE + u32::try_from(h % u64::from(PREV_DOMAIN)).unwrap();
    let cur = TOKEN_BASE + u32::try_from((h >> 16) % u64::from(CUR_DOMAIN)).unwrap();
    (last, cur)
}

const fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

const fn row_hash(seed: u64, i: u64) -> u64 {
    mix(seed ^ mix(i))
}

fn query<S: WriteStore>(store: &GenericUserStore<S>, seed: u64, i: u64) -> u64 {
    let h = row_hash(seed ^ 0xC0FF_EE00, i);
    let cur = TOKEN_BASE + u32::try_from((h >> 16) % u64::from(CUR_DOMAIN)).unwrap();
    let prev = if h.is_multiple_of(5) {
        None
    } else {
        Some(TOKEN_BASE + u32::try_from(h % u64::from(PREV_DOMAIN)).unwrap())
    };
    let delta = store.count_delta(prev, cur).expect("count_delta");
    delta.bigram_count + delta.unigram_delta
}

// ── config, metrics, paths ────────────────────────────────────────

struct Config {
    train: usize,
    reads: usize,
    predicted: usize,
    seed: u64,
}

fn config() -> Config {
    Config {
        train: env_usize("STORE_BENCH_TRAIN", 5_000),
        reads: env_usize("STORE_BENCH_READS", 50_000),
        predicted: env_usize("STORE_BENCH_PREDICTED", 200),
        seed: env_u64("STORE_BENCH_SEED", 0x057A_B1E5),
    }
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|&value| value > 0)
        .unwrap_or(default)
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn emit(key: &str, value: impl std::fmt::Display) {
    println!("{key}={value}");
}

fn emit_ms(key: &str, elapsed: Duration) {
    emit(key, format!("{:.1}", elapsed.as_secs_f64() * 1000.0));
}

fn finish() {
    match vm_hwm_kib() {
        Some(kib) => emit("vmhwm_kib", kib),
        None => emit("vmhwm_kib", "unavailable"),
    }
}

fn vm_hwm_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|kib| kib.parse().ok())
}

fn work_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "oxpinyin-store-bench-{tag}-{}.db",
        std::process::id()
    ))
}

fn remove_db(path: &Path) {
    let _ = std::fs::remove_file(path);
    let mut lock = path.as_os_str().to_os_string();
    lock.push("-lock");
    let _ = std::fs::remove_file(Path::new(&lock));
}

/// (apparent bytes, allocated bytes) of the data file at `path`.  On Unix,
/// the values are `st_size` and `st_blocks` × 512.  Elsewhere, allocation is
/// unavailable without platform-specific APIs and is reported as zero.
/// The LMDB `-lock` sidecar is not data and is not counted.
#[cfg(unix)]
fn file_sizes(path: &Path) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::metadata(path).map_or((0, 0), |meta| (meta.len(), meta.blocks() * 512))
}

#[cfg(not(unix))]
fn file_sizes(path: &Path) -> (u64, u64) {
    std::fs::metadata(path).map_or((0, 0), |meta| (meta.len(), 0))
}
