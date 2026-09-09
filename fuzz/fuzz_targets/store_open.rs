#![no_main]
//! Hostile bytes through the store seam's file ingress — the DBM
//! container a distro, a user, or an attacker leaves in the data
//! directory `pinyin_init` reads. The two targets that landed
//! 2026-09-08 covered the chunk readers (`phrase_library`) and the one
//! text file (`table_conf`); the DBM containers behind
//! `oxpinyin-store` were the remaining file ingress, and they are the
//! one ingress whose parser is a foreign library.
//!
//! The input **is** the file: the target writes the bytes at a fresh
//! path, opens it through both container classes the runtime uses —
//! `ReadStore::open_read_only`, the tree containers `LookupTable::open`
//! reads, and `RawReadStore::open_hash_read_only`, the hash container
//! `BigramTable::open` reads — and, when an open succeeds, drives every
//! read the tier exposes. No framing byte prefixes the input, so a
//! corpus entry is a database file verbatim and
//! `tools/store/seed-store-fuzz-corpus.sh` can seed the corpus from the
//! committed `fixtures/w3/<backend>/` tables.
//!
//! # The law
//!
//! Totality, and nothing else: constitution rule 4 — no input makes a
//! public store entry point panic, abort, or fault. Every visitor sums
//! the bytes of the key and value it is handed, so a record the
//! container reports with a length its own file does not back is a read
//! that a sanitizer (or the OS) sees rather than one the optimiser
//! elides. Determinism is deliberately **not** asserted: redb repairs
//! an unclean file on open, so two opens of the same bytes may
//! legitimately disagree, and asserting otherwise would report an
//! upstream feature as a defect.
//!
//! # How the backend gets instrumented
//!
//! `cargo fuzz` applies `-Zsanitizer=address` and libFuzzer's coverage
//! instrumentation through `RUSTFLAGS`, so they reach **Rust** code
//! only. What that means per backend — the crate compiles exactly one:
//!
//! * `redb` — pure Rust. Fully instrumented, coverage-guided end to
//!   end; the only backend where the container itself steers the
//!   fuzzer.
//! * `lmdb` — `mdb.c` and `midl.c` are compiled *in this build* by
//!   `lmdb-master-sys`'s `cc` invocation, so `CFLAGS` instruments them:
//!   `CFLAGS="-fsanitize=address -fsanitize-coverage=inline-8bit-counters,pc-table,trace-cmp"`
//!   with `CC=clang` puts LMDB's C under the same sanitizer and the
//!   same coverage feedback as the Rust. `CFLAGS` and not `CXXFLAGS`
//!   on purpose: libfuzzer-sys compiles libFuzzer itself through
//!   `cc::Build::cpp(true)`, and libFuzzer must not be instrumented
//!   with its own coverage.
//! * `tkrzw`, `kyotocabinet` — the container is a system shared object
//!   the distro built. Nothing here can instrument it, and libFuzzer
//!   gets no coverage signal from inside it. The target is still worth
//!   running on them: ASan replaces the process allocator and
//!   intercepts the libc string/memory routines, so an overflow of a
//!   heap chunk or a bad `memcpy` is still caught inside an
//!   uninstrumented `.so`; and the code this crate owns — the
//!   `table || 0x00 || key` framing, the `i32` length conversions, the
//!   borrowed-record callbacks that hand a C pointer and length to a
//!   Rust slice — is fully instrumented and is exactly where a hostile
//!   file's influence lands.
//!
//! `docs/findings/store-file-ingress-fuzzing.md` carries the measured
//! behaviour of each backend under this target and the reason no lane
//! gates on it yet.

use std::ops::Bound;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use libfuzzer_sys::fuzz_target;
use oxpinyin_store::{
    DefaultStore, RawReadStore, ReadStore, StoreError, DEFAULT_STORE_EXT, RAW_TABLE,
};

/// Rows visited before a walk is cut short. A corrupted container can
/// report a cycle or an absurd record count; libFuzzer's `-timeout`
/// would catch the hang, but reporting it as a timeout loses which
/// walk hung.
const MAX_ROWS: u64 = 1 << 16;

/// Table names the runtime actually scans. `RAW_TABLE` ("data") is the
/// one `for_each_row_with_store` walks and the one redb and LMDB
/// delegate their raw reads to; the other two are libpinyin's own
/// names, present so a framed prefix that exists in the file is
/// reachable.
const TABLES: &[&str] = &[RAW_TABLE, "pinyin_index", "bigram"];

