//! `oxpinyin-datagen` — compile the pinned model20 text into oxpinyin
//! runtime tables for one storage backend.
//!
//! ```text
//! oxpinyin-datagen compile [--model-dir DIR] [--out-dir DIR]
//!                          [--backend redb|lmdb|tkrzw|kyotocabinet] [--mini]
//!                          [--tables system,addon,punct]
//! ```
//!
//! The model directory is discovered exactly as the differential harness
//! discovers it (`PINYIN_MODEL_DIR`, `PINYIN_MODEL_CACHE/extracted`, the
//! workspace `target/model20/extracted` cache); run
//! `tools/model/fetch-model.sh` first. The output directory receives the
//! tables, a copy of `interpolation2.text` (the engine's system dir reads
//! it), and `datagen-manifest.txt` with the run's provenance.
//!
//! `--mini` reproduces the committed `fixtures/w3/` subset — the
//! regression recipe, not a shipping path.

#![forbid(unsafe_code)]
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use oxpinyin_datagen::manifest::{Manifest, TableRecord};
use oxpinyin_datagen::write::Backend;
use oxpinyin_datagen::{DatagenError, addon, punct, system};

/// The `database format:` token of the emitted `table.conf`, per drop-in
/// backend — the same string the corresponding libpinyin build writes
/// (`KyotoCabinet` / `Tkrzw`; the container bytes are identical for both
/// DBM families, only the token names the build).
fn database_format_token(backend: Backend) -> &'static str {
    match backend {
        Backend::KyotoCabinet => "KyotoCabinet",
        Backend::Tkrzw => "Tkrzw",
        // redb/LMDB never emit table.conf (not a drop-in backend), so
        // this branch is unreachable for them; the token is defensive.
        _ => "native",
    }
}

#[derive(Debug)]
struct Options {
    model_dir: Option<PathBuf>,
    out_dir: Option<PathBuf>,
    backend: Backend,
    mini: bool,
    tables: Tables,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            model_dir: None,
            out_dir: None,
            // Default selection from `Backend::DEFAULT` (Kyoto Cabinet),
            // which mirrors `oxpinyin_store::DefaultStore` under the
            // workspace's default feature set. Each of the peer backends
            // (redb, lmdb, tkrzw) is reachable through the corresponding
            // `--no-default-features --features <backend>` build plus
            // `--backend <backend>` at the CLI.
            backend: Backend::DEFAULT,
            mini: false,
            tables: Tables::default(),
        }
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
struct Tables {
    system: bool,
    addon: bool,
    punct: bool,
}

fn usage() -> ! {
    eprintln!(
        "usage: oxpinyin-datagen compile [--model-dir DIR] [--out-dir DIR] \
         [--backend redb|lmdb|tkrzw|kyotocabinet] [--mini] \
         [--tables system,addon,punct]"
    );
    std::process::exit(2);
}

fn parse_tables(value: &str) -> Tables {
    let mut tables = Tables::default();
    for part in value.split(',') {
        match part.trim() {
            "system" => tables.system = true,
            "addon" => tables.addon = true,
            "punct" => tables.punct = true,
            _ => usage(),
        }
    }
    tables
}

fn parse_args() -> Options {
    let mut options = Options::default();
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        usage();
    };
    if command != "compile" {
        usage();
    }
    while let Some(arg) = args.next() {
        let value =
            |args: &mut std::iter::Skip<std::env::Args>| args.next().unwrap_or_else(|| usage());
        match arg.as_str() {
            "--model-dir" => options.model_dir = Some(PathBuf::from(value(&mut args))),
            "--out-dir" => options.out_dir = Some(PathBuf::from(value(&mut args))),
            "--backend" => {
                options.backend = Backend::parse(&value(&mut args)).unwrap_or_else(|e| fail(&e));
            }
            "--mini" => options.mini = true,
            "--tables" => options.tables = parse_tables(&value(&mut args)),
            _ => usage(),
        }
    }
    if options.tables == Tables::default() {
        options.tables = Tables {
            system: true,
            addon: true,
            punct: true,
        };
    }
    options
}

