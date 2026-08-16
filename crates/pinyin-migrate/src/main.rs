//! Converter and exporter: oracle data → portable redb tables.
//!
//! Two commands, per `docs/findings/data-layer-export.md`:
//!
//! - `convert <input.tkh> [-o <output.redb>] [-n <limit>]` — verbatim
//!   record-for-record copy of a Tkrzw HashDBM into a redb `data` table.
//!   Used for the system bigram and for ad-hoc inspection.
//! - `export --out-dir <dir> [--mini]` — full public-ABI export of the
//!   four system phrase libraries plus the bigram copy. Requires the
//!   `oracle-ffi` feature and the pin-built oracle (Linux-first).
//! - `migrate --legacy-dir <dir> --user-dir <dir>` — read a legacy libpinyin
//!   user directory (`user.bin` + `user_bigram.db`) through libpinyin's own
//!   storage classes and import its phrases, pronunciations, unigrams and
//!   bigrams into the pinyin-rs user redb store. Read-only on the legacy
//!   dir; idempotent (a monotonic floor over the existing store). Requires
//!   `oracle-ffi` and the pin-built oracle.

#![warn(missing_docs)]

mod tkrzw;

#[cfg(feature = "oracle-ffi")]
mod export;

#[cfg(feature = "oracle-ffi")]
mod storage_dump;

use std::fs;
use std::path::{Path, PathBuf};

const REDB_TABLE: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new("data");

/// Writes `entries` (already ordered) into a fresh redb at `path`.
fn write_redb(
    path: &Path,
    entries: &[(Vec<u8>, Vec<u8>)],
) -> Result<(), Box<dyn std::error::Error>> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    let db = redb::Database::create(path)?;
    {
        let txn = db.begin_write()?;
        {
            let mut table = txn.open_table(REDB_TABLE)?;
            for (key, value) in entries {
                table.insert(key.as_slice(), value.as_slice())?;
            }
        }
        txn.commit()?;
    }
    let out_size = fs::metadata(path)?.len();
    eprintln!(
        "wrote {} records ({out_size} bytes) → {}",
        entries.len(),
        path.display()
    );
    Ok(())
}

fn usage() -> ! {
    eprintln!(
        "Usage:\n  pinyin-migrate convert <input.tkh> [-o <output.redb>] [-n <limit>]\n  \
         pinyin-migrate export --out-dir <dir> [--mini]   (requires --features oracle-ffi)\n  \
         pinyin-migrate migrate --legacy-dir <dir> --user-dir <dir>   (requires --features oracle-ffi)"
    );
    std::process::exit(2);
}

fn convert(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut limit: Option<usize> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                i += 1;
                output = Some(PathBuf::from(args.get(i).unwrap_or_else(|| usage())));
            }
            "-n" => {
                i += 1;
                limit = Some(
                    args.get(i)
                        .unwrap_or_else(|| usage())
                        .parse()
                        .map_err(|_| "invalid -n value")?,
                );
            }
            arg if !arg.starts_with('-') => {
                input = Some(PathBuf::from(arg));
            }
            _ => usage(),
        }
        i += 1;
    }

    let input = input.unwrap_or_else(|| usage());
    let output = output.unwrap_or_else(|| {
        let mut path = input.clone();
        path.set_extension("redb");
        path
    });

    let path_c = std::ffi::CString::new(input.to_string_lossy().as_bytes())
        .map_err(|_| "input path contains a NUL byte")?;
    let reader = tkrzw::TkrzwReader::open(&path_c)?;

    let mut entries = reader.entries()?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    if let Some(n) = limit {
        entries.truncate(n);
    }

    write_redb(&output, &entries)
}

#[cfg(feature = "oracle-ffi")]
fn export(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut out_dir: Option<PathBuf> = None;
    let mut mini = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--out-dir" => {
                i += 1;
                out_dir = Some(PathBuf::from(args.get(i).unwrap_or_else(|| usage())));
            }
            "--mini" => mini = true,
            _ => usage(),
        }
        i += 1;
    }

    export::run(&out_dir.unwrap_or_else(|| usage()), mini)
}

#[cfg(not(feature = "oracle-ffi"))]
fn export(_args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    Err(
        "`export` requires building with `--features oracle-ffi` on Linux \
         with the pin-built oracle installed"
            .into(),
    )
}

#[cfg(feature = "oracle-ffi")]
fn migrate(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let mut legacy_dir: Option<PathBuf> = None;
    let mut user_dir: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--legacy-dir" => {
                i += 1;
                legacy_dir = Some(PathBuf::from(args.get(i).unwrap_or_else(|| usage())));
            }
            "--user-dir" => {
                i += 1;
                user_dir = Some(PathBuf::from(args.get(i).unwrap_or_else(|| usage())));
            }
            _ => usage(),
        }
        i += 1;
    }

    let legacy_dir = legacy_dir.unwrap_or_else(|| usage());
    let user_dir = user_dir.unwrap_or_else(|| usage());

    // Read the legacy dir first, before any write to the user store, so a
    // malformed legacy dir never touches the destination.
    let dump = storage_dump::dump(&legacy_dir)?;
    let (migration, stats) = to_migration(&dump);

    fs::create_dir_all(&user_dir)?;
    let store_path = user_dir.join("user_store.redb");
    let mut store = pinyin_user::UserStore::open(&store_path)?;
    store.apply_migration(&migration)?;

    eprintln!(
        "{} tokens iterated, {} null slots skipped, {} corrupt records skipped, \
         {} phrases migrated, {} pronunciations migrated, {} tone-stripped, {} keys unmapped",
        dump.tokens_iterated,
        dump.null_slot_count,
        dump.corrupt_count,
        stats.phrases_migrated,
        stats.pronunciations_migrated,
        stats.tone_stripped,
        stats.keys_unmapped,
    );
    Ok(())
}

