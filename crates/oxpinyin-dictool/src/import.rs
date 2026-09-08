//! `oxpinyin-dictool import`: text → user-store add batch → save.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::os::raw::c_int;
use std::path::{Path, PathBuf};

use oxpinyin_core::graph::FewestKeys;
use oxpinyin_user::{DEFAULT_PHRASE_COUNT, ExportedPhrase, PinyinKey, USER_DICTIONARY, UserStore};

use crate::context::UserImportContext;

use crate::format::ParseError;

/// Largest count the format accepts: `gint`'s positive range on the ABI.
pub const MAX_COUNT: u64 = c_int::MAX as u64;

/// A user-vocabulary import run.
#[derive(Debug)]
pub enum ImportError {
    /// The input file could not be read as UTF-8 text.
    Read(PathBuf, std::io::Error),
    /// The input file is not valid UTF-8 at the reported 1-based line.
    Utf8(PathBuf, usize),
    /// The text does not match the pinned format.
    Parse(ParseError),
    /// The user-store context could not be opened.
    Context(PathBuf, String),
    /// A parsed record was rejected by the add path.
    Add {
        /// 1-based line number in the input file.
        line: usize,
    },
    /// `pinyin_save` reported failure after the import batch.
    Save,
    /// The pre-import phrase snapshot could not be read.
    Snapshot,
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(path, error) => write!(f, "cannot read {}: {error}", path.display()),
            Self::Utf8(path, line) => {
                write!(f, "{} is not valid UTF-8 at line {line}", path.display())
            }
            Self::Parse(error) => write!(f, "{error}"),
            Self::Context(path, detail) => {
                write!(
                    f,
                    "cannot open user store under {}: {detail}",
                    path.display()
                )
            }
            Self::Add { line } => {
                write!(
                    f,
                    "line {line}: pinyin_iterator_add_phrase rejected the record"
                )
            }
            Self::Save => write!(f, "pinyin_save failed after the import batch"),
            Self::Snapshot => write!(f, "cannot read the existing phrase snapshot"),
        }
    }
}

impl std::error::Error for ImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read(_, error) => Some(error),
            Self::Parse(error) => Some(error),
            Self::Utf8(_, _)
            | Self::Context(_, _)
            | Self::Add { .. }
            | Self::Save
            | Self::Snapshot => None,
        }
    }
}

/// Read `path` as UTF-8 with a line number when decoding fails.
fn read_utf8(path: &Path) -> Result<String, ImportError> {
    let bytes = fs::read(path).map_err(|error| ImportError::Read(path.to_path_buf(), error))?;
    match String::from_utf8(bytes) {
        Ok(text) => Ok(text),
        Err(error) => {
            let valid = error.utf8_error().valid_up_to();
            let line = error.as_bytes()[..valid]
                .iter()
                .filter(|byte| **byte == b'\n')
                .count()
                + 1;
            Err(ImportError::Utf8(path.to_path_buf(), line))
        }
    }
}

/// One add through the store, with `pinyin_iterator_add_phrase`'s parse
/// selection: the frozen untuned full-pinyin inventory under the
/// longest-parsed-prefix then fewest-keys rule (`FewestKeys`), complete
/// keys only, trailing unparsed bytes ignored.
fn add_phrase(user: &mut UserStore, phrase: &str, pinyin: &str, count: u64) -> bool {
    let Some(parsed) = FewestKeys::parse(pinyin) else {
        return false;
    };
    let Some(keys) = parsed
        .keys()
        .iter()
        .map(|key| PinyinKey::try_from(key.index()).ok())
        .collect::<Option<Vec<PinyinKey>>>()
    else {
        return false;
    };
    user.add_phrase_in(USER_DICTIONARY, phrase, &keys, Some(count))
        .is_ok()
}