fn fail(error: &DatagenError) -> ! {
    eprintln!("oxpinyin-datagen: {error}");
    std::process::exit(1);
}

fn resolve_model_dir(explicit: Option<&Path>) -> PathBuf {
    if let Some(dir) = explicit {
        return dir.to_path_buf();
    }
    match pinyin_oracle::model_cache::locate_model_dir() {
        Ok(Some(dir)) => dir,
        Ok(None) => {
            eprintln!(
                "oxpinyin-datagen: no model20 cache found (set --model-dir or run \
                 tools/model/fetch-model.sh)"
            );
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("oxpinyin-datagen: model dir unusable: {e:?}");
            std::process::exit(1);
        }
    }
}

fn write_table(
    backend: Backend,
    out_dir: &Path,
    base: &str,
    entries: &oxpinyin_datagen::Entries,
    manifest: &mut Vec<TableRecord>,
) {
    let path = backend.table_path(out_dir, base);
    backend.write(&path, entries).unwrap_or_else(|e| fail(&e));
    eprintln!(
        "  {base}.{}: {} records → {}",
        backend.extension(),
        entries.len(),
        path.display()
    );
    let record = Manifest::record_file(
        &path.file_name().unwrap().to_string_lossy(),
        &path,
        entries.len() as u64,
    )
    .unwrap_or_else(|e| fail(&e));
    manifest.push(record);
}

/// One libpinyin-schema output file (raw keyspace or hash container).
fn write_libpinyin_table(
    backend: Backend,
    out_dir: &Path,
    file_name: &str,
    entries: &oxpinyin_datagen::Entries,
    hash: bool,
    manifest: &mut Vec<TableRecord>,
) {
    let path = out_dir.join(file_name);
    if hash {
        backend
            .write_hash(&path, entries)
            .unwrap_or_else(|e| fail(&e));
    } else {
        backend
            .write_raw(&path, entries)
            .unwrap_or_else(|e| fail(&e));
    }
    eprintln!(
        "  {file_name}: {} records → {}",
        entries.len(),
        path.display()
    );
    let record = Manifest::record_file(
        &path.file_name().unwrap().to_string_lossy(),
        &path,
        entries.len() as u64,
    )
    .unwrap_or_else(|e| fail(&e));
    manifest.push(record);
}

/// One per-library chunk file (plain bytes, no store container).
fn write_chunk_file(
    out_dir: &Path,
    file_name: &str,
    bytes: &[u8],
    manifest: &mut Vec<TableRecord>,
) {
    let path = out_dir.join(file_name);
    std::fs::write(&path, bytes).unwrap_or_else(|e| fail(&DatagenError::Io(e)));
    eprintln!("  {file_name}: {} bytes → {}", bytes.len(), path.display());
    let record = Manifest::record_file(
        &path.file_name().unwrap().to_string_lossy(),
        &path,
        bytes.len() as u64,
    )
    .unwrap_or_else(|e| fail(&e));
    manifest.push(record);
}

