//! Helpers for the env-gated differential suites against the pin-built
//! libpinyin trainer binaries.
//!
//! The W9 tool suites (counter, emitter, lambda, corpus) share the same
//! mechanical harness: locating pin binaries and data dirs from the
//! environment, a fresh copy of the flat trainer data per pin run, a
//! runner that captures one pin command's stdout, the FNV-1a manifest
//! checksum, and the `estimate_interpolation` stdout parser. This module
//! holds each piece once; the suites contribute only their own fixtures
//! and assertions.
//!
//! Everything here is inert without the `PINYIN_*` environment variables:
//! the locators return `None` and the suites skip rather than fail, which
//! is what keeps these tests CI-unconditional on hosts without the pin.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// FNV-1a 64-bit, dependency-free and deterministic across platforms.
///
/// Same construction as the W9 training manifests
/// (`fixtures/w9/*.manifest`): a change-detection fingerprint, not a
/// cryptographic digest.
#[must_use]
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The `{unigrams, bigrams, fnv1a64}` manifest shape shared by the counter
/// and emitter golden files.
pub struct Manifest {
    /// Expected unigram count.
    pub unigrams: usize,
    /// Expected bigram count.
    pub bigrams: usize,
    /// Expected FNV-1a 64 checksum, lowercase hex in the file.
    pub checksum: u64,
}

/// Parses a committed golden manifest; blank and `#` lines are ignored.
#[must_use]
pub fn parse_manifest(text: &str) -> Manifest {
    let mut manifest = Manifest {
        unigrams: 0,
        bigrams: 0,
        checksum: 0,
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.split_whitespace();
        let (Some(key), Some(value)) = (fields.next(), fields.next()) else {
            continue;
        };
        match key {
            "unigrams" => manifest.unigrams = value.parse().expect("manifest unigrams"),
            "bigrams" => manifest.bigrams = value.parse().expect("manifest bigrams"),
            "fnv1a64" => {
                manifest.checksum = u64::from_str_radix(value, 16).expect("manifest checksum");
            }
            _ => {}
        }
    }
    manifest
}

/// Resolves `name` to an existing file path — the shape of every
/// `PINYIN_<TOOL>` binary locator in the differential suites.
#[must_use]
pub fn locate_bin(name: &str) -> Option<PathBuf> {
    let raw = std::env::var_os(name)?;
    let path = PathBuf::from(raw);
    path.is_file().then_some(path)
}

/// Resolves `name` to a non-empty data dir holding `table.conf` — the
/// shape of every `PINYIN_*_DATA` locator in the differential suites.
#[must_use]
pub fn locate_data(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os(name)?);
    (path.join("table.conf").is_file() && path.read_dir().is_ok_and(|mut dir| dir.next().is_some()))
        .then_some(path)
}

/// A fresh copy of the pin trainer's flat data dir that one pin command
/// can run in; removes itself on drop.
///
/// The dir is a [`tempfile::TempDir`]: a unique name per instance, so two
/// live `PinDir`s never share a path even with the same tag in the same
/// process, and nothing pre-existing is ever deleted to make room.
///
/// Only the raw `.table` sources and `table.conf` are copied. The
/// `.bin`/`.db` files in a live data dir are *outputs* of earlier runs;
/// copying them would make `gen_ngram` append onto a stale `bigram.db`
/// (and `gen_unigram` onto a stale `phrase_index.bin`). `gen_binary_files`
/// rebuilds every binary index from the tables, so the pipeline must
/// start clean.
pub struct PinDir {
    dir: tempfile::TempDir,
}

