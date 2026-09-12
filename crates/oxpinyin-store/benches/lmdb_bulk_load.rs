//! `LmdbStore::bulk_load_raw`'s insert loop at N = 272 pre-sorted pairs:
//! plain `put` against `put_with_flags(APPEND)` into an environment
//! opened the way the merged loader opens it (NO_SYNC + WRITE_MAP +
//! NO_SUB_DIR), so the delta between the two arms is the `MDB_APPEND`
//! fast path alone (#343).
//!
//! The environment and its table are created once outside the criterion
//! loop. Each iteration clears the table untimed, then times only the
//! insert loop via `iter_custom`; the commit runs after the clock stops
//! and is cheap under NO_SYNC. `force_sync` is never called: it is
//! identical for both arms and not the quantity of interest, and the
//! file is deleted when the bench ends.
//!
//! Synthetic data is deterministic: 272 `([u8; 8], [u8; 8])` pairs from
//! a fixed-seed splitmix64 stream, keys sorted ascending and big-endian
//! so byte order equals numeric order, values random. No model fixture.
//! Compile with `--no-default-features --features lmdb`.
//!
//! The bench drives the system LMDB C API directly, through the very
//! declarations the backend uses: `ffi` below is `src/lmdb/ffi.rs`
//! pulled in by path, so there is one generated binding surface in the
//! crate rather than a second copy maintained here. The library's build
//! script emits the `-llmdb` link directive for every target in this
//! package, benches included, so nothing further is needed to link.
// The gate mirrors this target's `required-features`: cargo already skips
// the bench when LMDB is not the selected backend, but rust-analyzer
// analyzes required-features targets regardless and would flag the ffi
// module's bindgen include as unresolved under every other backend's
// feature set. It sits above the `expect` so a false gate strips the
// expectation together with the code it governs — an empty crate with a
// live `#![expect(unsafe_code)]` would report the expectation unfulfilled.
#![cfg(feature = "lmdb")]
#![expect(
    unsafe_code,
    reason = "the bench calls liblmdb directly; every block carries a SAFETY comment"
)]

use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, Criterion, Throughput, criterion_group, criterion_main};
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[path = "../src/lmdb/ffi.rs"]
mod ffi;

/// Panics with LMDB's own message unless `rc` is `MDB_SUCCESS`. A bench
/// has no error path worth preserving: any failure here is a broken
/// measurement, not a condition to report.
fn ok(rc: std::ffi::c_int, what: &str) {
    assert!(rc == 0, "{what} failed: {} (LMDB {rc})", ffi::strerror(rc));
}

/// Pair count, the same 272 sorted entries as the S4 measurement in #341.
const N: usize = 272;
/// Fixed seed for the synthetic pairs ("oxpinyin" as ASCII bytes).
const SEED: u64 = 0x6F78_7069_6E79_696E;
/// Map ceiling only: 272 pairs of 16 bytes fill a handful of pages, and
/// the pages a `clear` frees are reused by the next iteration's writes.
const MAP_SIZE: usize = 64 << 20;

type Pair = ([u8; 8], [u8; 8]);

/// splitmix64 step: the seeded stream behind the synthetic pairs.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// N pairs with strictly ascending keys: random 64-bit keys from the
/// seeded stream, sorted, big-endian so byte order equals numeric order;
/// each value is the next 8 bytes of the same stream.
fn pairs() -> Vec<Pair> {
    let mut state = SEED;
    let mut pairs: Vec<Pair> = (0..N)
        .map(|_| {
            let key = splitmix64(&mut state).to_be_bytes();
            let value = splitmix64(&mut state).to_be_bytes();
            (key, value)
        })
        .collect();
    pairs.sort_unstable_by_key(|pair| pair.0);
    pairs.dedup_by(|a, b| a.0 == b.0);
    assert_eq!(pairs.len(), N, "seeded keys are distinct");
    pairs
}

/// A private directory under the system temp root, keyed by pid so
/// concurrent bench processes never share a file. Holds the data file
/// and LMDB's NO_SUB_DIR `-lock` sidecar; removed whole at the end.
fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "oxpinyin-bench-lmdb-bulk-load-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Opens the environment the way the merged loader does: MDB_NOSUBDIR
/// (single file at `path`) plus MDB_WRITEMAP and MDB_NOSYNC.
///
/// The caller closes the returned handle with `mdb_env_close` once the
/// measurement is done, before removing the temp directory.
fn open_env(path: &Path) -> *mut ffi::MDB_env {
    let c_path = CString::new(path.as_os_str().as_encoded_bytes()).expect("path without NUL");
    let mut env: *mut ffi::MDB_env = std::ptr::null_mut();
    // SAFETY: `env` is a live out-pointer that LMDB fills on success.
    ok(unsafe { ffi::mdb_env_create(&mut env) }, "mdb_env_create");
    // SAFETY: the environment is created and not yet open, which is when
    // both setters must be called.
    ok(
        unsafe { ffi::mdb_env_set_maxdbs(env, 1) },
        "mdb_env_set_maxdbs",
    );
    // SAFETY: as above; MAP_SIZE is a 64 MiB page multiple.
    ok(
        unsafe { ffi::mdb_env_set_mapsize(env, MAP_SIZE) },
        "mdb_env_set_mapsize",
    );
    // NO_SYNC costs crash durability, which a throwaway file that is
    // deleted and never reopened does not need; WRITE_MAP and NOSUBDIR
    // match what `LmdbStore::bulk_load_raw` opens with, which is the
    // whole point of the comparison.
    let flags = ffi::MDB_NOSUBDIR | ffi::MDB_NOTLS | ffi::MDB_WRITEMAP | ffi::MDB_NOSYNC;
    // SAFETY: the environment is configured and unopened, and `c_path`
    // outlives the call as a NUL-terminated path.
    ok(
        unsafe { ffi::mdb_env_open(env, c_path.as_ptr(), flags, 0o644) },
        "mdb_env_open",
    );
    env
}

