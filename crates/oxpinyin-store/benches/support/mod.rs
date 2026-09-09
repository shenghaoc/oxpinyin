//! Shared implementation for the four `backend_matrix_*` criterion benches.
//!
//! One bench target per backend (`required-features` per the b8aff564
//! convention), all driving this module over the store-tier traits, so the
//! four runs measure identical work over identical bytes. Operations mirror
//! `pinyin-oracle`'s `dbm_bench` where the store API allows, so the two
//! reports' rows align:
//!
//! - `init_load` — open the six system DBMs of the pre-built w3 fixture
//!   directory (`fixtures/w3/<ext>`), the same opens production makes: tree
//!   open for five, hash open for `bigram`. Timed opens; teardown untimed.
//! - `train_write/{64,256}` — N bigram + N phrase rows in one write
//!   transaction (its commit is the save). The store file is created per
//!   iteration in untimed setup; the routine times writes + commit only.
//! - `observe_commit` — **one** observation: the four read-modify-write
//!   counter bumps `UserStore::update` commits, in one transaction, on a
//!   pre-populated store. This is the per-commit unit; `train_write`
//!   amortises a single commit over 128/512 puts and cannot resolve it.
//! - `train_sentence/8` — eight of those, each its own transaction: the
//!   shape `Session::train` runs for an eight-token sentence, which is the
//!   user-visible unit built out of eight commits.
//! - `user_db_open` — read-write open of a fresh copy of a pre-populated
//!   user store, copying outside the timed window so every open sees
//!   identical bytes.
//!
//! These are storage-tier numbers: no facade, no lookup, no decode. They are
//! NOT comparable in absolute value with `dbm_bench`'s facade+DBM numbers —
//! within-project deltas are the comparable signal.
//!
//! RAM is a separate axis and a separate mode: `--vmhwm` re-executes this
//! binary as one child per operation and reports each child's
//! `/proc/self/status` VmHWM. Do not mix the two axes in one table.

use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use criterion::{BatchSize, Criterion};

use oxpinyin_store::{DEFAULT_STORE_EXT, RawReadStore, ReadStore, WriteStore};

/// User-store table names, mirroring the user store's own tables.
pub const BIGRAM: &str = "user_bigram";
pub const PHRASE: &str = "user_phrase";
/// The three further counter tables `UserStore::update` touches per
/// observation, named as the user store names them.
pub const BIGRAM_TOTAL: &str = "user_bigram_total";
pub const UNIGRAM: &str = "user_unigram";
pub const UNIGRAM_TOTAL: &str = "user_unigram_total";

/// (stem, libpinyin file name, is-hash) for the six system DBMs — the same
/// naming `oxpinyin-data`'s `SystemDbm` applies: libpinyin's own names on
/// the libpinyin DBM backends, `<stem>.<ext>` on redb and LMDB.
const SYSTEM_DBMS: [(&str, &str, bool); 6] = [
    ("pinyin_index", "pinyin_index.bin", false),
    ("phrase_index", "phrase_index.bin", false),
    ("bigram", "bigram.db", true),
    ("punct", "punct.bin", false),
    ("addon_pinyin_index", "addon_pinyin_index.bin", false),
    ("addon_phrase_index", "addon_phrase_index.bin", false),
];

/// The w3 fixture directory for the compiled-in backend; the directory is
/// named by the store extension (`kct`, `tkt`, `lmdb`, `redb`).
pub fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/w3")
        .join(DEFAULT_STORE_EXT)
}

// ── operations ─────────────────────────────────────────────────────