/// The previous input's file, removed at the start of the next one —
/// one orphan can remain at process exit, the footprint
/// `user_store_ops` leaves. A **fresh path per input** is required, not
/// merely tidy: the LMDB backend keeps a process-global environment
/// cache keyed by path, so reusing one path would hand every later
/// input the first input's mapping.
static LAST_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
static PATH_SERIAL: AtomicU64 = AtomicU64::new(0);

/// Removes a store file and its `-lock` sidecar, whichever container
/// shape the compiled backend used (file, directory, or both).
fn remove_store(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir_all(path);
    let lock = PathBuf::from(format!("{}-lock", path.display()));
    let _ = std::fs::remove_file(&lock);
    let _ = std::fs::remove_dir_all(&lock);
}

fn fresh_path() -> PathBuf {
    if let Ok(mut last) = LAST_PATH.lock() {
        if let Some(old) = last.take() {
            remove_store(&old);
        }
    }
    std::env::temp_dir().join(format!(
        "oxpinyin-fuzz-store-open-{}-{}.{DEFAULT_STORE_EXT}",
        std::process::id(),
        PATH_SERIAL.fetch_add(1, Ordering::Relaxed)
    ))
}

/// A visitor that reads every byte it is handed and stops the walk once
/// `MAX_ROWS` rows have gone by. The sum is returned so the reads are
/// observable and cannot be optimised away.
fn walk<F>(rows: &mut u64, sum: &mut u64, scan: F)
where
    F: FnOnce(&mut oxpinyin_store::Visitor<'_>) -> Result<(), StoreError>,
{
    let mut visit = |key: &[u8], value: &[u8]| -> Result<(), StoreError> {
        *sum = sum.wrapping_add(key.iter().map(|&b| u64::from(b)).sum::<u64>());
        *sum = sum.wrapping_add(value.iter().map(|&b| u64::from(b)).sum::<u64>());
        *rows += 1;
        if *rows > MAX_ROWS {
            // The only way out of a `Visitor`: the tier's scan methods
            // stop on a visitor error and hand it back to the caller,
            // which discards it below.
            return Err(StoreError::InvalidInput("fuzz row budget exhausted"));
        }
        Ok(())
    };
    let _ = scan(&mut visit);
}

/// Every read the tier exposes, over a handle the target opened.
fn drive(store: &DefaultStore) -> u64 {
    let mut rows = 0u64;
    let mut sum = 0u64;
    for table in TABLES {
        let _ = store.get(table, b"");
        let _ = store.get(table, &7u32.to_be_bytes());
        let _ = store.get(table, &[0xFF; 511]);
        let _ = store.is_empty(table);
        walk(&mut rows, &mut sum, |visit| {
            store.range(table, Bound::Unbounded, Bound::Unbounded, visit)
        });
        walk(&mut rows, &mut sum, |visit| {
            store.range(
                table,
                Bound::Included(b"\x00".as_slice()),
                Bound::Excluded(b"\xff\xff".as_slice()),
                visit,
            )
        });
        walk(&mut rows, &mut sum, |visit| store.for_each(table, visit));
    }
    // The unframed keyspace: libpinyin's own layout, and the half the
    // hash container is read through.
    let _ = store.get_raw(b"");
    let _ = store.get_raw(&7u32.to_le_bytes());
    walk(&mut rows, &mut sum, |visit| {
        store.range_raw(Bound::Unbounded, Bound::Unbounded, visit)
    });
    let _ = store.count_raw();
    sum
}

fuzz_target!(|data: &[u8]| {
    let path = fresh_path();
    if std::fs::write(&path, data).is_err() {
        return;
    }
    // Both container classes, because the runtime opens both: the tree
    // containers through `ReadStore::open_read_only` and `bigram.db`
    // through `RawReadStore::open_hash_read_only`. On redb and LMDB the
    // second delegates to the first; on Kyoto Cabinet and tkrzw it
    // selects a different container class, so the same bytes reach a
    // second parser.
    let mut sum = 0u64;
    if let Ok(store) = DefaultStore::open_read_only(&path) {
        sum = sum.wrapping_add(drive(&store));
    }
    if let Ok(store) = DefaultStore::open_hash_read_only(&path) {
        sum = sum.wrapping_add(drive(&store));
    }
    // Keeps the reads above from being optimised out without asserting
    // anything about a corrupted container's contents.
    std::hint::black_box(sum);
    if let Ok(mut last) = LAST_PATH.lock() {
        *last = Some(path);
    }
});
