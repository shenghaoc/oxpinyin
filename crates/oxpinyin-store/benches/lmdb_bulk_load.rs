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

use criterion::measurement::WallTime;
use criterion::{BenchmarkGroup, Criterion, Throughput, criterion_group, criterion_main};
use heed::types::Bytes;
use heed::{Database, Env, EnvFlags, EnvOpenOptions, PutFlags, RwTxn};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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

/// Opens the environment the way the merged loader does: NO_SUB_DIR
/// (single file at `path`) plus WRITE_MAP and NO_SYNC.
#[allow(unsafe_code)]
fn open_env(path: &Path) -> Env {
    let mut opts = EnvOpenOptions::new();
    opts.max_dbs(1);
    opts.map_size(MAP_SIZE);
    let flags = EnvFlags::NO_SUB_DIR | EnvFlags::WRITE_MAP | EnvFlags::NO_SYNC;
    // SAFETY: we uphold LMDB's contract — a single process opens each
    // data file with a consistent map-size, and heed's RwTxn borrow
    // enforces the single-writer invariant within this process.
    // `flags()` is unsafe because certain flag combinations can violate
    // LMDB invariants (heed names NO_SYNC, NO_META_SYNC, NO_LOCK); our
    // chosen flags (NO_SUB_DIR plus WRITE_MAP) stay out of that class,
    // and this NO_SYNC user accepts reduced crash durability for a
    // throwaway file that is deleted, never reopened, when the bench
    // ends.
    unsafe {
        opts.flags(flags);
        opts.open(path)
    }
    .expect("open env")
}

/// One arm; `put` closes over the write flags. Each iteration clears the
/// table untimed, times the insert loop alone, then commits after the
/// clock has stopped (cheap: NO_SYNC).
fn bench_arm<F>(
    group: &mut BenchmarkGroup<'_, WallTime>,
    name: &str,
    env: &Env,
    db: &Database<Bytes, Bytes>,
    pairs: &[Pair],
    put: F,
) where
    F: Fn(&mut RwTxn<'_>, &[u8], &[u8]) -> heed::Result<()>,
{
    group.bench_function(name, |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                // Untimed: start every iteration from an empty table.
                let mut clear_txn = env.write_txn().expect("write_txn");
                db.clear(&mut clear_txn).expect("clear");
                clear_txn.commit().expect("commit");

                // Timed: the insert loop only.
                let mut write_txn = env.write_txn().expect("write_txn");
                let start = Instant::now();
                for (key, value) in pairs {
                    put(&mut write_txn, key.as_slice(), value.as_slice()).expect("put");
                }
                total += start.elapsed();
                write_txn.commit().expect("commit");
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
    let db: Database<Bytes, Bytes> = {
        let mut wtxn = env.write_txn().expect("write_txn");
        let db = env
            .create_database(&mut wtxn, Some(oxpinyin_store::RAW_TABLE))
            .expect("create_database");
        wtxn.commit().expect("commit");
        db
    };

    let mut group = c.benchmark_group("lmdb_bulk_load_272");
    group.throughput(Throughput::Elements(N as u64));
    bench_arm(
        &mut group,
        "sequential_put",
        &env,
        &db,
        &pairs,
        |txn, key, value| db.put(txn, key, value),
    );
    bench_arm(
        &mut group,
        "sequential_put_append",
        &env,
        &db,
        &pairs,
        |txn, key, value| db.put_with_flags(txn, PutFlags::APPEND, key, value),
    );
    group.finish();

    drop(env);
    let _ = std::fs::remove_dir_all(&dir);
}

criterion_group!(benches, bench_bulk_load);
criterion_main!(benches);