/// Import `path` into the user store under `user_dir`.
///
/// The directory is created when missing. Parsing is a full preflight, so a
/// malformed file performs no writes. Adds are per-phrase committed
/// (upstream `pinyin.cpp:614-653` semantics); if one somehow fails after a
/// valid parse, earlier adds remain, matching the source behaviour.
pub fn run(user_dir: &Path, path: &Path) -> Result<(), ImportError> {
    let text = read_utf8(path)?;
    let records = crate::format::parse(&text).map_err(ImportError::Parse)?;

    fs::create_dir_all(user_dir)
        .map_err(|error| ImportError::Read(user_dir.to_path_buf(), error))?;
    let mut context = UserImportContext::open(user_dir).ok_or_else(|| {
        ImportError::Context(
            user_dir.to_path_buf(),
            "the user store could not be opened".to_owned(),
        )
    })?;

    // File count is a desired absolute floor; the ABI count is an add
    // amount (`docs/findings/dictool-format.md` §3).
    let existing: HashMap<(String, String), u64> = context
        .core()
        .export_phrases(u32::from(USER_DICTIONARY))
        .ok_or(ImportError::Snapshot)?
        .into_iter()
        .map(
            |ExportedPhrase {
                 text,
                 pinyin,
                 count,
             }| ((text, pinyin), count),
        )
        .collect();

    let Some(user) = context.user() else {
        return Err(ImportError::Snapshot);
    };
    let mut first_error = None;
    for record in &records {
        let key = (record.phrase.clone(), record.pinyin.clone());
        let current = existing.get(&key).copied().unwrap_or(0);
        let desired = record.count.unwrap_or(DEFAULT_PHRASE_COUNT);
        // Desired count is a monotonic floor. Count 0 against a missing
        // row must still create the row (`atoi("0")`); a re-run is a no-op.
        let delta = if desired > current {
            desired - current
        } else if desired == 0 && !existing.contains_key(&key) {
            0
        } else {
            continue;
        };

        if !add_phrase(user, &record.phrase, &record.pinyin, delta) {
            first_error.get_or_insert(record.line);
        }
    }

    // `pinyin_end_add_phrases`' persistence side: the §4 dirty flag arms
    // whether or not any add succeeded (upstream compacts and sets
    // `m_modified` unconditionally, `pinyin.cpp:657-658`), so the gated
    // save below compacts even for an all-no-op re-run.
    user.mark_modified();
    let saved = context.core_mut().save_user();

    if let Some(line) = first_error {
        return Err(ImportError::Add { line });
    }
    if !saved {
        return Err(ImportError::Save);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("oxpinyin-dictool-{tag}-{}.d", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temp dir");
        path
    }

    fn write(file: &Path, text: &str) {
        fs::write(file, text).expect("write fixture");
    }

    fn exported(user_dir: &Path) -> Vec<ExportedPhrase> {
        let context = UserImportContext::open(user_dir).expect("reopen user store");
        context
            .core()
            .export_phrases(u32::from(USER_DICTIONARY))
            .expect("user phrase rows")
    }

    #[test]
    fn import_round_trips_through_export_and_is_idempotent() {
        let dir = temp_dir("roundtrip");
        let input = dir.join("input.txt");
        write(&input, "你好 ni'hao 3\n世界 shi'jie 7\n词 ci 1\n");

        run(&dir.join("user"), &input).expect("first import");
        let first = exported(&dir.join("user"));
        assert_eq!(
            first,
            vec![
                ExportedPhrase {
                    text: "你好".to_owned(),
                    pinyin: "ni'hao".to_owned(),
                    count: 3,
                },
                ExportedPhrase {
                    text: "世界".to_owned(),
                    pinyin: "shi'jie".to_owned(),
                    count: 7,
                },
                ExportedPhrase {
                    text: "词".to_owned(),
                    pinyin: "ci".to_owned(),
                    count: 1,
                },
            ]
        );
        // Re-running the same input is a no-op for every pronunciation
        // count: the CLI treats file counts as desired state, so 3/7/1 stay.
        run(&dir.join("user"), &input).expect("second import");
        assert_eq!(exported(&dir.join("user")), first);

        // Frontend-style export text -> import -> export is row-identical
        // modulo ordering (no bigrams were trained, so export has only
        // phrase rows; sorting the lines makes the comparison ordering-free).
        let export_path = dir.join("export.txt");
        crate::export::run(&dir.join("user"), Some(&export_path)).expect("export");
        let exported_lines = {
            let text = fs::read_to_string(&export_path).expect("read export");
            let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
            lines.sort();
            lines
        };
        assert_eq!(
            exported_lines,
            vec!["世界 shi'jie 7", "你好 ni'hao 3", "词 ci 1"]
        );
    }

    #[test]
    fn two_field_lines_floor_at_the_abi_default_count() {
        let dir = temp_dir("default-count");
        let input = dir.join("input.txt");
        write(&input, "词 ci\n");

        run(&dir.join("user"), &input).expect("first import");
        run(&dir.join("user"), &input).expect("second import is idempotent");

        assert_eq!(
            exported(&dir.join("user")),
            vec![ExportedPhrase {
                text: "词".to_owned(),
                pinyin: "ci".to_owned(),
                count: 5,
            }]
        );
    }

    #[test]
    fn import_raises_existing_counts_to_the_desired_floor() {
        let dir = temp_dir("floor");
        let first = dir.join("first.txt");
        let second = dir.join("second.txt");
        write(&first, "词 ci 5\n");
        write(&second, "词 ci 2\n");

        run(&dir.join("user"), &first).expect("first import");
        // Desired count is a monotonic floor: a lower target never deletes
        // or lowers a stored pronunciation count.
        run(&dir.join("user"), &second).expect("second import");

        assert_eq!(
            exported(&dir.join("user")),
            vec![ExportedPhrase {
                text: "词".to_owned(),
                pinyin: "ci".to_owned(),
                count: 5,
            }]
        );
    }

    #[test]
    fn malformed_file_reports_its_line_number() {
        let dir = temp_dir("malformed");
        let input = dir.join("bad.txt");
        write(&input, "# ok\n你好 ni'hao 3\n词 ci not-a-count\n");

        let error = run(&dir.join("user"), &input).unwrap_err();
        assert_eq!(
            error.to_string(),
            "line 3: count is not a decimal integer: \"not-a-count\""
        );
        // Preflight parsing wrote nothing.
        assert!(!dir.join("user").exists());
    }

    #[test]
    fn imported_phrase_with_same_text_and_new_reading_merges() {
        let dir = temp_dir("merge");
        let input = dir.join("merge.txt");
        write(&input, "行 xing 4\n行 hang 2\n");

        run(&dir.join("user"), &input).expect("import");
        assert_eq!(
            exported(&dir.join("user")),
            vec![
                ExportedPhrase {
                    text: "行".to_owned(),
                    pinyin: "xing".to_owned(),
                    count: 4,
                },
                ExportedPhrase {
                    text: "行".to_owned(),
                    pinyin: "hang".to_owned(),
                    count: 2,
                },
            ]
        );
    }

    #[test]
    fn unseparated_pinyin_is_the_same_pronunciation_as_the_export_row() {
        let dir = temp_dir("canonical");
        let input = dir.join("input.txt");
        write(&input, "你好 nihao 3\n");

        run(&dir.join("user"), &input).expect("first import");
        run(&dir.join("user"), &input).expect("second import is idempotent");

        assert_eq!(
            exported(&dir.join("user")),
            vec![ExportedPhrase {
                text: "你好".to_owned(),
                pinyin: "ni'hao".to_owned(),
                count: 3,
            }]
        );
    }
}