/// The `table.conf` every libpinyin runtime expects in its data dir
/// (`pinyin_init` loads the lambda from it); emitted verbatim from the
/// pinned install's copy with the writing backend's `database format:`
/// token substituted.
fn write_table_conf(out_dir: &Path, backend: Backend, manifest: &mut Vec<TableRecord>) {
    let content = format!(
        "binary format version:7\n\
         model data version:14\n\
         lambda parameter:0.312699\n\
         \n\
         source table format:pinyin\n\
         database format:{format}\n\
         \n\
         default RESERVED NULL NULL NULL NOT_USED\n\
         default GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE\n\
         default GBK_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin SYSTEM_FILE\n\
         default OPENGRAM_DICTIONARY opengram.table opengram.bin opengram.dbin SYSTEM_FILE\n\
         default MERGED_DICTIONARY merged.table merged.bin merged.dbin SYSTEM_FILE\n\
         default ADDON_DICTIONARY NULL NULL addon.bin USER_FILE\n\
         default NETWORK_DICTIONARY NULL NULL network.bin USER_FILE\n\
         default USER_DICTIONARY NULL NULL user.bin USER_FILE\n\
         \n\
         addon 4 art.table art.bin NULL DICTIONARY\n\
         addon 5 culture.table culture.bin NULL DICTIONARY\n\
         addon 6 economy.table economy.bin NULL DICTIONARY\n\
         addon 7 geology.table geology.bin NULL DICTIONARY\n\
         addon 8 history.table history.bin NULL DICTIONARY\n\
         \n\
         addon 9 life.table life.bin NULL DICTIONARY\n\
         addon 10 nature.table nature.bin NULL DICTIONARY\n\
         addon 11 people.table people.bin NULL DICTIONARY\n\
         addon 12 science.table science.bin NULL DICTIONARY\n\
         addon 13 society.table society.bin NULL DICTIONARY\n\
         addon 14 sport.table sport.bin NULL DICTIONARY\n\
         addon 15 technology.table technology.bin NULL DICTIONARY\n",
        format = database_format_token(backend),
    );
    let path = out_dir.join("table.conf");
    std::fs::write(&path, content).unwrap_or_else(|e| fail(&DatagenError::Io(e)));
    eprintln!("  table.conf → {}", path.display());
    let record = Manifest::record_file(&path.file_name().unwrap().to_string_lossy(), &path, 1)
        .unwrap_or_else(|e| fail(&e));
    manifest.push(record);
}

/// The `--tables system` half: the three compiled system tables plus the
/// engine's `interpolation2.text` copy.
fn compile_system(
    backend: Backend,
    mini: bool,
    model_dir: &Path,
    out_dir: &Path,
    manifest: &mut Vec<TableRecord>,
) {
    let subset = if mini {
        system::Subset::MiniFixture
    } else {
        system::Subset::Full
    };
    // The engine's system dir consumes interpolation2.text directly on
    // every backend.
    let target = out_dir.join("interpolation2.text");
    std::fs::copy(model_dir.join("interpolation2.text"), &target)
        .unwrap_or_else(|e| fail(&DatagenError::Io(e)));
    eprintln!("  interpolation2.text → {}", target.display());

    if backend.emits_libpinyin_schema() {
        let out = system::compile_libpinyin(model_dir, subset).unwrap_or_else(|e| fail(&e));
        eprintln!(
            "  system: {} chunk files · {} pinyin rows · {} phrase rows · {} bigram rows",
            out.chunks.len(),
            out.pinyin_index.len(),
            out.phrase_index.len(),
            out.bigram.len(),
        );
        for (file_name, bytes) in &out.chunks {
            write_chunk_file(out_dir, file_name, bytes, manifest);
        }
        write_libpinyin_table(
            backend,
            out_dir,
            "pinyin_index.bin",
            &out.pinyin_index,
            false,
            manifest,
        );
        write_libpinyin_table(
            backend,
            out_dir,
            "phrase_index.bin",
            &out.phrase_index,
            false,
            manifest,
        );
        write_libpinyin_table(backend, out_dir, "bigram.db", &out.bigram, true, manifest);
        return;
    }

    let (tables, stats) = system::compile(model_dir, subset).unwrap_or_else(|e| fail(&e));
    eprintln!(
        "  system: rows {:+?} · {} index keys · {} phrases · {} bigram entries \
         ({} records, {} special tokens)",
        stats.library_rows,
        stats.index_keys,
        stats.phrases,
        stats.bigram_entries,
        stats.bigram_records,
        stats.special_tokens
    );
    write_table(
        backend,
        out_dir,
        "pinyin_index",
        &tables.pinyin_index,
        manifest,
    );
    write_table(
        backend,
        out_dir,
        "phrase_index",
        &tables.phrase_index,
        manifest,
    );
    write_table(backend, out_dir, "bigram", &tables.bigram, manifest);
}

