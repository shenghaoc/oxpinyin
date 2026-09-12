//! Criterion wall-clock and child-process VmHWM for libpinyin's DBM backends.
//!
//! Measures the user-data path through the C facade — `pinyin_init`,
//! `pinyin_train`, `pinyin_save` — against whichever oracle prefix
//! `PINYIN_ORACLE_PREFIX` names (a prefix from `tools/oracle/build-oracle.sh`,
//! optionally `--dbm`-selected; non-tkrzw prefixes additionally need
//! `PINYIN_BENCH_DBM`, see `docs/testing/oracle-environment.md`). The facade
//! is byte-identical across the three DBM builds, so the *delta* between runs
//! isolates the backend; absolute numbers are facade+DBM, not storage-tier,
//! and are not comparable with oxpinyin's store-tier benches.
//!
//! Operations (group `libpinyin_dbm`):
//! - `init_load` — one `pinyin_init` over the prefix's system data and a
//!   fresh empty user dir. Timed with `iter_custom`; teardown is outside the
//!   timed window. Includes the fresh-dir `mkdir` (µs-scale, common to all
//!   backends).
//! - `train_write/64`, `train_write/256` — N train events over the fixed
//!   input set below plus one `pinyin_save` (the commit). `iter_batched`
//!   reopens from a clean user dir per iteration; the oracle is returned from
//!   the routine so its `Drop` lands outside the timed region.
//! - `user_db_open` — `pinyin_init` over a copy of a pre-populated user dir
//!   (256 trains + save, i.e. exactly a `train_write/256` state). Each
//!   iteration copies the dir outside the timed window so every open sees
//!   identical bytes.
//!
//! Wall-clock (criterion):
//!
//! ```text
//! PINYIN_ORACLE_PREFIX=<prefix> cargo bench -p pinyin-oracle \
//!   --features oracle-ffi --bench dbm_bench -- --save-baseline <name>
//! ```
//!
//! RAM is a separate axis with a separate mechanism, because criterion cannot
//! measure it: `--vmhwm` re-executes this binary as one child per operation
//! (the `backend_bench.rs` pattern) and reports each child's `/proc/self/status`
//! VmHWM. Do not mix the two axes in one table.

#![allow(missing_docs)]
// The gate mirrors this target's `required-features`: cargo already skips
// the bench without `oracle-ffi`, but rust-analyzer analyzes
// required-features targets regardless and would flag the `Oracle*`
// imports (that feature's `ffi` module) as unresolved on any host where
// the feature is off — which is every non-Linux host, since the feature
// is Linux-only.
#![cfg(feature = "oracle-ffi")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use criterion::{BatchSize, Criterion};
use pinyin_oracle::{Oracle, OracleFlags, OraclePrefix};

/// Train inputs, fixed and deterministic: common full-pinyin phrases that
/// parse completely and always yield candidates under `OracleFlags::DEFAULT`.
/// Cycling through a set spreads bigram updates across distinct prev tokens,
/// the way real training does.
const TRAIN_INPUTS: [&str; 16] = [
    "zhongguo",
    "renmin",
    "beijing",
    "shanghai",
    "zhongguorenmin",
    "gongheguo",
    "tiananmen",
    "nihaoma",
    "jintian",
    "mingtian",
    "xuesheng",
    "laoshi",
    "xuexiao",
    "gongzuo",
    "xihuan",
    "xiawu",
];

/// Population size for the `user_db_open` fixture: identical to
/// `train_write/256`, so the open measurement reopens exactly the state the
/// write measurement produced.
const USER_DB_POPULATE_TRAINS: usize = 256;