#[cfg(not(feature = "oracle-ffi"))]
fn migrate(_args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    Err(
        "`migrate` requires building with `--features oracle-ffi` on Linux \
         with the pin-built oracle installed"
            .into(),
    )
}

/// Per-syllable mapping counters, reported on `stderr` after a migration so
/// the tone-strip and null-slot arithmetic is auditable.
#[cfg(feature = "oracle-ffi")]
struct MigrationStats {
    phrases_migrated: usize,
    pronunciations_migrated: usize,
    tone_stripped: usize,
    keys_unmapped: usize,
}

/// Strips exactly one trailing tone digit (`1`..`5`) from a libpinyin
/// `get_pinyin_string()` output. Zero-tone syllables carry no digit and are
/// returned unchanged; pinyin spellings never end in an ASCII digit, so the
/// strip is unambiguous.
#[cfg(feature = "oracle-ffi")]
fn strip_tone(syllable: &str) -> &str {
    if let Some(last) = syllable.chars().last()
        && ('1'..='5').contains(&last)
    {
        return &syllable[..syllable.len() - last.len_utf8()];
    }
    syllable
}

/// Maps a legacy pronunciation's per-syllable pinyin strings to dense
/// [`pinyin_user::PinyinKey`]s. A pronunciation whose syllables all map is
/// returned; one with any unmappable syllable (a reading the frozen inventory
/// cannot render) is dropped and counted, keeping the phrase but not the
/// unrepresentable reading.
#[cfg(feature = "oracle-ffi")]
fn map_keys(syllables: &[String], stats: &mut MigrationStats) -> Option<Vec<pinyin_user::PinyinKey>> {
    let mut keys = Vec::with_capacity(syllables.len());
    for syllable in syllables {
        let stripped = strip_tone(syllable);
        if stripped.len() != syllable.len() {
            stats.tone_stripped += 1;
        }
        match pinyin_core::SyllableKey::from_text(stripped) {
            Some(key) => keys.push(key.index() as pinyin_user::PinyinKey),
            None => {
                stats.keys_unmapped += 1;
                return None;
            }
        }
    }
    Some(keys)
}

/// Converts a parsed [`storage_dump::LegacyDump`] into the bulk-write input,
/// tone-stripping and mapping each reading. Unigrams, bigram totals and
/// successor counts are widened `u32` → `u64` verbatim; the bigram `total` is
/// carried for the record but recomputed from successors by `apply_migration`.
#[cfg(feature = "oracle-ffi")]
fn to_migration(dump: &storage_dump::LegacyDump) -> (pinyin_user::MigrationDump, MigrationStats) {
    let mut stats = MigrationStats {
        phrases_migrated: 0,
        pronunciations_migrated: 0,
        tone_stripped: 0,
        keys_unmapped: 0,
    };

    let mut phrases = Vec::with_capacity(dump.phrases.len());
    for p in &dump.phrases {
        let mut pronunciations = Vec::new();
        for pron in &p.pronunciations {
            if let Some(keys) = map_keys(&pron.syllables, &mut stats) {
                pronunciations.push(pinyin_user::MigrationPronunciation {
                    keys,
                    count: u64::from(pron.freq),
                });
            }
        }
        stats.pronunciations_migrated += pronunciations.len();
        phrases.push(pinyin_user::MigrationPhrase {
            token: p.token,
            text: p.text.clone(),
            unigram: u64::from(p.unigram_freq),
            pronunciations,
        });
        stats.phrases_migrated += 1;
    }

    let bigrams = dump
        .bigrams
        .iter()
        .map(|b| pinyin_user::MigrationBigram {
            prev: b.prev,
            total: u64::from(b.total_freq),
            successors: b
                .successors
                .iter()
                .map(|&(cur, count)| (cur, u64::from(count)))
                .collect(),
        })
        .collect();

    (pinyin_user::MigrationDump { phrases, bigrams }, stats)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("convert") => convert(&args[2..]),
        Some("export") => export(&args[2..]),
        Some("migrate") => migrate(&args[2..]),
        // The bare form `pinyin-migrate <input.tkh> [-o …] [-n …]` is the
        // invocation frozen in docs/findings/data-formats.md §1.1; keep it
        // as an alias for `convert`.
        Some(first) if !first.starts_with('-') => convert(&args[1..]),
        _ => usage(),
    }
}
