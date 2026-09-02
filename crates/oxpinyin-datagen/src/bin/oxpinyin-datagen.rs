//! `oxpinyin-datagen` — compile the pinned model20 text into a system data
//! directory for one storage backend.
//!
//! ```text
//! oxpinyin-datagen compile [--model-dir DIR] [--out-dir DIR]
//!                          [--backend redb|lmdb|tkrzw|kyotocabinet] [--mini]
//!                          [--tables system,addon,punct]
//! ```
//!
//! The output is the data directory libpinyin's own build produces —
//! the sixteen per-library chunk files, `pinyin_index.bin`,
//! `phrase_index.bin`, `bigram.db`, `punct.bin`, the `addon_*` pair, and
//! `table.conf` — written through the selected backend. On Kyoto Cabinet
//! and tkrzw the files carry libpinyin's names and are its drop-in set;
//! on redb and LMDB the same records live in that backend's container
//! under `<stem>.<ext>`. The chunk files are backend-independent.
//!
//! The model directory is discovered exactly as the differential harness
//! discovers it (`PINYIN_MODEL_DIR`, `PINYIN_MODEL_CACHE/extracted`, the
//! workspace `target/model20/extracted` cache); run
//! `tools/model/fetch-model.sh` first. The output directory receives the
//! tables, the `\1-gram` section of `interpolation2.text` (the only
//! section the engine reads at runtime), and `datagen-manifest.txt` with
//! the run's provenance.
//!
//! `--mini` reproduces the committed `fixtures/w3/<backend>/` subset — the
//! regression recipe, not a shipping path.

#![forbid(unsafe_code)]
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use oxpinyin_datagen::manifest::{Manifest, TableRecord};
use oxpinyin_datagen::write::{Backend, DbmFile};
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

fn record(path: &Path, rows: u64, manifest: &mut Vec<TableRecord>) {
    let record = Manifest::record_file(&path.file_name().unwrap().to_string_lossy(), path, rows)
        .unwrap_or_else(|e| fail(&e));
    manifest.push(record);
}

fn write_interpolation2_1gram(src: &Path, dst: &Path) -> Result<(), DatagenError> {
    // `--out-dir` pointed at the model dir would make dst alias src, and
    // creating dst would truncate the source mid-read.
    let src_canon = std::fs::canonicalize(src).ok();
    let dst_canon = std::fs::canonicalize(dst).ok();
    if src == dst || (src_canon.is_some() && src_canon == dst_canon) {
        return Err(DatagenError::Consistency(format!(
            "interpolation2.text source and destination are the same file: {}",
            src.display()
        )));
    }
    // Write to a sibling temp file and rename into place only on success, so
    // a failed extraction leaves an existing dst untouched.
    let mut tmp = dst.as_os_str().to_os_string();
    tmp.push(format!(".{}.tmp", std::process::id()));
    let tmp = PathBuf::from(tmp);

    let extracted = (|| -> Result<(), DatagenError> {
        let file = std::fs::File::open(src).map_err(DatagenError::Io)?;
        let mut reader = BufReader::new(file);
        let out = std::fs::File::create(&tmp).map_err(DatagenError::Io)?;
        let mut writer = BufWriter::new(out);
        let mut buf = String::new();
        let mut seen_1gram = false;

        loop {
            buf.clear();
            let n = reader.read_line(&mut buf).map_err(DatagenError::Io)?;
            if n == 0 {
                break;
            }
            let trimmed = buf.trim_end_matches(['\r', '\n']);

            if !seen_1gram {
                writer.write_all(buf.as_bytes()).map_err(DatagenError::Io)?;
                if trimmed == "\\1-gram" {
                    seen_1gram = true;
                }
                continue;
            }
            if trimmed.starts_with('\\') && !trimmed.starts_with("\\item") {
                break;
            }
            writer.write_all(buf.as_bytes()).map_err(DatagenError::Io)?;
        }

        if !seen_1gram {
            return Err(DatagenError::Consistency(
                "no \\1-gram section found".into(),
            ));
        }
        writer.write_all(b"\\end\n").map_err(DatagenError::Io)?;
        writer.flush().map_err(DatagenError::Io)?;
        Ok(())
    })();

    match extracted {
        Ok(()) => std::fs::rename(&tmp, dst).map_err(DatagenError::Io),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// One DBM file: the raw keyspace of a tree container or the hash
/// container; every row read back before it is reported.
fn write_dbm(
    backend: Backend,
    out_dir: &Path,
    dbm: DbmFile,
    entries: &oxpinyin_datagen::Entries,
    manifest: &mut Vec<TableRecord>,
) {
    let path = backend
        .write_dbm(out_dir, dbm, entries)
        .unwrap_or_else(|e| fail(&e));
    eprintln!(
        "  {}: {} records → {}",
        backend.dbm_file_name(dbm),
        entries.len(),
        path.display()
    );
    record(&path, entries.len() as u64, manifest);
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
    record(&path, bytes.len() as u64, manifest);
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
        format = backend.database_format_token(),
    );
    let path = out_dir.join("table.conf");
    std::fs::write(&path, content).unwrap_or_else(|e| fail(&DatagenError::Io(e)));
    eprintln!("  table.conf → {}", path.display());
    record(&path, 1, manifest);
}

/// The `--tables system` half: the chunk files, the two index DBMs,
/// the bigram, and the `\1-gram` section of `interpolation2.text`.
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
    let (out, stats) = system::compile(model_dir, subset).unwrap_or_else(|e| fail(&e));
    eprintln!(
        "  system: rows {:+?} · {} spellings · {} phrases · {} bigram entries \
         ({} records, {} special tokens)",
        stats.library_rows,
        stats.index_keys,
        stats.phrases,
        stats.bigram_entries,
        stats.bigram_records,
        stats.special_tokens
    );
    for (file_name, bytes) in &out.chunks {
        write_chunk_file(out_dir, file_name, bytes, manifest);
    }
    write_dbm(
        backend,
        out_dir,
        DbmFile::PinyinIndex,
        &out.pinyin_index,
        manifest,
    );
    write_dbm(
        backend,
        out_dir,
        DbmFile::PhraseIndex,
        &out.phrase_index,
        manifest,
    );
    write_dbm(backend, out_dir, DbmFile::Bigram, &out.bigram, manifest);
    // The engine reads only the \1-gram section at runtime; emit that section only.
    let target = out_dir.join("interpolation2.text");
    write_interpolation2_1gram(&model_dir.join("interpolation2.text"), &target)
        .unwrap_or_else(|e| fail(&e));
    eprintln!("  interpolation2.text (1-gram only) → {}", target.display());
}