/// The `--tables addon` half: every add-on library's two tables.
fn compile_addon(
    backend: Backend,
    mini: bool,
    model_dir: &Path,
    out_dir: &Path,
    manifest: &mut Vec<TableRecord>,
) {
    let subset = if mini {
        addon::Subset::MiniFixture
    } else {
        addon::Subset::Full
    };
    if backend.emits_libpinyin_schema() {
        // Upstream's second generate_binary_files run: one merged DBM
        // pair over all twelve libraries plus one chunk file each.
        let out = addon::compile_libpinyin(model_dir, subset).unwrap_or_else(|e| fail(&e));
        eprintln!(
            "  addon: {} chunk files · {} pinyin rows · {} phrase rows",
            out.chunks.len(),
            out.pinyin_index.len(),
            out.phrase_index.len(),
        );
        for (file_name, bytes) in &out.chunks {
            write_chunk_file(out_dir, file_name, bytes, manifest);
        }
        write_libpinyin_table(
            backend,
            out_dir,
            "addon_pinyin_index.bin",
            &out.pinyin_index,
            false,
            manifest,
        );
        write_libpinyin_table(
            backend,
            out_dir,
            "addon_phrase_index.bin",
            &out.phrase_index,
            false,
            manifest,
        );
        return;
    }
    let libraries = addon::compile(model_dir, subset).unwrap_or_else(|e| fail(&e));
    for library in &libraries {
        write_table(
            backend,
            out_dir,
            &format!("addon_{}_pinyin_index", library.index),
            &library.pinyin_index,
            manifest,
        );
        write_table(
            backend,
            out_dir,
            &format!("addon_{}_phrase_index", library.index),
            &library.phrase_index,
            manifest,
        );
    }
}

/// The `--tables punct` half: the full punctuation table (no mini variant;
/// the frozen fixtures hold the full table).
fn compile_punct(
    backend: Backend,
    model_dir: &Path,
    out_dir: &Path,
    manifest: &mut Vec<TableRecord>,
) {
    if backend.emits_libpinyin_schema() {
        let entries = punct::compile_libpinyin(model_dir).unwrap_or_else(|e| fail(&e));
        eprintln!("  punct: {} tokens", entries.len());
        write_libpinyin_table(backend, out_dir, "punct.bin", &entries, false, manifest);
        return;
    }
    let entries = punct::compile(model_dir).unwrap_or_else(|e| fail(&e));
    eprintln!("  punct: {} tokens", entries.len());
    write_table(backend, out_dir, "punct", &entries, manifest);
}

fn main() -> ExitCode {
    let options = parse_args();
    if !options.backend.available() {
        fail(&DatagenError::Consistency(format!(
            "backend {:?} requires rebuilding with --features {}",
            options.backend,
            options.backend.feature()
        )));
    }
    let model_dir = resolve_model_dir(options.model_dir.as_deref());
    let out_dir = options
        .out_dir
        .unwrap_or_else(|| PathBuf::from("target/datagen").join(options.backend.extension()));
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        fail(&DatagenError::Io(e));
    }
    eprintln!(
        "compiling model20 from {} → {} (backend {}, mini={})",
        model_dir.display(),
        out_dir.display(),
        options.backend.extension(),
        options.mini
    );

    let mut manifest = Vec::new();

    if options.tables.system {
        compile_system(
            options.backend,
            options.mini,
            &model_dir,
            &out_dir,
            &mut manifest,
        );
    }

    if options.tables.addon {
        compile_addon(
            options.backend,
            options.mini,
            &model_dir,
            &out_dir,
            &mut manifest,
        );
    }

    if options.tables.punct {
        compile_punct(options.backend, &model_dir, &out_dir, &mut manifest);
    }

    if options.backend.emits_libpinyin_schema() {
        write_table_conf(&out_dir, options.backend, &mut manifest);
    }

    let manifest = Manifest {
        backend: options.backend,
        model_sha256: pinyin_oracle::model_cache::MODEL20_SHA256.to_owned(),
        producer_version: env!("CARGO_PKG_VERSION").to_owned(),
        tables: manifest,
    };
    manifest.write_to_dir(&out_dir).unwrap_or_else(|e| fail(&e));
    eprintln!(
        "wrote {} → {}",
        oxpinyin_datagen::manifest::MANIFEST_FILE,
        out_dir.display()
    );
    ExitCode::SUCCESS
}
