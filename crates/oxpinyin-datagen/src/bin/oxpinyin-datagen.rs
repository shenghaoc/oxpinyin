//! `oxpinyin-datagen` — compile the pinned model20 text into oxpinyin
//! runtime tables for one storage backend.
//!
//! ```text
//! oxpinyin-datagen compile [--model-dir DIR] [--out-dir DIR]
//!                          [--backend redb|lmdb|tkrzw] [--mini]
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

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use oxpinyin_datagen::manifest::{Manifest, TableRecord};
use oxpinyin_datagen::write::Backend;
use oxpinyin_datagen::{DatagenError, addon, punct, system};

#[derive(Debug)]
struct Options {
    model_dir: Option<PathBuf>,
    out_dir: Option<PathBuf>,
    backend: Backend,
    mini: bool,
    tables: Tables,
}

impl Default for Options {
    /// Creates compilation options with the redb backend, all table categories enabled, and mini mode disabled.
    ///
    /// # Examples
    ///
    /// ```
    /// let options = Options::default();
    /// assert!(!options.mini);
    /// ```
    fn default() -> Self {
        Self {
            model_dir: None,
            out_dir: None,
            backend: Backend::Redb,
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

/// Prints the command-line usage message and exits with status code 2.
///
/// # Examples
///
/// ```no_run
/// usage();
/// ```
fn usage() -> ! {
    eprintln!(
        "usage: oxpinyin-datagen compile [--model-dir DIR] [--out-dir DIR] \
         [--backend redb|lmdb|tkrzw] [--mini] [--tables system,addon,punct]"
    );
    std::process::exit(2);
}

/// Parses a comma-separated list of table names into table-selection flags.
///
/// # Examples
///
/// ```
/// let tables = parse_tables("system, punct");
/// assert!(tables.system);
/// assert!(!tables.addon);
/// assert!(tables.punct);
/// ```
///
/// Unknown table names terminate the process with a usage error.
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

/// Parses the `compile` command-line arguments into compilation options.
///
/// When no table selection is provided, system, addon, and punctuation tables are enabled.
/// Invalid commands or options terminate the process after displaying usage information.
///
/// # Examples
///
/// ```no_run
/// let options = parse_args();
/// assert!(options.tables.system);
/// ```
fn parse_args() -> Options
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
                options.backend = Backend::parse(&value(&mut args)).unwrap_or_else(|e| fail(e));
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

/// Reports a data-generation error and terminates the process with status code 1.
///
/// # Examples
///
/// ```no_run
/// # fn example(error: DatagenError) {
/// fail(error);
/// # }
/// ```
fn fail(error: DatagenError) -> ! {
    eprintln!("oxpinyin-datagen: {error}");
    std::process::exit(1);
}

/// Resolves the model directory from an explicit path or the configured model cache.
///
/// # Parameters
///
/// * `explicit` - An optional model directory to use instead of cache discovery.
///
/// # Returns
///
/// The resolved model directory.
///
/// Exits the process if no usable model directory can be found.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// let dir = resolve_model_dir(Some(Path::new("models")));
/// assert_eq!(dir, Path::new("models"));
/// ```
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

/// Writes compiled entries to a backend-specific table file and records the file in the manifest.
///
/// # Examples
///
/// ```no_run
/// // Use a compiled entry set to create a table and update its manifest.
/// write_table(backend, out_dir, "system", &entries, &mut manifest);
/// ```
///
/// # Parameters
///
/// * `base` — Base name used to derive the table path.
/// * `entries` — Compiled records to write.
/// * `manifest` — Manifest records to which the written table is appended.
fn write_table(
    backend: Backend,
    out_dir: &Path,
    base: &str,
    entries: &oxpinyin_datagen::Entries,
    manifest: &mut Vec<TableRecord>,
) {
    let path = backend.table_path(out_dir, base);
    backend.write(&path, entries).unwrap_or_else(|e| fail(e));
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
    .unwrap_or_else(|e| fail(e));
    manifest.push(record);
}

/// Compiles selected model20 data into runtime tables and writes a manifest.
///
/// # Examples
///
/// ```no_run
/// let status = main();
/// assert_eq!(status, std::process::ExitCode::SUCCESS);
/// ```
///
/// # Returns
///
/// [`ExitCode::SUCCESS`] after all selected tables and manifest data are written.
fn main() -> ExitCode {
    let options = parse_args();
    if !options.backend.available() {
        fail(DatagenError::Consistency(format!(
            "backend {:?} requires rebuilding with --features {}",
            options.backend,
            options.backend.extension()
        )));
    }
    let model_dir = resolve_model_dir(options.model_dir.as_deref());
    let out_dir = options
        .out_dir
        .unwrap_or_else(|| PathBuf::from("target/datagen").join(options.backend.extension()));
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        fail(DatagenError::Io(e));
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
        let subset = if options.mini {
            system::Subset::MiniFixture
        } else {
            system::Subset::Full
        };
        let (tables, stats) = system::compile(&model_dir, subset).unwrap_or_else(|e| fail(e));
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
            options.backend,
            &out_dir,
            "pinyin_index",
            &tables.pinyin_index,
            &mut manifest,
        );
        write_table(
            options.backend,
            &out_dir,
            "phrase_index",
            &tables.phrase_index,
            &mut manifest,
        );
        write_table(
            options.backend,
            &out_dir,
            "bigram",
            &tables.bigram,
            &mut manifest,
        );
        // The engine's system dir consumes interpolation2.text directly.
        let target = out_dir.join("interpolation2.text");
        std::fs::copy(model_dir.join("interpolation2.text"), &target).unwrap_or_else(|e| {
            fail(DatagenError::Io(e));
        });
        eprintln!("  interpolation2.text → {}", target.display());
    }

    if options.tables.addon {
        let subset = if options.mini {
            addon::Subset::MiniFixture
        } else {
            addon::Subset::Full
        };
        let libraries = addon::compile(&model_dir, subset).unwrap_or_else(|e| fail(e));
        for library in &libraries {
            write_table(
                options.backend,
                &out_dir,
                &format!("addon_{}_pinyin_index", library.index),
                &library.pinyin_index,
                &mut manifest,
            );
            write_table(
                options.backend,
                &out_dir,
                &format!("addon_{}_phrase_index", library.index),
                &library.phrase_index,
                &mut manifest,
            );
        }
    }

    if options.tables.punct {
        // The frozen fixtures hold the full punct table; no mini variant.
        let entries = punct::compile(&model_dir).unwrap_or_else(|e| fail(e));
        eprintln!("  punct: {} tokens", entries.len());
        write_table(options.backend, &out_dir, "punct", &entries, &mut manifest);
    }

    let manifest = Manifest {
        backend: options.backend,
        model_sha256: pinyin_oracle::model_cache::MODEL20_SHA256.to_owned(),
        producer_version: env!("CARGO_PKG_VERSION").to_owned(),
        tables: manifest,
    };
    manifest.write_to_dir(&out_dir).unwrap_or_else(|e| fail(e));
    eprintln!(
        "wrote {} → {}",
        oxpinyin_datagen::manifest::MANIFEST_FILE,
        out_dir.display()
    );
    ExitCode::SUCCESS
}