fn bench_prefix() -> OraclePrefix {
    let Some(root) = std::env::var_os("PINYIN_ORACLE_PREFIX") else {
        eprintln!(
            "dbm_bench: PINYIN_ORACLE_PREFIX must name the oracle prefix to measure \
             (built by tools/oracle/build-oracle.sh, optionally --dbm-selected)"
        );
        std::process::exit(2);
    };
    match OraclePrefix::open(&root) {
        Ok(prefix) => prefix,
        Err(error) => {
            eprintln!("dbm_bench: cannot open prefix {root:?}: {error}");
            std::process::exit(2);
        }
    }
}

fn train_and_save(oracle: &mut Oracle, n: usize) {
    {
        let mut session = oracle
            .session(OracleFlags::DEFAULT)
            .expect("oracle session");
        // pinyin_train has a non-obvious precondition: it trains n-best
        // result `index`, not a candidate from pinyin_guess_candidates,
        // and returns false when no n-best results exist (pinyin.cpp:2676
        // at the pin). train_top runs pinyin_guess_sentence first for
        // exactly this reason; calling pinyin_train on a candidates-only
        // instance silently turns every train into a false return.
        for i in 0..n {
            let input = TRAIN_INPUTS[i % TRAIN_INPUTS.len()];
            session
                .train_top(input.as_bytes())
                .unwrap_or_else(|error| panic!("train_top {i} ({input:?}): {error}"));
        }
    }
    oracle.save_user_data().expect("pinyin_save");
}

/// Owns one bench's temporary root directory and removes it, with everything
/// written under it, on drop — without the guard every run leaves a per-pid
/// root behind until temp storage fills (the same defect the review flagged
/// in backend_matrix's support module). Oracles opened under the root are
/// dropped before the guard: criterion drops `iter_batched` outputs inside
/// the measured-bench call, and the per-iteration oracles in `iter_custom`
/// are dropped explicitly first.
struct BenchRoot(PathBuf);

impl BenchRoot {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("dbm-bench-{tag}-{}", std::process::id()));
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
        // Best effort: a leftover root is untidy, not incorrect, and Drop
        // must not panic.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            std::fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn bench_init_load(c: &mut Criterion) {
    let prefix = bench_prefix();
    let parent = BenchRoot::new("init-load");
    let parent_path = parent.path().to_path_buf();
    c.bench_function("libpinyin_dbm/init_load", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let started = Instant::now();
                let oracle = Oracle::open(prefix.clone(), &parent_path).expect("pinyin_init");
                total += started.elapsed();
                drop(oracle);
            }
            total
        })
    });
}

fn bench_train_write(c: &mut Criterion, n: usize, name: &'static str) {
    let prefix = bench_prefix();
    let parent = BenchRoot::new(&format!("train-{n}"));
    let parent_path = parent.path().to_path_buf();
    c.bench_function(name, move |b| {
        b.iter_batched(
            || Oracle::open(prefix.clone(), &parent_path).expect("pinyin_init"),
            |mut oracle| {
                train_and_save(&mut oracle, n);
                oracle
            },
            BatchSize::PerIteration,
        )
    });
}

fn populate_user_dir(prefix: &OraclePrefix, dir: &Path) {
    std::fs::create_dir_all(dir).expect("populate dir");
    let mut oracle = Oracle::open_with_user_dir(prefix.clone(), dir).expect("pinyin_init");
    train_and_save(&mut oracle, USER_DB_POPULATE_TRAINS);
}

fn bench_user_db_open(c: &mut Criterion) {
    let prefix = bench_prefix();
    let root = BenchRoot::new("user-db-open");
    let populated = root.path().join("populated");
    populate_user_dir(&prefix, &populated);

    c.bench_function("libpinyin_dbm/user_db_open", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for i in 0..iters {
                let target = root.path().join(format!("open-{i}"));
                copy_dir(&populated, &target).expect("copy populated user dir");
                let started = Instant::now();
                let oracle =
                    Oracle::open_with_user_dir(prefix.clone(), &target).expect("pinyin_init");
                total += started.elapsed();
                drop(oracle);
            }
            // Untimed cleanup of the per-iteration copies.
            for i in 0..iters {
                let _ = std::fs::remove_dir_all(root.path().join(format!("open-{i}")));
            }
            total
        })
    });
}

