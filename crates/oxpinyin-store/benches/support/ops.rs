//! `--op` mode: one store operation, timed in this process, for paired
//! comparisons of two builds of the same backend.
//!
//! `--op <name> --iters <n> --data <dir>` prepares the operation's inputs
//! untimed, runs it `n` times inside [`measured`] (the region a Callgrind
//! `--toggle-collect` names), and prints `key=value` lines:
//!
//! - `op`, `iters`, `units` — the operation, the iteration count and the
//!   number of store calls the timed region made;
//! - `ns` — the timed region's wall time;
//! - `checksum` — a digest of every result the region produced, so two
//!   builds can be shown to have done the same work on the same bytes;
//! - `vmhwm_kib` — the process's peak resident set at exit.
//!
//! `<dir>` is a libpinyin data directory of the compiled backend (a
//! pin-built one: the oracle's `lib/libpinyin/data`). `--iters 0` runs the
//! setup alone, which is what a whole-process profiler subtracts.
//!
//! Inputs are chosen deterministically from the data: every key of the
//! walked file, bucketed by value size, then an evenly strided sample.

use std::hint::black_box;
use std::ops::{Bound, ControlFlow};
use std::path::{Path, PathBuf};
use std::time::Instant;

use oxpinyin_store::{
    DefaultUserBigramDb, RawReadStore, ReadStore, UserBigramDb, WriteStore, WriteTxn,
};

use super::{BIGRAM, PHRASE, emit, run_train_write, vm_hwm_kib};

/// Every operation the mode knows, with the iteration count the paired
/// runner uses by default.
pub const OPS: [(&str, u64); 21] = [
    ("get_raw.tree.empty", 200_000),
    ("get_raw.tree.small", 200_000),
    ("get_raw.tree.medium", 200_000),
    ("get_raw.tree.large", 100_000),
    ("get_raw.tree.miss", 200_000),
    ("get_raw.hash", 200_000),
    ("scan_raw.suggestion", 20),
    ("scan_raw.full.tree", 5),
    ("scan_raw.full.hash", 2),
    ("framed.get", 200_000),
    ("framed.scan", 50_000),
    ("framed.txn_scan", 2_000),
    ("framed.is_empty", 200_000),
    ("user_bigram.get", 200_000),
    ("user_bigram.store", 200_000),
    ("user_bigram.remove_store", 100_000),
    ("user_bigram.keys", 100),
    ("lifecycle.open_system", 200),
    ("lifecycle.user_load", 50),
    ("lifecycle.user_save", 50),
    ("session.typing_training", 1),
];

/// A timed region's wall time, checksum and store-call count.
type Timed = (std::time::Duration, u64, u64);

/// Runs one step function inside [`measured`].
type Runner<'a> = dyn FnMut(&mut dyn FnMut(u64) -> u64) -> Timed + 'a;

/// Keys sampled per value-size class.
const SAMPLE: usize = 256;
/// Rows the user bigram and the framed store hold.
const USER_ROWS: usize = 4096;

/// The timed region. Not inlined, so a profiler can scope to it by name.
#[inline(never)]
pub fn measured(iters: u64, mut step: impl FnMut(u64) -> u64) -> (u64, u64) {
    let mut checksum = 0_u64;
    let mut units = 0_u64;
    for i in 0..iters {
        let (sum, count) = split(step(i));
        checksum = fold(checksum, sum);
        units += count;
    }
    (checksum, units)
}

/// A step returns its result digest in the high 48 bits and its call
/// count in the low 16.
const fn pack(digest: u64, calls: u64) -> u64 {
    (digest << 16) | (calls & 0xFFFF)
}

const fn split(value: u64) -> (u64, u64) {
    (value >> 16, value & 0xFFFF)
}

const fn fold(acc: u64, value: u64) -> u64 {
    (acc ^ value).wrapping_mul(0x0100_0000_01B3).rotate_left(5)
}

fn digest(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0xCBF2_9CE4_8422_2325_u64 ^ bytes.len() as u64, |acc, &b| {
            (acc ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3)
        })
}

