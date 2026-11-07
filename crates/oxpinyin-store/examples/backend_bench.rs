//! The compiled-in store backend on the raw store tier traits.
//!
//! Measurement only; consumes public APIs and touches no parity code.
//! Run in release against the default (tkrzw):
//!
//! ```text
//! cargo run -p oxpinyin-store --release --example backend_bench
//! ```
//!
//! A peer build swaps the compiled-in backend, not a column of this run
//! (the store's exactly-one-backend guard refuses two peers in one
//! binary, so the old two-column comparison mode of this example was
//! dead long before the backend it contrasted was removed —
//! cross-backend comparison happens across CI builds, not in one
//! process):
//!
//! ```text
//! cargo run -p oxpinyin-store --release --example backend_bench \
//!   --no-default-features --features kyotocabinet
//! ```
//!
//! Each scenario runs in a child process (a re-exec of this binary), so
//! `/proc/self/status` `VmHWM` measures that scenario alone rather than a
//! process-monotonic high-water mark.  Workload rows derive
//! deterministically from the seed, and every scenario body is one
//! generic function over its tier — identical keys, values, and
//! operation order on every backend, which is what makes cross-build
//! comparisons of the printed figures meaningful.
//!
//! Sizes via env vars (defaults in parentheses): `BACKEND_BENCH_READONLY`
//! (`400_000`), `BACKEND_BENCH_SEED`.
//!
//! On Unix, on-disk sizes report both apparent length (`st_size`) and
//! allocated blocks (`st_blocks` × 512) of the data file only; allocated is
//! the comparison figure, and apparent >> allocated reveals a sparse file.
//! On other platforms, allocated bytes are reported as zero (unavailable).

use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

#[cfg(feature = "bdb")]
use oxpinyin_store::BdbStore;
#[cfg(feature = "kyotocabinet")]
use oxpinyin_store::KcStore;
#[cfg(feature = "tkrzw")]
use oxpinyin_store::TkrzwStore;
use oxpinyin_store::{RawReadStore, WriteStore, WriteTxn};

const SCENARIOS: [&str; 1] = ["readonly_scan"];

const TOKEN_BASE: u64 = 0x0100_0000;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |n: usize| args.get(n).map(String::as_str);
    match (arg(1), arg(2), arg(3)) {
        (Some("--child"), Some(backend), Some(scenario)) => {
            run_child(backend, scenario);
        }
        _ => parent(),
    }
}

// ── parent: spawn one child per scenario, print the metric table ────

/// The compiled-in backend's label, from the feature the store's
/// exactly-one-backend guard leaves enabled.
fn compiled_backend() -> &'static str {
    if cfg!(feature = "kyotocabinet") {
        "kyotocabinet"
    } else if cfg!(feature = "tkrzw") {
        "tkrzw"
    } else if cfg!(feature = "bdb") {
        "bdb"
    } else {
        "the compiled-in backend"
    }
}

fn parent() {
    let cfg = config();
    println!(
        "backend_bench — {} on the store tier traits",
        compiled_backend()
    );
    println!("readonly={}  seed={:#x}", cfg.readonly, cfg.seed);
    println!("one fresh process per scenario; VmHWM per scenario");
    println!("compare across builds: the workload is a pure function of the seed");
    println!();

    for scenario in SCENARIOS {
        let rows = spawn_child(compiled_backend(), scenario);
        print_block(scenario, compiled_backend(), &rows);
    }
}