/// The `--tables addon` half: twelve chunk files and the merged addon DBM
/// pair (upstream's second `generate_binary_files` run).
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
    let out = addon::compile(model_dir, subset).unwrap_or_else(|e| fail(&e));
    eprintln!(
        "  addon: {} chunk files · {} pinyin rows · {} phrase rows",
        out.chunks.len(),
        out.pinyin_index.len(),
        out.phrase_index.len(),
    );
    for (file_name, bytes) in &out.chunks {
        write_chunk_file(out_dir, file_name, bytes, manifest);
    }
    write_dbm(
        backend,
        out_dir,
        DbmFile::AddonPinyinIndex,
        &out.pinyin_index,
        manifest,
    );
    write_dbm(
        backend,
        out_dir,
        DbmFile::AddonPhraseIndex,
        &out.phrase_index,
        manifest,
    );
}

/// The `--tables punct` half: the full punctuation table (no mini variant).
fn compile_punct(
    backend: Backend,
    model_dir: &Path,
    out_dir: &Path,
    manifest: &mut Vec<TableRecord>,
) {
    let entries = punct::compile(model_dir).unwrap_or_else(|e| fail(&e));
    eprintln!("  punct: {} tokens", entries.len());
    write_dbm(backend, out_dir, DbmFile::Punct, &entries, manifest);
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
        options.backend.feature(),
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

    write_table_conf(&out_dir, options.backend, &mut manifest);

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

#[cfg(test)]
mod tests {
    use super::write_interpolation2_1gram;

    /// A fresh per-test scratch dir; this workspace keeps no tempfile dep.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oxpinyin-datagen-bin-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn tmp_litter(dir: &std::path::Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .expect("read dir")
            .filter_map(|e| {
                let name = e.expect("dir entry").file_name().into_string().ok()?;
                name.ends_with(".tmp").then_some(name)
            })
            .collect()
    }

    #[test]
    fn emits_1gram_section_only_and_leaves_no_temp_file() {
        let dir = scratch("emits");
        let src = dir.join("src.text");
        std::fs::write(
            &src,
            "\\created_by\tmarisa\n\\1-gram\n\\item\ta 1.0\n\\2-gram\n\\item\tx 2.0\n",
        )
        .expect("write src");
        let dst = dir.join("interpolation2.text");

        write_interpolation2_1gram(&src, &dst).expect("extract");

        assert_eq!(
            std::fs::read_to_string(&dst).expect("read dst"),
            "\\created_by\tmarisa\n\\1-gram\n\\item\ta 1.0\n\\end\n"
        );
        assert!(tmp_litter(&dir).is_empty());
    }

    #[test]
    fn missing_1gram_section_leaves_destination_intact() {
        let dir = scratch("missing");
        let src = dir.join("src.text");
        std::fs::write(&src, "\\created_by\tmarisa\n").expect("write src");
        let dst = dir.join("interpolation2.text");
        std::fs::write(&dst, "previous output\n").expect("write dst");

        let err = write_interpolation2_1gram(&src, &dst).expect_err("no 1-gram section");

        assert!(
            matches!(err, super::DatagenError::Consistency(ref msg) if msg.contains("1-gram")),
            "unexpected error: {err:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&dst).expect("read dst"),
            "previous output\n"
        );
        assert!(tmp_litter(&dir).is_empty());
    }

    #[test]
    fn aliased_source_and_destination_rejected() {
        let dir = scratch("alias");
        let src = dir.join("interpolation2.text");
        std::fs::write(&src, "\\1-gram\n").expect("write src");

        let err = write_interpolation2_1gram(&src, &src).expect_err("src and dst are one file");

        assert!(
            matches!(err, super::DatagenError::Consistency(ref msg) if msg.contains("same file")),
            "unexpected error: {err:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&src).expect("read src"),
            "\\1-gram\n"
        );
        assert!(tmp_litter(&dir).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_destination_alias_rejected() {
        let dir = scratch("symlink-alias");
        let src = dir.join("src.text");
        std::fs::write(&src, "\\1-gram\n").expect("write src");
        let link = dir.join("interpolation2.text");
        std::os::unix::fs::symlink(&src, &link).expect("symlink");

        let err = write_interpolation2_1gram(&src, &link).expect_err("dst resolves to src");

        assert!(
            matches!(err, super::DatagenError::Consistency(_)),
            "unexpected error: {err:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&src).expect("read src"),
            "\\1-gram\n"
        );
    }
}
