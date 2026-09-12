//! redb write-transaction `is_empty` probe cost, isolated from commit
//! fsync (#342).
//!
//! The store answers an emptiness check inside a write transaction, so a
//! plain `b.iter` through `WriteStore::write` measures the probe and
//! redb's commit fsync together — the fsync dominates by orders of
//! magnitude. This bench opens a `redb::Database` directly and times only
//! the probe: each iteration begins a write transaction untimed, runs the
//! arm's probe between two `Instant`s, then drops the transaction to
//! abort it. `WriteTransaction`'s `Drop` rolls back without touching
//! storage, so no commit, fsync, or disk I/O sits anywhere near the timed
//! region.
//!
//! Two arms over the same probe table:
//! - `list_tables_walk` — the pre-F4 probe (introduced by 3dd7d39b,
//!   removed by 9a75191c, no longer in the codebase): `list_tables()`
//!   plus a name walk before opening, so an emptiness check never created
//!   an absent table.
//! - `open_table_header` — the post-F4 probe (`RedbWriteTxn::is_empty` in
//!   src/lib.rs): open the definition directly; a fresh table has no
//!   root, so the answer comes straight from the header.
//!
//! Dimension: 1 or 9 pre-created tables. The probe table is always one of
//! them; the 8 distinctly named fillers give the walk real entries to
//! traverse at 9. Every table uses the production shape
//! `TableDefinition<&'static [u8], &'static [u8]>` (`table_def` in
//! src/lib.rs) — redb validates declared key/value types on open, so a
//! differently typed definition would not exercise the production path.

// The gate mirrors this target's `required-features`: cargo already skips
// the bench when redb is not the selected backend, but rust-analyzer
// analyzes required-features targets regardless and would flag the `redb`
// imports as unresolved under every other backend's feature set.
#![cfg(feature = "redb")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use redb::{Database, ReadableTableMetadata, TableDefinition, TableHandle};

type Def = TableDefinition<'static, &'static [u8], &'static [u8]>;

/// One arm's probe, run inside an open write transaction.
type Probe = fn(&redb::WriteTransaction) -> bool;

/// The table both arms probe.
const PROBE_TABLE: Def = TableDefinition::new("probe");

/// Distinctly named fillers, opened up to `n_tables - 1` at a time so the
/// walk has entries to traverse past at the 9-table end of the dimension.
const FILLER_TABLES: [Def; 8] = [
    TableDefinition::new("filler-0"),
    TableDefinition::new("filler-1"),
    TableDefinition::new("filler-2"),
    TableDefinition::new("filler-3"),
    TableDefinition::new("filler-4"),
    TableDefinition::new("filler-5"),
    TableDefinition::new("filler-6"),
    TableDefinition::new("filler-7"),
];

/// The pre-F4 probe, reimplemented here because 9a75191c removed it from
/// the store: probe existence via `list_tables` first, so the check never
/// created an absent table.
fn probe_list_tables_walk(txn: &redb::WriteTransaction) -> bool {
    let exists = txn
        .list_tables()
        .expect("list_tables")
        .any(|handle| handle.name() == PROBE_TABLE.name());
    if !exists {
        return true; // an absent table counts as empty
    }
    let tbl = txn.open_table(PROBE_TABLE).expect("open probe table");
    tbl.is_empty().expect("probe is_empty")
}

/// The post-F4 probe, mirroring `RedbWriteTxn::is_empty` in src/lib.rs:
/// open the definition directly and read the header.
fn probe_open_table_header(txn: &redb::WriteTransaction) -> bool {
    let tbl = txn.open_table(PROBE_TABLE).expect("open probe table");
    tbl.is_empty().expect("probe is_empty")
}

/// A throwaway database in a unique tempdir, removed on drop.
struct BenchDb {
    db: Option<Database>,
    dir: PathBuf,
}

impl BenchDb {
    /// Opens a database with `n_tables` pre-created tables — the probe
    /// table plus fillers — left exactly as a committed store would.
    fn new(n_tables: usize) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "oxpinyin-bench-redb-is-empty-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&dir).expect("create bench tempdir");
        let db = Database::create(dir.join("bench.redb")).expect("create redb database");
        let txn = db.begin_write().expect("begin setup write txn");
        drop(
            txn.open_table(PROBE_TABLE)
                .expect("open probe table (setup)"),
        );
        for def in FILLER_TABLES.iter().copied().take(n_tables - 1) {
            drop(txn.open_table(def).expect("open filler table (setup)"));
        }
        txn.commit().expect("commit setup");
        Self { db: Some(db), dir }
    }
}

impl Drop for BenchDb {
    fn drop(&mut self) {
        self.db.take(); // close the database before unlinking its directory
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn redb_is_empty_probe(c: &mut Criterion) {
    let mut group = c.benchmark_group("redb_is_empty_probe");
    let arms: &[(&str, Probe)] = &[
        ("list_tables_walk", probe_list_tables_walk),
        ("open_table_header", probe_open_table_header),
    ];
    for &n_tables in &[1usize, 9] {
        for &(arm, probe) in arms {
            let bench = BenchDb::new(n_tables);
            let db = bench.db.as_ref().expect("bench database");
            group.bench_with_input(
                BenchmarkId::new(arm, format!("tables/{n_tables}")),
                &probe,
                |b, &probe| {
                    b.iter_custom(|iters| {
                        let mut total = Duration::ZERO;
                        for _ in 0..iters {
                            let write_txn = db.begin_write().expect("begin_write");
                            let start = Instant::now();
                            let empty = probe(&write_txn);
                            let elapsed = start.elapsed();
                            std::hint::black_box(empty);
                            drop(write_txn); // abort: no commit, no fsync
                            total += elapsed;
                        }
                        total
                    });
                },
            );
        }
    }
    group.finish();
}

criterion_group!(benches, redb_is_empty_probe);
criterion_main!(benches);
