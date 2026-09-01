//! Init-time / RSS breakdown for dictionary and LM load.
//!
//! Measurement only — not part of the decode path. Drive it with the
//! full export tables and the fetched model20 cache:
//!
//! ```text
//! PINYIN_EXPORT_DIR=/tmp/oxpinyin-export \
//! PINYIN_MODEL_DIR=$PWD/../pinyin-rs/target/model20/extracted \
//! cargo run -p oxpinyin-data --release --example load_profile
//! ```
//!
//! Optional: `PINYIN_PUNCT_REDB`, `PINYIN_INTERPOLATION2`. Repeats default
//! to 3 (`LOAD_PROFILE_REPEATS`).

#![allow(missing_docs)]

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::hint::black_box;
use std::io::{BufRead, BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use oxpinyin_core::{Dictionary, SyllableKey, syllable_initial};
use oxpinyin_data::{
    BigramLanguageModel, LookupTable, PunctTable, SystemDictionary, build_prefix_tables,
    default_store_file, parse_interpolation2, parse_interpolation2_from_reader,
};
use redb::{ReadableDatabase, ReadableTable};

const DATA_TABLE: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new("data");

fn main() {
    let export = env_dir("PINYIN_EXPORT_DIR", "/tmp/oxpinyin-export");
    let interpolation2 = interpolation2_path();
    let punct = std::env::var_os("PINYIN_PUNCT_REDB").map(PathBuf::from);
    let repeats = std::env::var("LOAD_PROFILE_REPEATS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|&value| value > 0)
        .unwrap_or(3_usize);
    let mode = std::env::args().nth(1).unwrap_or_else(|| "all".to_owned());

    println!("mode: {mode}");
    println!("host RSS at start: {}", rss_line());
    println!("export: {}", export.display());
    println!("interpolation2: {}", interpolation2.display());
    match &punct {
        Some(path) => println!("punct: {}", path.display()),
        None => println!("punct: (unset — open_optional on missing file)"),
    }
    println!("repeats: {repeats}");
    println!();

    match mode.as_str() {
        "inventory" => {
            file_inventory(&export, &interpolation2, punct.as_deref());
            interpolation2_shape(&interpolation2);
        }
        "isolated" => run_isolated(&export, &interpolation2, punct.as_deref(), repeats),
        "all" => {
            file_inventory(&export, &interpolation2, punct.as_deref());
            interpolation2_shape(&interpolation2);
            println!();
            run_isolated(&export, &interpolation2, punct.as_deref(), repeats);
        }
        "cumulative" => {
            println!("=== cumulative capi-shaped init (fresh process) ===");
            cumulative_capi_init(&export, &interpolation2, punct.as_deref());
        }
        "dict" => retain_step("SystemDictionary::open", || {
            SystemDictionary::open(
                &export.join(default_store_file("pinyin_index")),
                &export.join(default_store_file("phrase_index")),
            )
            .expect("dict")
        }),
        "pinyin" => retain_step("LookupTable::open pinyin_index", || {
            LookupTable::open(&export.join(default_store_file("pinyin_index"))).expect("pinyin")
        }),
        "phrase" => retain_step("LookupTable::open phrase_index", || {
            LookupTable::open(&export.join(default_store_file("phrase_index"))).expect("phrase")
        }),
        "lookups" => lookup_profile(&export, repeats),
        "bigram" => retain_step("LookupTable::open bigram", || {
            LookupTable::open(&export.join(default_store_file("bigram"))).expect("bigram")
        }),
        "interp" => retain_step("parse_interpolation2", || {
            parse_interpolation2(&interpolation2).expect("interp")
        }),
        "full" => retain_step("capi-shaped dict+lm+interp+punct", || {
            full_capi_state(&export, &interpolation2, punct.as_deref())
        }),
        "keycosts" => {
            println!("=== dict+lm load then key_cost_table (pinyin_alloc_instance body) ===");
            let mut prev = Instant::now();
            report("start", &mut prev);
            let (dict, lm, punct_table) =
                full_capi_state(&export, &interpolation2, punct.as_deref());
            report("capi-shaped dict+lm+interp+punct", &mut prev);
            let costs = oxpinyin_core::scoring::key_cost_table(&dict, &lm).expect("key costs");
            report("key_cost_table (430 syllable lookups)", &mut prev);
            println!("  key_costs len={}", costs.len());
            drop((dict, lm, punct_table, costs));
        }
        other => {
            eprintln!(
                "unknown mode {other:?}\n\
                 usage: load_profile [all|inventory|isolated|cumulative|dict|lookups|pinyin|phrase|bigram|interp|full|keycosts]"
            );
            std::process::exit(2);
        }
    }
}

fn run_isolated(export: &Path, interpolation2: &Path, punct: Option<&Path>, repeats: usize) {
    println!("=== isolated steps (median of {repeats}) ===");
    profile_redb(&export.join("pinyin_index.redb"), "pinyin_index", repeats);
    profile_redb(&export.join("phrase_index.redb"), "phrase_index", repeats);
    profile_redb(&export.join("bigram.redb"), "bigram", repeats);
    if let Some(path) = punct {
        if path.is_file() {
            // `open_optional` treats a present-but-invalid file as empty.
            // The strict redb slurp profiler panics on that input, so only
            // run it after confirming Option A.
            if PunctTable::open(path).is_ok() {
                profile_redb(path, "punct", repeats);
            } else {
                println!("  punct redb profiler skipped (not a valid Option A table)");
            }
            profile_punct(path, repeats);
        }
    } else {
        profile_punct(Path::new("/no/such/punct.redb"), repeats);
    }
    profile_interpolation2(interpolation2, repeats);
    profile_dict_derived(export, repeats);
    println!();
    println!("=== prepared unigram sidecar vs text parse ===");
    profile_unigram_sidecar(interpolation2, repeats);
}

fn retain_step<T>(label: &str, body: impl FnOnce() -> T) {
    let mut prev = Instant::now();
    report("start", &mut prev);
    let held = body();
    report(label, &mut prev);
    let _ = black_box(&held);
    report("still held", &mut prev);
}

fn full_capi_state(
    export: &Path,
    interpolation2: &Path,
    punct: Option<&Path>,
) -> (SystemDictionary, BigramLanguageModel, PunctTable) {
    let dict = SystemDictionary::open(
        &export.join(default_store_file("pinyin_index")),
        &export.join(default_store_file("phrase_index")),
    )
    .expect("dict");
    let mut lm = BigramLanguageModel::open(&export.join(default_store_file("bigram"))).expect("lm");
    if interpolation2.is_file() {
        lm.set_unigrams_from_interpolation2(interpolation2)
            .expect("interp");
    }
    let punct_table =
        PunctTable::open_optional(punct.unwrap_or_else(|| Path::new("/no/such/punct.redb")));
    (dict, lm, punct_table)
}

fn env_dir(var: &str, default: &str) -> PathBuf {
    std::env::var_os(var).map_or_else(|| PathBuf::from(default), PathBuf::from)
}

fn interpolation2_path() -> PathBuf {
    if let Some(path) = std::env::var_os("PINYIN_INTERPOLATION2") {
        return PathBuf::from(path);
    }
    if let Some(dir) = std::env::var_os("PINYIN_MODEL_DIR") {
        return PathBuf::from(dir).join("interpolation2.text");
    }
    PathBuf::from(
        "/home/sheng/Documents/repos/pinyin-rs/target/model20/extracted/interpolation2.text",
    )
}

fn file_inventory(export: &Path, interpolation2: &Path, punct: Option<&Path>) {
    println!("=== on-disk ===");
    for (label, path) in [
        ("pinyin_index.redb", export.join("pinyin_index.redb")),
        ("phrase_index.redb", export.join("phrase_index.redb")),
        ("bigram.redb", export.join("bigram.redb")),
        ("interpolation2.text", interpolation2.to_path_buf()),
    ] {
        print_file(label, &path);
    }
    if let Some(path) = punct {
        print_file("punct.redb", path);
    }
}

fn print_file(label: &str, path: &Path) {
    match std::fs::metadata(path) {
        Ok(meta) => println!("  {label}: {} bytes  {}", meta.len(), path.display()),
        Err(error) => println!("  {label}: MISSING ({error})  {}", path.display()),
    }
}

fn interpolation2_shape(path: &Path) {
    let Ok(file) = File::open(path) else {
        println!("interpolation2 shape: unreadable");
        return;
    };
    let mut reader = BufReader::new(file);
    let mut buffer = String::new();
    let mut section = String::new();
    let mut one_gram_items = 0_u64;
    let mut two_gram_items = 0_u64;
    let mut one_gram_bytes = 0_u64;
    let mut two_gram_bytes = 0_u64;
    let mut other_bytes = 0_u64;
    loop {
        buffer.clear();
        let Ok(n) = reader.read_line(&mut buffer) else {
            break;
        };
        if n == 0 {
            break;
        }
        let line = buffer.trim_end_matches(['\r', '\n']);
        if line == "\\1-gram" || line == "\\2-gram" || line == "\\end" || line.starts_with("\\data")
        {
            line.clone_into(&mut section);
        }
        let bytes = n as u64;
        match section.as_str() {
            "\\1-gram" => {
                one_gram_bytes += bytes;
                if line.starts_with("\\item") {
                    one_gram_items += 1;
                }
            }
            "\\2-gram" => {
                two_gram_bytes += bytes;
                if line.starts_with("\\item") {
                    two_gram_items += 1;
                }
            }
            _ => other_bytes += bytes,
        }
    }
    println!("=== interpolation2.text shape ===");
    println!("  \\1-gram items: {one_gram_items}  bytes-in-section: {one_gram_bytes}");
    println!("  \\2-gram items: {two_gram_items}  bytes-in-section: {two_gram_bytes}");
    println!("  other bytes (headers): {other_bytes}");
}

fn profile_redb(path: &Path, label: &str, repeats: usize) {
    println!("-- {label} --");
    if !path.is_file() {
        println!("  missing");
        return;
    }

    let open_only = median_of(repeats, || {
        let started = Instant::now();
        let db = redb::Builder::new().open_read_only(path).expect("open");
        let elapsed = started.elapsed();
        drop(db);
        elapsed
    });
    println!("  redb open_read_only (mmap, no slurp): {open_only}");

    let scan = median_of(repeats, || {
        let started = Instant::now();
        let stats = scan_redb(path);
        let elapsed = started.elapsed();
        let _ = black_box(stats);
        elapsed
    });
    let stats = scan_redb(path);
    println!(
        "  redb mmap-scan (iter, drop values): {scan}  rows={}  key_bytes={}  value_bytes={}  payload={}",
        stats.rows,
        stats.key_bytes,
        stats.value_bytes,
        stats.key_bytes + stats.value_bytes
    );

    let slurp_btree = median_of(repeats, || {
        let started = Instant::now();
        let table = LookupTable::open(path).expect("slurp");
        let elapsed = started.elapsed();
        drop(table);
        elapsed
    });
    println!("  LookupTable::open (copy into BTreeMap): {slurp_btree}");

    let slurp_hash = median_of(repeats, || {
        let started = Instant::now();
        let map = slurp_hashmap(path);
        let elapsed = started.elapsed();
        drop(map);
        elapsed
    });
    println!("  slurp into HashMap (copy, no order): {slurp_hash}");

    let iter_ref = {
        let table = LookupTable::open(path).expect("slurp");
        median_of(repeats, || {
            let started = Instant::now();
            let rows = table.iter().count();
            let elapsed = started.elapsed();
            let _ = black_box(rows);
            elapsed
        })
    };
    println!("  LookupTable::iter (borrowed walk): {iter_ref}");
    let iter_clone = {
        let table = LookupTable::open(path).expect("slurp");
        median_of(repeats, || {
            let started = Instant::now();
            let rows: Vec<(Vec<u8>, Vec<u8>)> = table
                .iter()
                .map(|(key, value)| (key.to_vec(), value.to_vec()))
                .collect();
            let elapsed = started.elapsed();
            drop(rows);
            elapsed
        })
    };
    println!("  LookupTable::iter + to_vec (old clone-every-row): {iter_clone}");
}

struct ScanStats {
    rows: u64,
    key_bytes: u64,
    value_bytes: u64,
}

fn scan_redb(path: &Path) -> ScanStats {
    let db = redb::Builder::new().open_read_only(path).expect("open");
    let txn = db.begin_read().expect("txn");
    let table = txn.open_table(DATA_TABLE).expect("table");
    let mut stats = ScanStats {
        rows: 0,
        key_bytes: 0,
        value_bytes: 0,
    };
    for item in table.iter().expect("iter") {
        let (key, value) = item.expect("row");
        stats.rows += 1;
        stats.key_bytes += key.value().len() as u64;
        stats.value_bytes += value.value().len() as u64;
    }
    stats
}

fn slurp_hashmap(path: &Path) -> HashMap<Vec<u8>, Vec<u8>> {
    let db = redb::Builder::new().open_read_only(path).expect("open");
    let txn = db.begin_read().expect("txn");
    let table = txn.open_table(DATA_TABLE).expect("table");
    let mut map = HashMap::new();
    for item in table.iter().expect("iter") {
        let (key, value) = item.expect("row");
        map.insert(key.value().to_vec(), value.value().to_vec());
    }
    map
}

fn profile_punct(path: &Path, repeats: usize) {
    let open = median_of(repeats, || {
        let started = Instant::now();
        let table = PunctTable::open_optional(path);
        let elapsed = started.elapsed();
        drop(table);
        elapsed
    });
    let table = PunctTable::open_optional(path);
    println!(
        "  PunctTable::open_optional: {open}  tokens={}  empty={}",
        table.token_count(),
        table.is_empty()
    );
}

fn profile_interpolation2(path: &Path, repeats: usize) {
    println!("-- interpolation2.text --");
    if !path.is_file() {
        println!("  missing");
        return;
    }

    let read = median_of(repeats, || {
        let started = Instant::now();
        let bytes = std::fs::read(path).expect("read");
        let elapsed = started.elapsed();
        drop(bytes);
        elapsed
    });
    println!("  fs::read (anonymous copy of whole file): {read}");

    let parse = median_of(repeats, || {
        let started = Instant::now();
        let table = parse_interpolation2(path).expect("parse");
        let elapsed = started.elapsed();
        drop(table);
        elapsed
    });
    let table = parse_interpolation2(path).expect("parse");
    println!(
        "  parse_interpolation2 (BufReader, stop at \\2-gram): {parse}  records={}  total={}",
        table.len(),
        table.total()
    );

    let from_bytes = median_of(repeats, || {
        let bytes = std::fs::read(path).expect("read");
        let started = Instant::now();
        let parsed = parse_interpolation2_from_reader(path, BufReader::new(Cursor::new(&bytes)))
            .expect("parse bytes");
        let elapsed = started.elapsed();
        drop(parsed);
        drop(bytes);
        elapsed
    });
    println!("  parse from fs::read Cursor (copy already paid): {from_bytes}");
}

fn profile_dict_derived(export: &Path, repeats: usize) {
    println!("-- SystemDictionary derived maps --");
    let pinyin =
        LookupTable::open(&export.join(default_store_file("pinyin_index"))).expect("pinyin");
    let phrase =
        LookupTable::open(&export.join(default_store_file("phrase_index"))).expect("phrase");

    let unigrams = median_of(repeats, || {
        let started = Instant::now();
        let (map, total) = build_unigram_map(&pinyin);
        let elapsed = started.elapsed();
        let _ = black_box((map, total));
        elapsed
    });
    let (map, total) = build_unigram_map(&pinyin);
    println!(
        "  build_unigram_map (borrowed walk of pinyin_index): {unigrams}  tokens={}  total={total}",
        map.len()
    );

    let prefix = median_of(repeats, || {
        let started = Instant::now();
        let (pinyin_keys, initial_keys) = build_prefix_tables(&pinyin);
        let elapsed = started.elapsed();
        drop(pinyin_keys);
        drop(initial_keys);
        elapsed
    });
    let (pinyin_keys, initial_keys) = build_prefix_tables(&pinyin);
    println!(
        "  build_prefix_tables (borrowed walk of pinyin_index): {prefix}  pinyin_keys={}  initial_keys={}",
        pinyin_keys.len(),
        initial_keys.len()
    );

    let text_tokens = median_of(repeats, || {
        let started = Instant::now();
        let map = build_text_tokens(&phrase);
        let elapsed = started.elapsed();
        drop(map);
        elapsed
    });
    let text_tokens_map = build_text_tokens(&phrase);
    println!(
        "  build_text_tokens (borrowed walk of phrase_index): {text_tokens}  texts={}",
        text_tokens_map.len()
    );

    let open = median_of(repeats, || {
        let started = Instant::now();
        let dict = SystemDictionary::open(
            &export.join(default_store_file("pinyin_index")),
            &export.join(default_store_file("phrase_index")),
        )
        .expect("dict");
        let elapsed = started.elapsed();
        drop(dict);
        elapsed
    });
    println!("  SystemDictionary::open (slurp both + unigrams/prefix; reverse map lazy): {open}");
}

fn build_unigram_map(index: &LookupTable) -> (BTreeMap<u32, u64>, u64) {
    let mut map = BTreeMap::new();
    let mut total = 0_u64;
    for (_, value) in index.iter() {
        if !value.len().is_multiple_of(8) {
            continue;
        }
        for chunk in value.chunks_exact(8) {
            let token = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let freq = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
            *map.entry(token).or_default() += u64::from(freq);
            total = total.saturating_add(u64::from(freq));
        }
    }
    (map, total)
}

fn build_text_tokens(index: &LookupTable) -> BTreeMap<String, Vec<u32>> {
    let mut map: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for (key, value) in index.iter() {
        if key.len() != 4 {
            continue;
        }
        let token = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
        let Ok(text) = std::str::from_utf8(value).map(str::to_owned) else {
            continue;
        };
        map.entry(text).or_default().push(token);
    }
    map
}

fn cumulative_capi_init(export: &Path, interpolation2: &Path, punct: Option<&Path>) {
    println!("  {:<44} {:>10}  RSS/HWM", "step", "ms");
    let mut prev = Instant::now();
    report("start", &mut prev);

    let dict = SystemDictionary::open(
        &export.join(default_store_file("pinyin_index")),
        &export.join(default_store_file("phrase_index")),
    )
    .expect("dict");
    report("SystemDictionary::open", &mut prev);

    let mut lm = BigramLanguageModel::open(&export.join(default_store_file("bigram"))).expect("lm");
    report("BigramLanguageModel::open (slurp bigram)", &mut prev);

    if interpolation2.is_file() {
        lm.set_unigrams_from_interpolation2(interpolation2)
            .expect("interp");
        report("set_unigrams_from_interpolation2", &mut prev);
    } else {
        report("set_unigrams_from_interpolation2 skipped", &mut prev);
    }

    let punct_table =
        PunctTable::open_optional(punct.unwrap_or_else(|| Path::new("/no/such/punct.redb")));
    report("PunctTable::open_optional", &mut prev);

    drop((dict, lm, punct_table));
    report("drop all (allocator may keep RSS)", &mut prev);
}

fn profile_unigram_sidecar(interpolation2: &Path, repeats: usize) {
    if !interpolation2.is_file() {
        println!("  missing interpolation2.text");
        return;
    }
    let table = parse_interpolation2(interpolation2).expect("parse records");
    let records = table.records();
    let sidecar = std::env::temp_dir().join(format!(
        "oxpinyin-unigrams-profile-{}.redb",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&sidecar);
    let started = Instant::now();
    write_unigram_redb(&sidecar, records).expect("write sidecar");
    let write_ms = started.elapsed();
    let size = std::fs::metadata(&sidecar).expect("stat").len();
    println!(
        "  write unigrams.redb: {write_ms:?}  bytes={size}  records={}",
        records.len()
    );

    let load = median_of(repeats, || {
        let started = Instant::now();
        let loaded = LookupTable::open(&sidecar).expect("load sidecar");
        let elapsed = started.elapsed();
        drop(loaded);
        elapsed
    });
    println!("  LookupTable::open unigrams.redb: {load}");

    let scan = median_of(repeats, || {
        let started = Instant::now();
        let stats = scan_redb(&sidecar);
        let elapsed = started.elapsed();
        let _ = black_box(stats);
        elapsed
    });
    println!("  redb mmap-scan unigrams.redb: {scan}");

    let pod_path = std::env::temp_dir().join(format!(
        "oxpinyin-unigrams-profile-{}.bin",
        std::process::id()
    ));
    let started = Instant::now();
    write_unigram_pod(&pod_path, records).expect("write pod");
    let pod_write = started.elapsed();
    let pod_size = std::fs::metadata(&pod_path).expect("stat").len();
    println!("  write aligned (u32,u32,u64) blob: {pod_write:?}  bytes={pod_size}");

    let pod_load = median_of(repeats, || {
        let started = Instant::now();
        let bytes = std::fs::read(&pod_path).expect("read pod");
        let mut count = 0_usize;
        for chunk in bytes.chunks_exact(16) {
            let token = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let _pad = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
            let value = u64::from_le_bytes([
                chunk[8], chunk[9], chunk[10], chunk[11], chunk[12], chunk[13], chunk[14],
                chunk[15],
            ]);
            count += usize::from(token != 0 || value != 0);
        }
        let elapsed = started.elapsed();
        let _ = black_box((bytes, count));
        elapsed
    });
    println!("  fs::read + from_le_bytes walk of POD blob: {pod_load}");

    let _ = std::fs::remove_file(&sidecar);
    let _ = std::fs::remove_file(&pod_path);
}

fn write_unigram_redb(
    path: &Path,
    records: &[(u32, u64)],
) -> Result<(), Box<dyn std::error::Error>> {
    let db = redb::Database::create(path)?;
    {
        let txn = db.begin_write()?;
        {
            let mut out = txn.open_table(DATA_TABLE)?;
            for &(token, count) in records {
                let key = token.to_le_bytes();
                let value = count.to_le_bytes();
                out.insert(key.as_slice(), value.as_slice())?;
            }
        }
        txn.commit()?;
    }
    Ok(())
}

fn write_unigram_pod(
    path: &Path,
    records: &[(u32, u64)],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = Vec::with_capacity(records.len() * 16);
    for &(token, count) in records {
        bytes.extend_from_slice(&token.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

fn report(step: &str, prev: &mut Instant) {
    let elapsed = prev.elapsed();
    println!(
        "  {:<44} {:>8.1}  {}",
        step,
        elapsed.as_secs_f64() * 1000.0,
        rss_line()
    );
    *prev = Instant::now();
}

/// Steady-state lookup timing over the hot decode shapes: one keystroke
/// cycle's worth of `Dictionary::lookup_into` calls for a spread of key
/// lengths, plus the `SEARCH_CONTINUED` prefix probe. Median per batch,
/// so before/after representations compare.
fn lookup_profile(export: &Path, repeats: usize) {
    let dict = SystemDictionary::open(
        &export.join(default_store_file("pinyin_index")),
        &export.join(default_store_file("phrase_index")),
    )
    .expect("dict");
    let shapes: Vec<Vec<SyllableKey>> = ["ni", "ni'hao", "zhong'guo", "xian", "de"]
        .into_iter()
        .map(|spelling| {
            spelling
                .split('\'')
                .map(|syllable| SyllableKey::from_text(syllable).expect("frozen syllable"))
                .collect()
        })
        .collect();
    let mut scratch = Vec::new();
    for shape in &shapes {
        dict.lookup_into(shape, &mut scratch).expect("lookup");
        println!(
            "  {:<12} hits={:<5} prefix={}",
            shape
                .iter()
                .map(|key| key.text())
                .collect::<Vec<_>>()
                .join("'"),
            scratch.len(),
            dict.phrase_prefix_exists(shape).expect("probe")
        );
    }
    let per_batch = median_of(repeats, || {
        let started = Instant::now();
        for _ in 0..1000 {
            for shape in &shapes {
                dict.lookup_into(shape, &mut scratch).expect("lookup");
                let _ = black_box(&scratch);
                let _ = black_box(dict.phrase_prefix_exists(shape).expect("probe"));
            }
        }
        started.elapsed()
    });
    println!("  5000 lookups + 5000 prefix probes: {per_batch}");
}

fn median_of(repeats: usize, mut body: impl FnMut() -> Duration) -> String {
    let mut samples = Vec::with_capacity(repeats);
    for _ in 0..repeats {
        samples.push(body());
    }
    samples.sort();
    let median = samples[samples.len() / 2];
    format!("{:.1} ms", median.as_secs_f64() * 1000.0)
}

fn rss_line() -> String {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return "(no /proc)".to_owned();
    };
    let mut rss = None;
    let mut hwm = None;
    let mut anon = None;
    let mut file = None;
    for line in status.lines() {
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else { continue };
        let Some(value) = parts.next() else { continue };
        match key {
            "VmRSS:" => rss = Some(value.to_owned()),
            "VmHWM:" => hwm = Some(value.to_owned()),
            "RssAnon:" => anon = Some(value.to_owned()),
            "RssFile:" => file = Some(value.to_owned()),
            _ => {}
        }
    }
    format!(
        "RSS={} KiB  HWM={} KiB  RssAnon={} KiB  RssFile={} KiB",
        rss.unwrap_or_else(|| "?".to_owned()),
        hwm.unwrap_or_else(|| "?".to_owned()),
        anon.unwrap_or_else(|| "?".to_owned()),
        file.unwrap_or_else(|| "?".to_owned())
    )
}