/// Opens the six system DBMs under `fixture` exactly as production does —
/// tree opens for five, the hash open for `bigram` — and **returns them all
/// open**: production holds the six simultaneously for the process lifetime,
/// so open cost and peak RAM are only representative with every handle
/// retained. Drop the returned `Vec` to tear down.
pub fn open_system_dbms<S>(fixture: &Path) -> Vec<S>
where
    S: ReadStore + RawReadStore,
{
    let mut stores = Vec::with_capacity(SYSTEM_DBMS.len());
    for (index, file) in system_dbm_files().iter().enumerate() {
        let path = fixture.join(file);
        let store = if SYSTEM_DBMS[index].2 {
            S::open_hash_read_only(&path)
        } else {
            S::open_read_only(&path)
        }
        .expect("open system DBM");
        stores.push(store);
    }
    stores
}

/// N bigram + N phrase rows in one transaction; the commit is the save.
pub fn run_train_write<S: WriteStore>(store: &S, n: usize) {
    store
        .write(|txn| {
            for i in 0..n as u64 {
                let (key, value) = bigram_row(SEED, i);
                txn.put(BIGRAM, &key, &value)?;
                let (phrase_key, text) = phrase_row(SEED, i);
                txn.put(PHRASE, &phrase_key, text.as_bytes())?;
            }
            Ok(())
        })
        .expect("train writes");
}

/// Read-modify-write of one big-endian `u64` counter, the shape every
/// counter bump in `UserStore::update` takes (`txn_get_u64_or`, add, put).
fn bump_counter(
    txn: &mut dyn oxpinyin_store::WriteTxn,
    table: &str,
    key: &[u8],
    delta: u64,
) -> Result<(), oxpinyin_store::StoreError> {
    let prev = txn
        .get(table, key)?
        .and_then(|bytes| <[u8; 8]>::try_from(bytes.as_slice()).ok())
        .map_or(0, u64::from_be_bytes);
    txn.put(table, key, &prev.saturating_add(delta).to_be_bytes())?;
    Ok(())
}

/// **One** observation — one write transaction, one commit.
///
/// Mirrors `UserStore::update`: four read-modify-write counter bumps (the
/// `(prev, cur)` bigram pair, `prev`'s bigram total, `cur`'s unigram delta,
/// and the unigram grand total) inside a single transaction, whose commit is
/// the save. `Session::train` calls `observe` once per token of the trained
/// sentence, so every token pays a whole commit — the unit any per-commit
/// cost, a backend's `fsync` included, is charged on. `train_write/{64,256}`
/// amortises one commit over a 128- or 512-put batch and so cannot resolve
/// that cost; this routine is the one that can.
pub fn run_observe_commit<S: WriteStore>(store: &S, i: u64) {
    store
        .write(|txn| {
            let (pair_key, _) = bigram_row(SEED, i);
            bump_counter(txn, BIGRAM, &pair_key, 1)?;
            bump_counter(txn, BIGRAM_TOTAL, &pair_key[..4], 1)?;
            let (token_key, _) = phrase_row(SEED, i);
            bump_counter(txn, UNIGRAM, &token_key, 7)?;
            bump_counter(txn, UNIGRAM_TOTAL, &[0], 7)?;
            Ok(())
        })
        .expect("observe commit");
}

fn count_rows<S: ReadStore>(store: &S, table: &str) -> u64 {
    let mut rows = 0_u64;
    store
        .for_each(table, &mut |_key, _value| {
            rows += 1;
            Ok(())
        })
        .expect("count rows");
    rows
}

/// Creates the `user_db_open` fixture at `path`: exactly a
/// `train_write/256` state, with row-count sanity checks.
fn populate_user_store<S: WriteStore>(path: &Path) {
    let store = S::create(path).expect("create user store");
    run_train_write::<S>(&store, USER_DB_POPULATE_ROWS);
    assert_eq!(count_rows(&store, BIGRAM), USER_DB_POPULATE_ROWS as u64);
    assert_eq!(count_rows(&store, PHRASE), USER_DB_POPULATE_ROWS as u64);
}

/// Population size for the `user_db_open` fixture: identical to
/// `train_write/256`, so the open measurement reopens exactly the state the
/// write measurement produced.
const USER_DB_POPULATE_ROWS: usize = 256;

const SEED: u64 = 0x0BB5_EED1;