fn get_digest(value: Option<Vec<u8>>) -> u64 {
    value.map_or(1, |bytes| digest(&bytes))
}

/// Every row of a raw store, in key order.
fn all_rows<S: RawReadStore>(store: &S) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut rows = Vec::new();
    store
        .range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
            rows.push((key.to_vec(), value.to_vec()));
            Ok(())
        })
        .expect("walk raw rows");
    rows
}

/// An evenly strided sample of at most [`SAMPLE`] items.
fn stride<T: Clone>(items: &[T]) -> Vec<T> {
    let step = items.len().div_ceil(SAMPLE).max(1);
    items.iter().step_by(step).take(SAMPLE).cloned().collect()
}

/// The exclusive upper bound of the keys that extend `prefix`.
fn prefix_upper(prefix: &[u8]) -> Option<Vec<u8>> {
    let last = prefix.iter().rposition(|&byte| byte != 0xFF)?;
    let mut bound = prefix[..=last].to_vec();
    bound[last] += 1;
    Some(bound)
}

struct Inputs {
    tree: PathBuf,
    pinyin: PathBuf,
    hash: PathBuf,
}

impl Inputs {
    fn new(data: &Path) -> Self {
        Self {
            tree: data.join("phrase_index.bin"),
            pinyin: data.join("pinyin_index.bin"),
            hash: data.join("bigram.db"),
        }
    }
}

