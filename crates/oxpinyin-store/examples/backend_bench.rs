//! redb vs LMDB on the raw [`OrderedStore`] trait — identical workloads.
//!
//! Measurement only; consumes public APIs and touches no parity code.
//! Run in release, with the LMDB half enabled:
//!
//! ```text
//! cargo run -p oxpinyin-store --release --example backend_bench --features lmdb
//! ```
//!
//! Without `--features lmdb` the example runs redb only and prints a note.
//! Each (backend, scenario) pair runs in a child process (a re-exec of this
//! binary), so `/proc/self/status` VmHWM measures that pair alone rather
//! than a process-monotonic high-water mark; the child baseline (runtime
//! plus binary) is common to both backends, so the redb-vs-lmdb delta is
//! the signal.  Workload rows derive deterministically from the seed, and
//! every scenario body is one generic function over `S: OrderedStore` —
//! both backends see identical keys, values, and operation order.  Before
//! printing each block the parent verifies that both children reported the
//! same metric set and agrees on every non-measurement metric (counts and
//! data checksums); a mismatch aborts instead of printing a comparison.
//!
//! Sizes via env vars (defaults in parentheses): `BACKEND_BENCH_N`
//! (100_000 bigram rows; pron = N/2, phrase = N/4), `BACKEND_BENCH_GETS`
//! (50_000), `BACKEND_BENCH_PREFIXES` (128), `BACKEND_BENCH_READONLY`
//! (400_000), `BACKEND_BENCH_SEED`.  The LMDB env map size is fixed at
//! 1 GiB by the backend, so very large overrides can exhaust it.
//!
//! Compaction is backend-dependent and best-effort: redb rewrites and
//! reclaims pages, while LMDB reuses freed pages in place and does not shrink.
//! The save_compact scenario reports timings and sizes for both backends.
//!
//! On Unix, on-disk sizes report both apparent length (st_size) and
//! allocated blocks (st_blocks × 512) of the data file only; allocated is
//! the comparison figure, and apparent >> allocated reveals a sparse file.
//! On other platforms, allocated bytes are reported as zero (unavailable).

use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

#[cfg(feature = "lmdb")]
use oxpinyin_store::LmdbStore;
use oxpinyin_store::{OrderedStore, RedbStore};

const SCENARIOS: [&str; 5] = [
    "bulk_load",
    "point_get",
    "prefix_scan",
    "save_compact",
    "readonly_scan",
];

// Table names and key shapes mirror the user store's tables: bigram keys
// are (prev, cur) big-endian token pairs, pron keys are a token plus
// variable-length u16 syllable keys, phrase keys are bare tokens.
const BIGRAM: &str = "user_bigram";
const PRON: &str = "user_pronunciation";
const PHRASE: &str = "user_phrase";
const SYSTEM: &str = "system_bigram";

const TOKEN_BASE: u64 = 0x0100_0000;
const PREV_DOMAIN: u64 = 2048;
const CUR_DOMAIN: u64 = 60_000;
const PRON_TOKEN_DOMAIN: u64 = 30_000;
const PHRASE_TOKEN_DOMAIN: u64 = 50_000;

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

// ── parent: spawn children, print the comparison table ────────────

fn parent() {
    let cfg = config();
    println!("backend_bench — redb vs lmdb on the OrderedStore trait");
    println!(
        "n={} (pron {}/2, phrase {}/4)  gets={}  prefixes={}  readonly={}  seed={:#x}",
        cfg.n, cfg.n, cfg.n, cfg.gets, cfg.prefixes, cfg.readonly, cfg.seed
    );
    println!("one fresh process per (backend, scenario); VmHWM per pair");
    if cfg!(feature = "lmdb") {
        println!("backends: redb, lmdb");
    } else {
        println!("backends: redb only — rebuild with --features lmdb for the comparison");
    }
    println!();

    for scenario in SCENARIOS {
        let redb = spawn_child("redb", scenario);
        let lmdb = if cfg!(feature = "lmdb") {
            Some(spawn_child("lmdb", scenario))
        } else {
            None
        };
        verify_parity(scenario, &redb, lmdb.as_deref());
        print_block(scenario, &redb, lmdb.as_deref());
    }
}