/// Begins a write transaction on `env`.
fn write_txn(env: *mut ffi::MDB_env) -> *mut ffi::MDB_txn {
    let mut txn: *mut ffi::MDB_txn = std::ptr::null_mut();
    // SAFETY: the environment is open, no parent transaction is passed,
    // and `txn` is a live out-pointer.
    ok(
        unsafe { ffi::mdb_txn_begin(env, std::ptr::null_mut(), 0, &mut txn) },
        "mdb_txn_begin",
    );
    txn
}

/// One `mdb_put` with the arm's flags.
fn put(txn: *mut ffi::MDB_txn, dbi: ffi::MDB_dbi, key: &[u8], value: &[u8], flags: u32) {
    let mut k = ffi::val(key);
    let mut v = ffi::val(value);
    // SAFETY: the transaction is live and writable, `dbi` belongs to its
    // environment, and both vals borrow slices that outlive the call —
    // LMDB copies the record before returning.
    ok(
        unsafe { ffi::mdb_put(txn, dbi, &mut k, &mut v, flags) },
        "mdb_put",
    );
}

/// One arm; `put` closes over the write flags. Each iteration clears the
/// table untimed, times the insert loop alone, then commits after the
/// clock has stopped (cheap: NO_SYNC).
fn bench_arm(
    group: &mut BenchmarkGroup<'_, WallTime>,
    name: &str,
    env: *mut ffi::MDB_env,
    dbi: ffi::MDB_dbi,
    pairs: &[Pair],
    put_flags: u32,
) {
    group.bench_function(name, |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                // Untimed: start every iteration from an empty table.
                // `mdb_drop` with del=0 empties the table and keeps the
                // handle valid for the next iteration.
                let clear_txn = write_txn(env);
                // SAFETY: the transaction is live and writable and `dbi`
                // belongs to its environment; del=0 keeps the DBI valid.
                ok(unsafe { ffi::mdb_drop(clear_txn, dbi, 0) }, "mdb_drop");
                // SAFETY: live and owned; the handle is freed by the call.
                ok(unsafe { ffi::mdb_txn_commit(clear_txn) }, "mdb_txn_commit");

                // Timed: the insert loop only.
                let txn = write_txn(env);
                let start = Instant::now();
                for (key, value) in pairs {
                    put(txn, dbi, key.as_slice(), value.as_slice(), put_flags);
                }
                total += start.elapsed();
                // SAFETY: as above — the commit runs after the clock has
                // stopped and frees the transaction.
                ok(unsafe { ffi::mdb_txn_commit(txn) }, "mdb_txn_commit");
            }
            total
        });
    });
}

// S3 isolation (WRITE_MAP vs no-WRITE_MAP) is not attempted here:
// WRITE_MAP is always on in the merged code, so the no-WRITE_MAP arm
// would be counterfactual. See issue #343 for rationale.
fn bench_bulk_load(c: &mut Criterion) {
    let pairs = pairs();
    debug_assert!(
        pairs.windows(2).all(|w| w[0].0 < w[1].0),
        "pairs are strictly ascending by key"
    );

    let dir = temp_dir();
    let env = open_env(&dir.join("lmdb_bulk_load.mdb"));
    let dbi = {
        let txn = write_txn(env);
        let name = CString::new(oxpinyin_store::RAW_TABLE).expect("table name without NUL");
        let mut dbi: ffi::MDB_dbi = 0;
        // SAFETY: the transaction is live, `name` outlives the call as a
        // NUL-terminated string, and `dbi` is a live out-param. No other
        // transaction in this process calls `mdb_dbi_open`, which is the
        // exclusion LMDB requires of it.
        ok(
            unsafe { ffi::mdb_dbi_open(txn, name.as_ptr(), ffi::MDB_CREATE, &mut dbi) },
            "mdb_dbi_open",
        );
        // SAFETY: live and owned; committing is what makes the DBI valid
        // env-wide for the transactions the arms open below.
        ok(unsafe { ffi::mdb_txn_commit(txn) }, "mdb_txn_commit");
        dbi
    };

    let mut group = c.benchmark_group("lmdb_bulk_load_272");
    group.throughput(Throughput::Elements(N as u64));
    bench_arm(&mut group, "sequential_put", env, dbi, &pairs, 0);
    bench_arm(
        &mut group,
        "sequential_put_append",
        env,
        dbi,
        &pairs,
        ffi::MDB_APPEND,
    );
    group.finish();

    // SAFETY: every transaction opened above was committed, so no handle
    // outlives this close, and the environment is not named again.
    unsafe { ffi::mdb_env_close(env) };
    let _ = std::fs::remove_dir_all(&dir);
}

criterion_group!(benches, bench_bulk_load);
criterion_main!(benches);