fn bench_libpinyin_dbm(c: &mut Criterion) {
    bench_init_load(c);
    bench_train_write(c, 64, "libpinyin_dbm/train_write/64");
    bench_train_write(c, 256, "libpinyin_dbm/train_write/256");
    bench_user_db_open(c);
}

// ── VmHWM mode: one child per operation ─────────────────────────────

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

/// Populates `dir` (arg after the flag) as the `user_db_open` fixture, in this
/// child, so the later open-measuring child never pays the population's RAM.
fn run_vmhwm_populate(dir: &str) {
    let prefix = bench_prefix();
    populate_user_dir(&prefix, Path::new(dir));
    emit("op", "vmhwm_populate");
    emit("trains", USER_DB_POPULATE_TRAINS);
}

fn run_vmhwm_child(op: &str) {
    let prefix = bench_prefix();
    let root = BenchRoot::new(&format!("vmhwm-{op}"));
    match op {
        "init_load" => {
            let oracle = Oracle::open(prefix, root.path()).expect("pinyin_init");
            drop(oracle);
        }
        "train_write_64" | "train_write_256" => {
            let n = if op == "train_write_64" { 64 } else { 256 };
            let mut oracle = Oracle::open(prefix, root.path()).expect("pinyin_init");
            train_and_save(&mut oracle, n);
        }
        "user_db_open" => {
            let populated = std::env::var_os("DBM_BENCH_POPULATED")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    eprintln!("dbm_bench: user_db_open child needs DBM_BENCH_POPULATED");
                    std::process::exit(2);
                });
            let once = root.path().join("once");
            copy_dir(&populated, &once).expect("copy populated user dir");
            let oracle = Oracle::open_with_user_dir(prefix, &once).expect("pinyin_init");
            drop(oracle);
        }
        other => {
            eprintln!("dbm_bench: unknown --vmhwm-child op {other:?}");
            std::process::exit(2);
        }
    }
    emit("op", op);
    match vm_hwm_kib() {
        Some(kib) => emit("vmhwm_kib", kib),
        None => emit("vmhwm_kib", "unavailable"),
    }
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

fn run_vmhwm_parent() {
    let ops = [
        "init_load",
        "train_write_64",
        "train_write_256",
        "user_db_open",
    ];

    // Population runs in its own child: its peak RSS must not leak into the
    // user_db_open measurement. The child populates the directory in place.
    let pop_root = BenchRoot::new("vmhwm-populated");
    let populated = pop_root.path().join("user");
    spawn_child(&["--vmhwm-populate", &populated.to_string_lossy()], None);

    println!("dbm_bench --vmhwm — one child per operation, /proc/self/status VmHWM");
    println!("{:<16} {:>12}", "op", "vmhwm_kib");
    for op in ops {
        let rows = if op == "user_db_open" {
            spawn_child(
                &["--vmhwm-child", op],
                Some(("DBM_BENCH_POPULATED", &populated)),
            )
        } else {
            spawn_child(&["--vmhwm-child", op], None)
        };
        println!("{:<16} {:>12}", op, lookup(&rows, "vmhwm_kib"));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--vmhwm-populate") {
        let dir = args
            .get(pos + 1)
            .expect("--vmhwm-populate needs a directory argument");
        run_vmhwm_populate(dir);
        return;
    }
    if let Some(pos) = args.iter().position(|a| a == "--vmhwm-child") {
        let op = args
            .get(pos + 1)
            .expect("--vmhwm-child needs an operation argument");
        run_vmhwm_child(op);
        return;
    }
    if args.iter().any(|a| a == "--vmhwm") {
        run_vmhwm_parent();
        return;
    }

    let mut criterion = Criterion::default().configure_from_args();
    bench_libpinyin_dbm(&mut criterion);
    criterion.final_summary();
}