// Metrics allowed to differ across backends: timings, memory, and on-disk
// sizes.  Everything else — workload counts and data checksums — must agree
// exactly, since both children run the same deterministic workload.
fn is_measurement(key: &str) -> bool {
    key.ends_with("_ms")
        || key.starts_with("us_per_")
        || key.ends_with("_bytes")
        || key == "size_live"
        || key == "vmhwm_kib"
}

fn lookup<'a>(rows: &'a [(String, String)], key: &str) -> Option<&'a str> {
    rows.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn verify_parity(scenario: &str, redb: &[(String, String)], lmdb: Option<&[(String, String)]>) {
    let Some(lmdb) = lmdb else { return };
    for (key, redb_value) in redb {
        match lookup(lmdb, key) {
            None => {
                eprintln!("{scenario}: lmdb missing metric {key:?}");
                std::process::exit(1);
            }
            Some(lmdb_value) if !is_measurement(key) && lmdb_value != redb_value => {
                eprintln!("{scenario}: {key} disagrees — redb {redb_value}, lmdb {lmdb_value}");
                std::process::exit(1);
            }
            Some(_) => {}
        }
    }
    for (key, _) in lmdb {
        if lookup(redb, key).is_none() {
            eprintln!("{scenario}: unexpected lmdb metric {key:?}");
            std::process::exit(1);
        }
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

fn print_block(scenario: &str, redb: &[(String, String)], lmdb: Option<&[(String, String)]>) {
    println!("── {scenario} ──");
    println!("  {:<26} {:>14} {:>14}", "metric", "redb", "lmdb");
    for (key, value) in redb {
        let lmdb_value = lmdb
            .and_then(|rows| rows.iter().find(|(k, _)| k == key))
            .map(|(_, v)| v.as_str())
            .unwrap_or("-");
        println!("  {:<26} {:>14} {:>14}", key, value, lmdb_value);
    }
    println!();
}

// ── child: one (backend, scenario) pair in this process ───────────

fn run_child(backend: &str, scenario: &str) {
    match backend {
        "redb" => dispatch::<RedbStore>(scenario),
        "lmdb" => dispatch_lmdb(scenario),
        other => {
            eprintln!("unknown backend {other:?}");
            std::process::exit(2);
        }
    }
}

#[cfg(feature = "lmdb")]
fn dispatch_lmdb(scenario: &str) {
    dispatch::<LmdbStore>(scenario);
}

#[cfg(not(feature = "lmdb"))]
fn dispatch_lmdb(_scenario: &str) {
    eprintln!("this build lacks the lmdb feature");
    std::process::exit(2);
}

fn dispatch<S: OrderedStore>(scenario: &str) {
    match scenario {
        "bulk_load" => bulk_load::<S>(),
        "point_get" => point_get::<S>(),
        "prefix_scan" => prefix_scan::<S>(),
        "save_compact" => save_compact::<S>(),
        "readonly_scan" => readonly_scan::<S>(),
        other => {
            eprintln!("unknown scenario {other:?}");
            std::process::exit(2);
        }
    }
}

fn bulk_load<S: OrderedStore>() {
    let cfg = config();
    let path = work_path("bulk-load");
    let started = Instant::now();
    let store = load_all::<S>(&cfg, &path);
    let load = started.elapsed();

    emit_ms("load_ms", load);
    emit("rows_bigram", count_rows(&store, BIGRAM));
    emit("rows_pron", count_rows(&store, PRON));
    emit("rows_phrase", count_rows(&store, PHRASE));
    let (live_apparent, live_alloc) = file_sizes(&path);
    emit("live_apparent_bytes", live_apparent);
    emit("live_alloc_bytes", live_alloc);
    finish();
    drop(store);
    remove_db(&path);
}

fn point_get<S: OrderedStore>() {
    let cfg = config();
    let path = work_path("point-get");
    let store = load_all::<S>(&cfg, &path);

    let started = Instant::now();
    let mut hits = 0_u64;
    let mut bytes = 0_u64;
    let mut value_sum = 0_u64;
    for i in 0..cfg.gets as u64 {
        let idx = row_hash(cfg.seed ^ 0xDEAD_BEEF, i) % cfg.n as u64;
        let (key, _) = bigram_row(cfg.seed, idx);
        if let Some(value) = store.get(BIGRAM, &key).expect("get") {
            hits += 1;
            bytes += value.len() as u64;
            value_sum = fold_bytes(value_sum, &value);
        }
    }
    let gets = started.elapsed();
    std::hint::black_box((hits, bytes));

    emit("gets", cfg.gets);
    emit("hits", hits);
    emit("value_sum", value_sum);
    emit_ms("gets_ms", gets);
    emit("us_per_get", gets.as_secs_f64() * 1e6 / cfg.gets as f64);
    let (live_apparent, live_alloc) = file_sizes(&path);
    emit("live_apparent_bytes", live_apparent);
    emit("live_alloc_bytes", live_alloc);
    finish();
    drop(store);
    remove_db(&path);
}

fn prefix_scan<S: OrderedStore>() {
    let cfg = config();
    let path = work_path("prefix-scan");
    let store = load_all::<S>(&cfg, &path);

    let started = Instant::now();
    let mut rows = 0_u64;
    let mut bigram_sum = 0_u64;
    let mut pron_sum = 0_u64;
    for i in 0..cfg.prefixes as u64 {
        let h = row_hash(cfg.seed ^ 0xFEED_FACE, i);

        // bigram (prev, *): the successor-set lookup
        let prev = TOKEN_BASE + h % PREV_DOMAIN;
        let lo = (prev as u32).to_be_bytes();
        let hi = (prev as u32 + 1).to_be_bytes();
        store
            .range(
                BIGRAM,
                Bound::Included(&lo[..]),
                Bound::Excluded(&hi[..]),
                &mut |key, value| {
                    rows += 1;
                    bigram_sum = fold_bytes(fold_bytes(bigram_sum, key), value);
                    Ok(())
                },
            )
            .expect("bigram range");

        // pron (token, *): the pronunciation-set lookup
        let token = TOKEN_BASE + (h >> 16) % PRON_TOKEN_DOMAIN;
        let lo = (token as u32).to_be_bytes();
        let hi = (token as u32 + 1).to_be_bytes();
        store
            .range(
                PRON,
                Bound::Included(&lo[..]),
                Bound::Excluded(&hi[..]),
                &mut |key, value| {
                    rows += 1;
                    pron_sum = fold_bytes(fold_bytes(pron_sum, key), value);
                    Ok(())
                },
            )
            .expect("pron range");
    }
    let scan = started.elapsed();
    std::hint::black_box(rows);

    emit("prefixes", cfg.prefixes);
    emit("rows_hit", rows);
    emit("bigram_sum", bigram_sum);
    emit("pron_sum", pron_sum);
    emit_ms("scan_ms", scan);
    let (live_apparent, live_alloc) = file_sizes(&path);
    emit("live_apparent_bytes", live_apparent);
    emit("live_alloc_bytes", live_alloc);
    finish();
    drop(store);
    remove_db(&path);
}

fn save_compact<S: OrderedStore>() {
    let cfg = config();
    let path = work_path("save-compact");
    let mut store = load_all::<S>(&cfg, &path);

    // Churn rows before the backend-dependent best-effort compact step:
    // overwrite a slice of bigram rows and remove a slice of pron rows.
    store
        .write(|txn| {
            let replacement = 997_u64.to_be_bytes();
            for i in 0..(cfg.n / 10) as u64 {
                let (key, _) = bigram_row(cfg.seed, i);
                txn.put(BIGRAM, &key, &replacement)?;
            }
            for i in 0..(cfg.n / 20) as u64 {
                let (key, _) = pron_row(cfg.seed, i);
                txn.remove(PRON, &key)?;
            }
            Ok(())
        })
        .expect("churn");

    let (live_apparent, live_alloc) = file_sizes(&path);
    let started = Instant::now();
    store.compact().expect("compact");
    let compact = started.elapsed();
    let (compacted_apparent, compacted_alloc) = file_sizes(&path);

    // Post-timing verification: the overwritten bigram slice must read
    // back with the replacement value and the removed pron slice must be
    // gone; the survivor checksum is compared across backends.
    let mut churn_sum = 0_u64;
    for i in 0..(cfg.n / 10) as u64 {
        let (key, _) = bigram_row(cfg.seed, i);
        match store.get(BIGRAM, &key).expect("get after compact") {
            Some(value) if value == 997_u64.to_be_bytes() => {
                churn_sum = fold_bytes(churn_sum, &value);
            }
            _ => {
                eprintln!("bigram row {i} wrong or missing after compact");
                std::process::exit(1);
            }
        }
    }
    for i in 0..(cfg.n / 20) as u64 {
        let (key, _) = pron_row(cfg.seed, i);
        if store.get(PRON, &key).expect("get after compact").is_some() {
            eprintln!("pron row {i} still present after removal + compact");
            std::process::exit(1);
        }
    }

    emit("live_apparent_bytes", live_apparent);
    emit("live_alloc_bytes", live_alloc);
    emit("compacted_apparent_bytes", compacted_apparent);
    emit("compacted_alloc_bytes", compacted_alloc);
    emit_ms("compact_ms", compact);
    emit("churn_sum", churn_sum);
    finish();
    drop(store);
    remove_db(&path);
}

fn readonly_scan<S: OrderedStore>() {
    let cfg = config();
    let path = work_path("readonly");

    let store = fresh::<S>(&path);
    let started = Instant::now();
    store
        .write(|txn| {
            for i in 0..cfg.readonly as u64 {
                let (key, value) = readonly_row(cfg.seed, i);
                txn.put(SYSTEM, &key, &value)?;
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
    let mut rows = 0_u64;
    let mut key_bytes = 0_u64;
    let mut value_sum = 0_u64;
    ro.for_each(SYSTEM, &mut |key, value| {
        rows += 1;
        key_bytes += key.len() as u64;
        value_sum = fold_bytes(value_sum, value);
        Ok(())
    })
    .expect("scan system table");
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

// ── shared setup ──────────────────────────────────────────────────

fn load_all<S: OrderedStore>(cfg: &Config, path: &Path) -> S {
    let store = fresh::<S>(path);
    store
        .write(|txn| {
            for i in 0..cfg.n as u64 {
                let (key, value) = bigram_row(cfg.seed, i);
                txn.put(BIGRAM, &key, &value)?;
            }
            for i in 0..(cfg.n / 2) as u64 {
                let (key, value) = pron_row(cfg.seed, i);
                txn.put(PRON, &key, &value)?;
            }
            for i in 0..(cfg.n / 4) as u64 {
                let (key, text) = phrase_row(cfg.seed, i);
                txn.put(PHRASE, &key, text.as_bytes())?;
            }
            Ok(())
        })
        .expect("bulk load");
    store
}

fn fresh<S: OrderedStore>(path: &Path) -> S {
    remove_db(path);
    S::create(path).expect("create store")
}

fn count_rows<S: OrderedStore>(store: &S, table: &str) -> u64 {
    let mut rows = 0_u64;
    store
        .for_each(table, &mut |_key, _value| {
            rows += 1;
            Ok(())
        })
        .expect("count rows");
    rows
}

// ── deterministic workload rows ───────────────────────────────────

fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn row_hash(seed: u64, i: u64) -> u64 {
    mix(seed ^ mix(i))
}

// FNV-1a-style rolling checksum over returned bytes.
fn fold_bytes(mut sum: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        sum = sum.wrapping_mul(0x100_0000_01B3).wrapping_add(u64::from(b));
    }
    sum
}

fn bigram_row(seed: u64, i: u64) -> ([u8; 8], [u8; 8]) {
    let h = row_hash(seed, i);
    let prev = TOKEN_BASE + h % PREV_DOMAIN;
    let cur = TOKEN_BASE + (h / PREV_DOMAIN) % CUR_DOMAIN;
    let mut key = [0_u8; 8];
    key[..4].copy_from_slice(&(prev as u32).to_be_bytes());
    key[4..].copy_from_slice(&(cur as u32).to_be_bytes());
    (key, (1 + h % 97).to_be_bytes())
}

fn pron_row(seed: u64, i: u64) -> (Vec<u8>, [u8; 8]) {
    let h = row_hash(seed, i ^ 0xA5A5_A5A5);
    let token = TOKEN_BASE + h % PRON_TOKEN_DOMAIN;
    let syllable_count = 1 + h % 3;
    let mut key = Vec::with_capacity(4 + 2 * syllable_count as usize);
    key.extend_from_slice(&(token as u32).to_be_bytes());
    for k in 0..syllable_count {
        let syllable = ((h >> (16 * k + 8)) & 0xFFFF) as u16;
        key.extend_from_slice(&syllable.to_be_bytes());
    }
    (key, (1 + (h >> 3) % 997).to_be_bytes())
}

fn phrase_row(seed: u64, i: u64) -> ([u8; 4], String) {
    let h = row_hash(seed, i ^ 0x5A5A_5A5A);
    let token = TOKEN_BASE + i % PHRASE_TOKEN_DOMAIN;
    let char_count = 2 + h % 3;
    let mut text = String::with_capacity(3 * char_count as usize);
    for k in 0..char_count {
        let code = 0x4E00 + ((h >> (9 * k + 4)) & 0x1FF);
        text.push(char::from_u32(code as u32).unwrap_or('词'));
    }
    ((token as u32).to_be_bytes(), text)
}

fn readonly_row(seed: u64, i: u64) -> ([u8; 8], [u8; 16]) {
    let h = row_hash(seed, i);
    let prev = TOKEN_BASE + h % 20_000;
    let cur = TOKEN_BASE + (h / 20_000) % 100_000;
    let mut key = [0_u8; 8];
    key[..4].copy_from_slice(&(prev as u32).to_be_bytes());
    key[4..].copy_from_slice(&(cur as u32).to_be_bytes());
    let mut value = [0_u8; 16];
    for (chunk, part) in value.chunks_exact_mut(4).zip([
        h as u32,
        (h >> 16) as u32,
        (h >> 32) as u32,
        (h >> 48) as u32,
    ]) {
        chunk.copy_from_slice(&part.to_be_bytes());
    }
    (key, value)
}

// ── config, metrics, paths ────────────────────────────────────────

struct Config {
    n: usize,
    gets: usize,
    prefixes: usize,
    readonly: usize,
    seed: u64,
}

fn config() -> Config {
    let cfg = Config {
        n: env_usize("BACKEND_BENCH_N", 100_000),
        gets: env_usize("BACKEND_BENCH_GETS", 50_000),
        prefixes: env_usize("BACKEND_BENCH_PREFIXES", 128),
        readonly: env_usize("BACKEND_BENCH_READONLY", 400_000),
        seed: env_u64("BACKEND_BENCH_SEED", 0x0BB5_EED1),
    };
    // phrase_row keys on i % PHRASE_TOKEN_DOMAIN, so a phrase workload
    // larger than the token domain would wrap and overwrite its own rows.
    if cfg.n / 4 > PHRASE_TOKEN_DOMAIN as usize {
        eprintln!(
            "BACKEND_BENCH_N too large: n/4 = {} exceeds the phrase \
             token domain {PHRASE_TOKEN_DOMAIN}; phrase keys would collide",
            cfg.n / 4
        );
        std::process::exit(2);
    }
    cfg
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

/// (apparent bytes, allocated bytes) of the data file at `path`: st_size
/// and st_blocks × 512.  Allocated is the fair on-disk figure; apparent is
/// kept alongside so a sparse file (apparent >> allocated) is visible.
/// The LMDB `-lock` sidecar is not data and is not counted.
fn file_sizes(path: &Path) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt as _;
    match std::fs::metadata(path) {
        Ok(meta) => (meta.len(), meta.blocks() * 512),
        Err(_) => (0, 0),
    }
}