// ── criterion entry points ─────────────────────────────────────────

fn bench_init_load<S>(c: &mut Criterion)
where
    S: ReadStore + RawReadStore,
{
    let fixture = fixture_dir();
    assert!(
        fixture.is_dir(),
        "missing fixture dir {}",
        fixture.display()
    );
    c.bench_function("backend_matrix/init_load", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let started = Instant::now();
                let stores = open_system_dbms::<S>(&fixture);
                total += started.elapsed();
                black_box(&stores);
                drop(stores);
            }
            total
        })
    });
}

fn bench_train_write<S>(c: &mut Criterion, n: usize, name: &'static str)
where
    S: WriteStore,
{
    let root = BenchRoot::new(&format!("train-{n}"));
    let root_path = root.path().to_path_buf();
    c.bench_function(name, move |b| {
        b.iter_batched(
            || {
                let path = unique_path(&root_path);
                S::create(&path).expect("create store")
            },
            |store| {
                run_train_write(&store, n);
                store
            },
            BatchSize::PerIteration,
        )
    });
}

/// `observe_commit` / `train_sentence/N`: `commits` successive observations,
/// each its own transaction, on a fresh copy of the pre-populated user store.
///
/// The copy and the open happen in untimed setup, so every iteration starts
/// from byte-identical state and the routine times commits alone — the
/// property that makes two backends', or two builds', numbers comparable.
fn bench_observe_commits<S>(c: &mut Criterion, commits: u64, name: &'static str)
where
    S: WriteStore,
{
    let root = BenchRoot::new(&format!("observe-{commits}"));
    let populated = root.path().join("populated.db");
    populate_user_store::<S>(&populated);
    let root_path = root.path().to_path_buf();

    c.bench_function(name, move |b| {
        b.iter_batched(
            || {
                let path = unique_path(&root_path);
                std::fs::copy(&populated, &path).expect("copy populated user store");
                S::create(&path).expect("open user store")
            },
            |store| {
                for i in 0..commits {
                    run_observe_commit(&store, i);
                }
                store
            },
            BatchSize::PerIteration,
        )
    });
}

fn bench_user_db_open<S>(c: &mut Criterion)
where
    S: WriteStore,
{
    let root = BenchRoot::new("user-db-open");
    let populated = root.path().join("populated.db");
    populate_user_store::<S>(&populated);

    c.bench_function("backend_matrix/user_db_open", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for i in 0..iters {
                let target = root.path().join(format!("open-{i}.db"));
                std::fs::copy(&populated, &target).expect("copy populated user store");
                let started = Instant::now();
                let store = S::create(&target).expect("open user store");
                total += started.elapsed();
                drop(store);
                remove_db_files(&target);
            }
            total
        })
    });
}

// ── VmHWM mode: one child per operation ────────────────────────────

fn vm_hwm_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|kib| kib.parse().ok())
}

fn emit(key: &str, value: impl std::fmt::Display) {
    println!("{key}={value}");
}

/// Populates `dir`/user.db as the `user_db_open` fixture, in this child, so
/// the later open-measuring child never pays the population's RAM.
fn run_vmhwm_populate<S: WriteStore>(dir: &str) {
    let root = PathBuf::from(dir);
    std::fs::create_dir_all(&root).expect("vmhwm populate dir");
    populate_user_store::<S>(&root.join("user.db"));
    emit("op", "vmhwm_populate");
    emit("rows", USER_DB_POPULATE_ROWS);
}