fn spawn_child(backend: &str, scenario: &str) -> Vec<(String, String)> {
    let exe = std::env::current_exe().expect("current exe");
    let output = Command::new(exe)
        .args(["--child", backend, scenario])
        .output()
        .expect("spawn bench child");
    if !output.status.success() {
        eprintln!(
            "child {backend}/{scenario} failed:\n{}",
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

fn print_block(scenario: &str, backend: &str, rows: &[(String, String)]) {
    println!("── {scenario} ──");
    println!("  {:<26} {:>14}", "metric", backend);
    for (key, value) in rows {
        println!("  {key:<26} {value:>14}");
    }
    println!();
}

// ── child: one (backend, scenario) pair in this process ───────────

fn run_child(backend: &str, scenario: &str) {
    match backend {
        #[cfg(feature = "kyotocabinet")]
        "kc" | "kyotocabinet" => dispatch::<KcStore>(scenario),
        #[cfg(feature = "tkrzw")]
        "tkrzw" => dispatch::<TkrzwStore>(scenario),
        #[cfg(feature = "bdb")]
        "bdb" | "berkeleydb" => dispatch::<BdbStore>(scenario),
        other => {
            // Normalize the `kc` alias to the actual cargo feature name.
            // "kc" is accepted as an input alias on the KC arm, but the
            // feature that compiles KC in is `kyotocabinet`, so a
            // suggestion of `--features kc` would be a dead-end.
            let feat = if other == "kc" { "kyotocabinet" } else { other };
            eprintln!(
                "backend {other:?} is not compiled into this build; \
                 rebuild with --no-default-features --features {feat}",
            );
            std::process::exit(2);
        }
    }
}

fn dispatch<S: WriteStore>(scenario: &str) {
    match scenario {
        "readonly_scan" => readonly_scan::<S>(),
        other => {
            eprintln!("unknown scenario {other:?}");
            std::process::exit(2);
        }
    }
}

fn readonly_scan<S: WriteStore>() {
    let cfg = config();
    let path = work_path("readonly");

    let store = fresh::<S>(&path);
    let started = Instant::now();
    store
        .write(|txn| {
            for i in 0..cfg.readonly as u64 {
                let (key, value) = readonly_row(cfg.seed, i);
                txn.put_raw(&key, &value)?;
            }
            Ok(())
        })
        .expect("load system table");
    let load = started.elapsed();
    drop(store);

    // Proxy for system-table load: reopen read-only, then one full scan.
    let started = Instant::now();
    let ro = S::open_read_only(&path).expect("open read-only");
    let open = started.elapsed();

    let started = Instant::now();
    let (rows, key_bytes, value_sum) = measure_full_scan(&ro);
    let scan = started.elapsed();
    std::hint::black_box((rows, key_bytes));

    emit_ms("load_ms", load);
    emit("rows", rows);
    emit("value_sum", value_sum);
    emit_ms("open_ro_ms", open);
    emit_ms("scan_ms", scan);
    let (live_apparent, live_alloc) = file_sizes(&path);
    emit("live_apparent_bytes", live_apparent);
    emit("live_alloc_bytes", live_alloc);
    finish();
    drop(ro);
    remove_db(&path);
}

// ── measured read work (read tier) ────────────────────────────────
//
// The fixture is laid down through `WriteStore`, but the timed read below
// needs nothing beyond `RawReadStore` — so it is bound to that tier and
// would run unchanged against a read-only backend handed the file.

/// Returns `(rows, key bytes, value checksum)` for one full walk.
fn measure_full_scan<S: RawReadStore>(store: &S) -> (u64, u64, u64) {
    let mut rows = 0_u64;
    let mut key_bytes = 0_u64;
    let mut value_sum = 0_u64;
    store
        .range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
            rows += 1;
            key_bytes += key.len() as u64;
            value_sum = fold_bytes(value_sum, value);
            Ok(())
        })
        .expect("scan system table");
    (rows, key_bytes, value_sum)
}

// ── shared setup ──────────────────────────────────────────────────

fn fresh<S: WriteStore>(path: &Path) -> S {
    remove_db(path);
    S::create(path).expect("create store")
}

// ── deterministic workload rows ───────────────────────────────────

const fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

const fn row_hash(seed: u64, i: u64) -> u64 {
    mix(seed ^ mix(i))
}

// FNV-1a-style rolling checksum over returned bytes.
fn fold_bytes(mut sum: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        sum = sum.wrapping_mul(0x100_0000_01B3).wrapping_add(u64::from(b));
    }
    sum
}

fn readonly_row(seed: u64, i: u64) -> ([u8; 8], [u8; 16]) {
    let h = row_hash(seed, i);
    let prev = TOKEN_BASE + h % 20_000;
    let cur = TOKEN_BASE + (h / 20_000) % 100_000;
    let mut key = [0_u8; 8];
    key[..4].copy_from_slice(&u32::try_from(prev).unwrap().to_be_bytes());
    key[4..].copy_from_slice(&u32::try_from(cur).unwrap().to_be_bytes());
    let mut value = [0_u8; 16];
    for (chunk, part) in value.as_chunks_mut::<4>().0.iter_mut().zip([
        u32::try_from(h & 0xFFFF_FFFF).unwrap(),
        u32::try_from((h >> 16) & 0xFFFF_FFFF).unwrap(),
        u32::try_from((h >> 32) & 0xFFFF_FFFF).unwrap(),
        u32::try_from((h >> 48) & 0xFFFF_FFFF).unwrap(),
    ]) {
        *chunk = part.to_be_bytes();
    }
    (key, value)
}

// ── config, metrics, paths ────────────────────────────────────────

struct Config {
    readonly: usize,
    seed: u64,
}

fn config() -> Config {
    Config {
        readonly: env_usize("BACKEND_BENCH_READONLY", 400_000),
        seed: env_u64("BACKEND_BENCH_SEED", 0x0BB5_EED1),
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
        "oxpinyin-backend-bench-{tag}-{}.db",
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
/// The `-lock` sidecar is not data and is not counted.
#[cfg(unix)]
fn file_sizes(path: &Path) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::metadata(path).map_or((0, 0), |meta| (meta.len(), meta.blocks() * 512))
}

#[cfg(not(unix))]
fn file_sizes(path: &Path) -> (u64, u64) {
    std::fs::metadata(path).map_or((0, 0), |meta| (meta.len(), 0))
}
