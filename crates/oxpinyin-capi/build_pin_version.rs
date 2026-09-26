//! The drop-in's libpinyin version, read from its one source of truth.
//!
//! Shared by the `build.rs` of `oxpinyin-capi` and `oxpinyin-zhuyin-capi`
//! (the latter includes it by `#[path]`). The installed identity — the
//! `.pc` `Version:`, the `libpinyin-<version>` include subdirectory and
//! `libpinyinincludedir` / `libzhuyinincludedir` — is the version the
//! pin's `configure.ac` stamps (`libpinyin_major/minor/micro_version`),
//! which `tools/oracle/oracle-pin.txt` records as `libpinyin_tag`. A pin
//! bump edits that record, and this module makes the build follow it:
//! the version is read from the record rather than restated, and the
//! crate's own `[package.metadata.capi]` tables — which cargo-c reads
//! and a build script cannot rewrite — are checked against it, so a
//! manifest left behind by a pin bump fails the build instead of
//! installing a stale header directory.

use std::fs;
use std::path::{Path, PathBuf};

/// The pin record, relative to either capi crate's manifest directory.
const PIN_RECORD: &str = "../../tools/oracle/oracle-pin.txt";

/// The key in the pin record that carries the pin's `configure.ac`
/// version.
const PIN_VERSION_KEY: &str = "libpinyin_tag=";

/// The pin record's path for the crate at `manifest_dir`, for
/// `cargo:rerun-if-changed`.
pub fn pin_record_path(manifest_dir: &Path) -> PathBuf {
    manifest_dir.join(PIN_RECORD)
}

/// Reads the pin's libpinyin version from the pin record and checks that
/// the crate's `Cargo.toml` declares the same one. Panics — failing the
/// build — when the record is unreadable, carries no well-formed
/// version, or disagrees with the manifest.
pub fn pin_version(manifest_dir: &Path) -> String {
    let record_path = pin_record_path(manifest_dir);
    let record = fs::read_to_string(&record_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", record_path.display()));
    let version = parse_pin_version(&record).unwrap_or_else(|| {
        panic!(
            "{}: no well-formed `{PIN_VERSION_KEY}<major>.<minor>.<micro>` line",
            record_path.display()
        )
    });

    let manifest_path = manifest_dir.join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", manifest_path.display()));
    if let Err(problem) = check_manifest(&manifest, &version) {
        panic!(
            "{}: {problem}; the pin record {} says {version} — update the \
             [package.metadata.capi] tables to match",
            manifest_path.display(),
            record_path.display()
        );
    }
    version
}

/// The value of the record's `libpinyin_tag=` line, when it is a dotted
/// numeric version.
fn parse_pin_version(record: &str) -> Option<String> {
    let value = record
        .lines()
        .find_map(|line| line.trim().strip_prefix(PIN_VERSION_KEY))?
        .trim();
    let well_formed = value.split('.').count() == 3
        && value
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    well_formed.then(|| value.to_owned())
}

/// Checks that every `"libpinyin-<v>"` string in the manifest (the
/// header subdirectory and the asset destinations) and the
/// `[package.metadata.capi.pkg_config]` `version` name `version`, and
/// that both are present.
fn check_manifest(manifest: &str, version: &str) -> Result<(), String> {
    let want_dir = format!("libpinyin-{version}");
    let mut dirs = 0usize;
    let mut pc_version = None;
    let mut section = "";
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = header;
            continue;
        }
        if section == "package.metadata.capi.pkg_config"
            && let Some(value) = line.strip_prefix("version")
            && let Some(value) = value.trim_start().strip_prefix('=')
        {
            pc_version = Some(value.trim().trim_matches('"').to_owned());
        }
        for (start, _) in line.match_indices("\"libpinyin-") {
            let rest = &line[start + 1..];
            let dir = rest.split('"').next().unwrap_or(rest);
            if dir != want_dir {
                return Err(format!("declares include subdirectory \"{dir}\""));
            }
            dirs += 1;
        }
    }
    if dirs == 0 {
        return Err(format!("declares no \"{want_dir}\" include subdirectory"));
    }
    match pc_version {
        Some(v) if v == version => Ok(()),
        Some(v) => Err(format!(
            "[package.metadata.capi.pkg_config] version is \"{v}\""
        )),
        None => Err("[package.metadata.capi.pkg_config] declares no version".to_owned()),
    }
}