fn run_vmhwm_child<S>(op: &str)
where
    S: ReadStore + RawReadStore + WriteStore,
{
    let root = BenchRoot::new(&format!("vmhwm-{op}"));
    // Holders keep opened handles alive through the VmHWM read at the end:
    // the child's peak must include the open handles, not just the open
    // calls (the production shape holds all six system DBMs at once).
    let mut held_stores: Option<Vec<S>> = None;
    match op {
        "init_load" => {
            let stores = open_system_dbms::<S>(&fixture_dir());
            emit("opened", stores.len());
            held_stores = Some(stores);
        }
        "train_write_64" | "train_write_256" => {
            let n = if op == "train_write_64" { 64 } else { 256 };
            let store = S::create(&root.path().join("train.db")).expect("create store");
            run_train_write::<S>(&store, n);
        }
        "user_db_open" => {
            let populated = std::env::var_os("BACKEND_MATRIX_POPULATED")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    eprintln!("backend_matrix: user_db_open child needs BACKEND_MATRIX_POPULATED");
                    std::process::exit(2);
                });
            let once = root.path().join("once.db");
            std::fs::copy(&populated, &once).expect("copy populated user store");
            let store = S::create(&once).expect("open user store");
            drop(store);
        }
        other => {
            eprintln!("backend_matrix: unknown --vmhwm-child op {other:?}");
            std::process::exit(2);
        }
    }
    emit("op", op);
    match vm_hwm_kib() {
        Some(kib) => emit("vmhwm_kib", kib),
        None => emit("vmhwm_kib", "unavailable"),
    }
    drop(held_stores);
}