impl PinDir {
    /// Copies `data`'s raw tables into a new `…/oxpinyin-pin-{tag}-XXXXXX`.
    ///
    /// # Errors
    ///
    /// When the temp dir cannot be created or a table cannot be copied.
    pub fn fresh(data: &Path, tag: &str) -> Result<Self, String> {
        let dir = tempfile::Builder::new()
            .prefix(&format!("oxpinyin-pin-{tag}-"))
            .tempdir()
            .map_err(|error| error.to_string())?;
        for entry in data
            .read_dir()
            .map_err(|error| error.to_string())?
            .flatten()
        {
            if entry.file_type().is_ok_and(|kind| kind.is_file()) {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name == "table.conf" || name.ends_with(".table") {
                    std::fs::copy(entry.path(), dir.path().join(entry.file_name()))
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        Ok(Self { dir })
    }

    /// The path of `name` inside this dir.
    #[must_use]
    pub fn path(&self, name: &str) -> std::path::PathBuf {
        self.dir.path().join(name)
    }

    /// Runs one pin command with this dir as its working directory,
    /// capturing stdout; errors name the binary, its exit status, and the
    /// captured stderr.
    ///
    /// # Errors
    ///
    /// When the command cannot spawn, stdin cannot be written, the wait
    /// fails, or the command exits nonzero.
    pub fn run(&self, bin: &Path, args: &[&str], stdin: Option<&[u8]>) -> Result<Vec<u8>, String> {
        let mut command = Command::new(bin);
        command.current_dir(self.dir.path()).args(args);
        if stdin.is_some() {
            command.stdin(Stdio::piped());
        }
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| error.to_string())?;
        if let Some(input) = stdin {
            use std::io::Write;
            child
                .stdin
                .as_mut()
                .ok_or("no stdin")?
                .write_all(input)
                .map_err(|error| error.to_string())?;
        }
        let output = child
            .wait_with_output()
            .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "{} exited {}: {}",
                bin.display(),
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(output.stdout)
    }
}

/// Parses `estimate_interpolation` stdout: `token:%d lambda:%f` per
/// context and `average lambda:%f`, kept as the pin printed them.
#[must_use]
pub fn parse_estimate_stdout(text: &str) -> (BTreeMap<u32, String>, Option<String>) {
    let mut per_context = BTreeMap::new();
    let mut average = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("token:")
            && let Some((token, lambda)) = rest.split_once(" lambda:")
            && let Ok(token) = token.parse::<u32>()
        {
            per_context.insert(token, lambda.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("average lambda:") {
            average = Some(rest.trim().to_string());
        }
    }
    (per_context, average)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use super::PinDir;

    /// A source data dir shaped like a live trainer dir: raw tables plus
    /// stale outputs and noise that `PinDir::fresh` must leave behind.
    fn live_data_dir() -> tempfile::TempDir {
        let data = tempfile::Builder::new()
            .prefix("oxpinyin-pin-src-")
            .tempdir()
            .expect("source temp dir");
        for (name, body) in [
            ("table.conf", "conf"),
            ("gb_char.table", "gb"),
            ("merged.table", "merged"),
            ("phrase_index.bin", "stale"),
            ("bigram.db", "stale"),
            ("notes.txt", "noise"),
        ] {
            std::fs::write(data.path().join(name), body).expect("source file");
        }
        // A directory whose name passes the extension filter is not a file.
        std::fs::create_dir(data.path().join("nested.table")).expect("source subdir");
        data
    }

    fn names(dir: &Path) -> BTreeSet<String> {
        dir.read_dir()
            .expect("read pin dir")
            .map(|entry| {
                entry
                    .expect("pin dir entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    #[test]
    fn fresh_copies_exactly_the_tables_and_table_conf() {
        let data = live_data_dir();
        let pin = PinDir::fresh(data.path(), "select").expect("pin dir");
        assert_eq!(
            names(pin.dir.path()),
            BTreeSet::from(["gb_char.table", "merged.table", "table.conf"].map(String::from)),
        );
        for name in ["gb_char.table", "merged.table", "table.conf"] {
            assert_eq!(
                std::fs::read(pin.dir.path().join(name)).expect("copied file"),
                std::fs::read(data.path().join(name)).expect("source file"),
                "{name} is copied byte-for-byte",
            );
        }
    }

    #[test]
    fn same_tag_dirs_are_distinct_while_live_and_removed_on_drop() {
        let data = live_data_dir();
        let first = PinDir::fresh(data.path(), "same").expect("first pin dir");
        let second = PinDir::fresh(data.path(), "same").expect("second pin dir");
        let first_path = first.dir.path().to_path_buf();
        let second_path = second.dir.path().to_path_buf();

        assert_ne!(first_path, second_path, "same tag, same process, two dirs");
        for path in [&first_path, &second_path] {
            let name = path.file_name().expect("dir name").to_string_lossy();
            assert!(name.starts_with("oxpinyin-pin-same-"), "{name}");
            assert!(path.join("table.conf").is_file(), "live dir keeps its copy");
        }

        drop(first);
        assert!(!first_path.exists(), "drop removes the dir");
        assert!(
            second_path.join("table.conf").is_file(),
            "the other survives"
        );
        drop(second);
        assert!(!second_path.exists(), "drop removes the dir");
    }

    #[cfg(unix)]
    #[test]
    fn run_executes_in_the_pin_dir_with_stdin_and_reports_failures() {
        let data = live_data_dir();
        let pin = PinDir::fresh(data.path(), "run").expect("pin dir");
        let sh = Path::new("/bin/sh");

        let stdout = pin
            .run(sh, &["-c", "cat table.conf; cat"], Some(b"|stdin"))
            .expect("command succeeds");
        assert_eq!(stdout, b"conf|stdin");

        let error = pin
            .run(sh, &["-c", "echo oops >&2; exit 3"], None)
            .expect_err("nonzero exit is an error");
        assert!(error.starts_with("/bin/sh exited "), "{error}");
        assert!(error.ends_with(": oops\n"), "{error}");
    }
}