/// Runs `--op` and prints its record.
pub fn run<S>(op: &str, iters: u64, data: &Path)
where
    S: ReadStore + RawReadStore + WriteStore,
{
    let inputs = Inputs::new(data);
    let scratch = super::BenchRoot::new(&format!("op-{op}"));
    let mut run = |step: &mut dyn FnMut(u64) -> u64| {
        let started = Instant::now();
        let (checksum, units) = measured(iters, step);
        (started.elapsed(), checksum, units)
    };
    let (elapsed, checksum, units) = match op {
        "get_raw.tree.empty"
        | "get_raw.tree.small"
        | "get_raw.tree.medium"
        | "get_raw.tree.large" => {
            // The phrase index holds the empty markers and the short token
            // lists; the pinyin index the longer item arrays.
            let (file, class): (&Path, fn(usize) -> bool) = match op {
                "get_raw.tree.empty" => (&inputs.tree, |len| len == 0),
                "get_raw.tree.small" => (&inputs.tree, |len| (1..=16).contains(&len)),
                "get_raw.tree.medium" => (&inputs.pinyin, |len| (17..=256).contains(&len)),
                _ => (&inputs.pinyin, |len| len > 256),
            };
            let store = S::open_read_only(file).expect("open index");
            let rows = all_rows(&store);
            let keys: Vec<Vec<u8>> = stride(
                &rows
                    .iter()
                    .filter(|(_, value)| class(value.len()))
                    .map(|(key, _)| key.clone())
                    .collect::<Vec<_>>(),
            );
            assert!(!keys.is_empty(), "no keys in class {op}");
            drop(rows);
            run(&mut |i| {
                let key = &keys[(i as usize) % keys.len()];
                pack(get_digest(store.get_raw(key).expect("get_raw")), 1)
            })
        }
        "get_raw.tree.miss" => {
            let store = S::open_read_only(&inputs.tree).expect("open phrase index");
            // Keys past every UCS-4 phrase: code points above U+10FFFF.
            let keys: Vec<[u8; 8]> = (0..SAMPLE as u32)
                .map(|i| {
                    let mut key = [0_u8; 8];
                    key[..4].copy_from_slice(&(0x0011_0000 + i).to_le_bytes());
                    key[4..].copy_from_slice(&(0x0011_0000 + i * 7).to_le_bytes());
                    key
                })
                .collect();
            run(&mut |i| {
                let key = &keys[(i as usize) % keys.len()];
                pack(get_digest(store.get_raw(key).expect("get_raw")), 1)
            })
        }
        "get_raw.hash" => {
            let store = S::open_hash_read_only(&inputs.hash).expect("open bigram");
            let keys: Vec<Vec<u8>> = stride(
                &all_rows(&store)
                    .into_iter()
                    .map(|(key, _)| key)
                    .collect::<Vec<_>>(),
            );
            run(&mut |i| {
                let key = &keys[(i as usize) % keys.len()];
                pack(get_digest(store.get_raw(key).expect("get_raw")), 1)
            })
        }
        "scan_raw.suggestion" => {
            // `PhraseTable::search_suggestion`'s walk: every key extending
            // a one-character prefix, from the prefix itself.
            let store = S::open_read_only(&inputs.tree).expect("open phrase index");
            let prefixes: Vec<Vec<u8>> = stride(
                &all_rows(&store)
                    .into_iter()
                    .filter(|(key, _)| key.len() == 4)
                    .map(|(key, _)| key)
                    .collect::<Vec<_>>(),
            );
            run(&mut |i| {
                let prefix = &prefixes[(i as usize) % prefixes.len()];
                let upper = prefix_upper(prefix);
                let mut sum = 0_u64;
                store
                    .scan_raw(
                        Bound::Included(prefix),
                        upper.as_deref().map_or(Bound::Unbounded, Bound::Excluded),
                        |key, value| {
                            sum = fold(sum, digest(key) ^ digest(value));
                            Ok(ControlFlow::Continue(()))
                        },
                    )
                    .expect("scan_raw");
                pack(sum, 1)
            })
        }
        "scan_raw.full.tree" | "scan_raw.full.hash" => {
            let store = if op.ends_with("tree") {
                S::open_read_only(&inputs.tree)
            } else {
                S::open_hash_read_only(&inputs.hash)
            }
            .expect("open");
            run(&mut |_| {
                let mut sum = 0_u64;
                store
                    .range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
                        sum = fold(sum, digest(key) ^ digest(value));
                        Ok(())
                    })
                    .expect("range_raw");
                pack(sum, 1)
            })
        }
        "framed.get" | "framed.scan" | "framed.txn_scan" | "framed.is_empty" => {
            let store = S::create_in_memory().expect("in-memory store");
            run_train_write(&store, USER_ROWS);
            let keys: Vec<Vec<u8>> = stride(
                &(0..USER_ROWS as u64)
                    .map(|i| super::bigram_row(super::SEED, i).0.to_vec())
                    .collect::<Vec<_>>(),
            );
            match op {
                "framed.get" => run(&mut |i| {
                    let key = &keys[(i as usize) % keys.len()];
                    pack(get_digest(store.get(BIGRAM, key).expect("get")), 1)
                }),
                "framed.scan" => run(&mut |i| {
                    let key = &keys[(i as usize) % keys.len()];
                    let mut sum = 0_u64;
                    let mut rows = 0;
                    store
                        .scan(BIGRAM, Bound::Included(key), Bound::Unbounded, |k, v| {
                            sum = fold(sum, digest(k) ^ digest(v));
                            rows += 1;
                            Ok(if rows == 16 {
                                ControlFlow::Break(())
                            } else {
                                ControlFlow::Continue(())
                            })
                        })
                        .expect("scan");
                    pack(sum, 1)
                }),
                "framed.txn_scan" => run(&mut |i| {
                    let key = &keys[(i as usize) % keys.len()];
                    let mut sum = 0_u64;
                    store
                        .write(|txn| {
                            let mut rows = 0;
                            txn.scan(PHRASE, Bound::Unbounded, Bound::Unbounded, |k, v| {
                                sum = fold(sum, digest(k) ^ digest(v));
                                rows += 1;
                                Ok(if rows == 16 {
                                    ControlFlow::Break(())
                                } else {
                                    ControlFlow::Continue(())
                                })
                            })?;
                            sum = fold(sum, get_digest(txn.get(BIGRAM, key)?));
                            Ok(())
                        })
                        .expect("txn scan");
                    pack(sum, 2)
                }),
                _ => run(&mut |i| {
                    let table = if i % 2 == 0 { BIGRAM } else { "absent_table" };
                    pack(u64::from(store.is_empty(table).expect("is_empty")), 1)
                }),
            }
        }
        "user_bigram.get"
        | "user_bigram.store"
        | "user_bigram.remove_store"
        | "user_bigram.keys" => {
            let rows = user_rows::<S>(&inputs.hash);
            let db = DefaultUserBigramDb::empty().expect("empty user bigram");
            for (key, value) in &rows {
                db.store(key, value).expect("fill user bigram");
            }
            let sample = stride(&rows);
            match op {
                "user_bigram.get" => run(&mut |i| {
                    let (key, _) = &sample[(i as usize) % sample.len()];
                    pack(get_digest(db.get(key).expect("get")), 1)
                }),
                "user_bigram.store" => run(&mut |i| {
                    let (key, value) = &sample[(i as usize) % sample.len()];
                    db.store(key, value).expect("store");
                    pack(digest(key), 1)
                }),
                "user_bigram.remove_store" => run(&mut |i| {
                    let (key, value) = &sample[(i as usize) % sample.len()];
                    db.remove(key).expect("remove");
                    db.store(key, value).expect("store");
                    pack(digest(key), 2)
                }),
                _ => run(&mut |_| {
                    let keys = db.keys().expect("keys");
                    let sum = keys.iter().fold(0, |acc, key| fold(acc, digest(key)));
                    pack(sum, 1)
                }),
            }
        }
        "lifecycle.open_system" => run(&mut |_| {
            let stores = super::open_system_dbms::<S>(data);
            let count = stores.len() as u64;
            drop(black_box(stores));
            pack(count, 6)
        }),
        "lifecycle.user_load" | "lifecycle.user_save" => {
            let rows = user_rows::<S>(&inputs.hash);
            let index_rows = index_rows::<S>(&inputs.tree);
            let bigram = scratch.path().join("user_bigram.db");
            let index = scratch.path().join("user_phrase_index.bin");
            let db = DefaultUserBigramDb::empty().expect("empty user bigram");
            for (key, value) in &rows {
                db.store(key, value).expect("fill user bigram");
            }
            db.save_db(&bigram).expect("save user bigram");
            S::write_user_index(&index, &index_rows).expect("write user index");
            if op == "lifecycle.user_load" {
                drop(db);
                run(&mut |_| {
                    let loaded = DefaultUserBigramDb::load_db(&bigram).expect("load user bigram");
                    let opened = S::open_user_index(&index).expect("open user index");
                    let sum = fold(
                        get_digest(loaded.get(&rows[0].0).expect("get")),
                        get_digest(opened.get_raw(&index_rows[0].0).expect("get")),
                    );
                    drop(black_box((loaded, opened)));
                    pack(sum, 2)
                })
            } else {
                run(&mut |_| {
                    db.save_db(&bigram).expect("save user bigram");
                    S::write_user_index(&index, &index_rows).expect("write user index");
                    pack(
                        digest(&std::fs::read(&bigram).expect("read saved bigram")),
                        2,
                    )
                })
            }
        }
        "session.typing_training" => session::<S>(data, &mut run),
        other => {
            eprintln!("backend_matrix: unknown --op {other:?}");
            std::process::exit(2);
        }
    };
    emit("op", op);
    emit("iters", iters);
    emit("units", units);
    emit("ns", elapsed.as_nanos());
    emit("checksum", format!("{checksum:#018x}"));
    match vm_hwm_kib() {
        Some(kib) => emit("vmhwm_kib", kib),
        None => emit("vmhwm_kib", "unavailable"),
    }
}

