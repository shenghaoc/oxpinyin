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
    (path.join("table.conf").is_file()
        && path
            .read_dir()
            .map(|mut dir| dir.next().is_some())
            .unwrap_or(false))
    .then_some(path)
}

/// A fresh copy of the pin trainer's flat data dir that one pin command
/// can run in; removes itself on drop.
///
/// Only the raw `.table` sources and `table.conf` are copied. The
/// `.bin`/`.db` files in a live data dir are *outputs* of earlier runs;
/// copying them would make `gen_ngram` append onto a stale `bigram.db`
/// (and `gen_unigram` onto a stale `phrase_index.bin`). `gen_binary_files`
/// rebuilds every binary index from the tables, so the pipeline must
/// start clean.
pub struct PinDir {
    dir: PathBuf,
}

impl PinDir {
    /// Copies `data`'s raw tables into `…/oxpinyin-pin-{tag}-{pid}`.
    ///
    /// # Errors
    ///
    /// When the temp dir cannot be created or a table cannot be copied.
    pub fn fresh(data: &Path, tag: &str) -> Result<Self, String> {
        let dir = std::env::temp_dir().join(format!("oxpinyin-pin-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        for entry in data
            .read_dir()
            .map_err(|error| error.to_string())?
            .flatten()
        {
            if entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
            {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name == "table.conf" || name.ends_with(".table") {
                    std::fs::copy(entry.path(), dir.join(entry.file_name()))
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        Ok(Self { dir })
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
        command.current_dir(&self.dir).args(args);
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

impl Drop for PinDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
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