fn spawn_child(args: &[&str], env: Option<(&str, &Path)>) -> Vec<(String, String)> {
    let exe = std::env::current_exe().expect("current exe");
    let mut command = Command::new(exe);
    command.args(args);
    if let Some((key, value)) = env {
        command.env(key, value);
    }
    let output = command.output().expect("spawn bench child");
    if !output.status.success() {
        eprintln!(
            "child {args:?} failed:\n{}",
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

fn lookup<'a>(rows: &'a [(String, String)], key: &str) -> &'a str {
    rows.iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .unwrap_or("-")
}

fn run_vmhwm_parent(backend: &str) {
    let ops = [
        "init_load",
        "train_write_64",
        "train_write_256",
        "user_db_open",
    ];

    // Population runs in its own child: its peak RSS must not leak into the
    // user_db_open measurement. The child receives the root directory and
    // writes user.db inside it; `populated` below is that file.
    let pop_root = BenchRoot::new("vmhwm-populated");
    let populated = pop_root.path().join("user.db");
    spawn_child(
        &["--vmhwm-populate", &pop_root.path().to_string_lossy()],
        None,
    );

    println!(
        "backend_matrix --vmhwm [{backend}] — one child per operation, /proc/self/status VmHWM"
    );
    println!("{:<16} {:>12}", "op", "vmhwm_kib");
    for op in ops {
        let rows = if op == "user_db_open" {
            spawn_child(
                &["--vmhwm-child", op],
                Some(("BACKEND_MATRIX_POPULATED", &populated)),
            )
        } else {
            spawn_child(&["--vmhwm-child", op], None)
        };
        println!("{:<16} {:>12}", op, lookup(&rows, "vmhwm_kib"));
    }
}

/// Arg dispatch + criterion entry point; the per-backend bench target's
/// `main` is one line calling this.
pub fn run<S>(backend: &'static str)
where
    S: ReadStore + RawReadStore + WriteStore,
{
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--vmhwm-populate") {
        let dir = args
            .get(pos + 1)
            .expect("--vmhwm-populate needs a directory argument");
        run_vmhwm_populate::<S>(dir);
        return;
    }
    if let Some(pos) = args.iter().position(|a| a == "--vmhwm-child") {
        let op = args
            .get(pos + 1)
            .expect("--vmhwm-child needs an operation argument");
        run_vmhwm_child::<S>(op);
        return;
    }
    if args.iter().any(|a| a == "--vmhwm") {
        run_vmhwm_parent(backend);
        return;
    }

    let mut criterion = Criterion::default().configure_from_args();
    bench_init_load::<S>(&mut criterion);
    bench_train_write::<S>(&mut criterion, 64, "backend_matrix/train_write/64");
    bench_train_write::<S>(&mut criterion, 256, "backend_matrix/train_write/256");
    bench_observe_commits::<S>(&mut criterion, 1, "backend_matrix/observe_commit");
    bench_observe_commits::<S>(&mut criterion, 8, "backend_matrix/train_sentence/8");
    bench_user_db_open::<S>(&mut criterion);
    criterion.final_summary();
}

// ── deterministic workload rows (from backend_bench.rs) ────────────

const TOKEN_BASE: u64 = 0x0100_0000;
const PREV_DOMAIN: u64 = 2048;
const CUR_DOMAIN: u64 = 60_000;
const PHRASE_TOKEN_DOMAIN: u64 = 50_000;

const fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

const fn row_hash(seed: u64, i: u64) -> u64 {
    mix(seed ^ mix(i))
}

/// bigram (prev, cur) key over big-endian u32 halves + a small counter value.
fn bigram_row(seed: u64, i: u64) -> ([u8; 8], [u8; 8]) {
    let h = row_hash(seed, i);
    let prev = TOKEN_BASE + h % PREV_DOMAIN;
    let cur = TOKEN_BASE + (h / PREV_DOMAIN) % CUR_DOMAIN;
    let mut key = [0_u8; 8];
    key[..4].copy_from_slice(&u32::try_from(prev).unwrap().to_be_bytes());
    key[4..].copy_from_slice(&u32::try_from(cur).unwrap().to_be_bytes());
    (key, (1 + h % 97).to_be_bytes())
}

/// phrase key is a bare big-endian token; the value a short CJK string.
fn phrase_row(seed: u64, i: u64) -> ([u8; 4], String) {
    let h = row_hash(seed, i ^ 0x5A5A_5A5A);
    let token = TOKEN_BASE + i % PHRASE_TOKEN_DOMAIN;
    let char_count = 2 + h % 3;
    let mut text = String::with_capacity(3 * char_count as usize);
    for k in 0..char_count {
        let code = 0x4E00 + ((h >> (9 * k + 4)) & 0x1FF);
        text.push(char::from_u32(u32::try_from(code).unwrap()).unwrap_or('词'));
    }
    (u32::try_from(token).unwrap().to_be_bytes(), text)
}

// ── paths ──────────────────────────────────────────────────────────

fn system_dbm_files() -> Vec<PathBuf> {
    SYSTEM_DBMS
        .iter()
        .map(|(stem, libpinyin_name, _)| {
            if matches!(DEFAULT_STORE_EXT, "kct" | "tkt") {
                PathBuf::from(libpinyin_name)
            } else {
                PathBuf::from(format!("{stem}.{DEFAULT_STORE_EXT}"))
            }
        })
        .collect()
}

static PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_path(root: &Path) -> PathBuf {
    root.join(format!(
        "store-{}-{}.db",
        std::process::id(),
        PATH_COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Owns one bench's temporary root directory and removes it, with everything
/// written under it, on drop. Without the guard every run would leave a
/// per-pid root behind (bench_root names embed the pid, so later runs never
/// clean earlier ones) until temp storage fills and bench setup starts
/// failing.
///
/// Store handles created under the root are dropped before the guard:
/// criterion drops `iter_batched` outputs inside the measured-bench call,
/// and the per-iteration stores in `iter_custom` are dropped explicitly
/// before the guard's scope ends.
struct BenchRoot(PathBuf);

impl BenchRoot {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("backend-matrix-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("bench temp dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for BenchRoot {
    fn drop(&mut self) {
        // Best effort, like FreshUserDir: a leftover root is untidy, not
        // incorrect, and Drop must not panic.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Removes a store file plus its `-lock` sidecar, the backend_bench pattern
/// (LMDB writes the sidecar on every open).
fn remove_db_files(path: &Path) {
    let _ = std::fs::remove_file(path);
    let mut lock = path.as_os_str().to_os_string();
    lock.push("-lock");
    let _ = std::fs::remove_file(Path::new(&lock));
}