/// The first [`USER_ROWS`] rows of the system bigram, in key order: real
/// `SingleGram` values under real 4-byte token keys.
fn user_rows<S: RawReadStore>(hash: &Path) -> Vec<(Vec<u8>, Vec<u8>)> {
    let store = S::open_hash_read_only(hash).expect("open bigram");
    let mut rows = all_rows(&store);
    rows.truncate(USER_ROWS);
    rows
}

/// The first [`USER_ROWS`] rows of the phrase index, in key order.
fn index_rows<S: RawReadStore>(tree: &Path) -> Vec<(Vec<u8>, Vec<u8>)> {
    let store = S::open_read_only(tree).expect("open phrase index");
    let mut rows = all_rows(&store);
    rows.truncate(USER_ROWS);
    rows
}

/// A typing and training session at the store tier, in the order the
/// engine issues it: the six system tables opened and the user files
/// loaded; per keystroke, the pinyin-index and phrase-index lookups and a
/// suggestion walk of the typed prefix; per committed sentence, the user
/// bigram's read-modify-write and the session store's counter bumps; at
/// the end, the user files saved and everything torn down.
fn session<S>(data: &Path, run: &mut Runner<'_>) -> Timed
where
    S: ReadStore + RawReadStore + WriteStore,
{
    let scratch = super::BenchRoot::new("op-session");
    let rows = user_rows::<S>(&data.join("bigram.db"));
    let pinyin: Vec<Vec<u8>> = {
        let store = S::open_read_only(&data.join("pinyin_index.bin")).expect("open pinyin index");
        stride(
            &all_rows(&store)
                .into_iter()
                .map(|(key, _)| key)
                .collect::<Vec<_>>(),
        )
    };
    let phrases: Vec<Vec<u8>> = {
        let store = S::open_read_only(&data.join("phrase_index.bin")).expect("open phrase index");
        stride(
            &all_rows(&store)
                .into_iter()
                .filter(|(key, _)| key.len() == 4)
                .map(|(key, _)| key)
                .collect::<Vec<_>>(),
        )
    };
    let bigram_file = scratch.path().join("user_bigram.db");
    {
        let db = DefaultUserBigramDb::empty().expect("empty user bigram");
        for (key, value) in rows.iter().take(512) {
            db.store(key, value).expect("fill user bigram");
        }
        db.save_db(&bigram_file).expect("save user bigram");
    }
    run(&mut |round| {
        let stores = super::open_system_dbms::<S>(data);
        let (pinyin_index, phrase_index) = (&stores[0], &stores[1]);
        let user = DefaultUserBigramDb::load_db(&bigram_file).expect("load user bigram");
        let session = S::create_in_memory().expect("session store");
        let mut sum = 0_u64;
        let mut calls = 0_u64;
        for sentence in 0..8_u64 {
            // Eight keystrokes per sentence.
            for key in 0..8_u64 {
                let at = ((round * 64 + sentence * 8 + key) as usize) % pinyin.len();
                sum = fold(
                    sum,
                    get_digest(pinyin_index.get_raw(&pinyin[at]).expect("pinyin")),
                );
                let prefix = &phrases[at % phrases.len()];
                sum = fold(
                    sum,
                    get_digest(phrase_index.get_raw(prefix).expect("phrase")),
                );
                let upper = prefix_upper(prefix);
                let mut walked = 0;
                phrase_index
                    .scan_raw(
                        Bound::Included(prefix),
                        upper.as_deref().map_or(Bound::Unbounded, Bound::Excluded),
                        |k, _| {
                            sum = fold(sum, digest(k));
                            walked += 1;
                            Ok(if walked == 64 {
                                ControlFlow::Break(())
                            } else {
                                ControlFlow::Continue(())
                            })
                        },
                    )
                    .expect("suggestion");
                calls += 3;
            }
            // Training: the user bigram's load/store, then one commit.
            let (key, value) = &rows[((round * 8 + sentence) as usize) % 512];
            let current = user
                .get(key)
                .expect("user get")
                .unwrap_or_else(|| value.clone());
            user.store(key, &current).expect("user store");
            super::run_observe_commit(&session, round * 8 + sentence);
            calls += 3;
        }
        user.save_db(&bigram_file).expect("save user bigram");
        sum = fold(sum, digest(&std::fs::read(&bigram_file).expect("read")));
        drop(black_box((stores, user, session)));
        pack(sum, calls + 1)
    })
}
