//! Link discovery for the pin-built libpinyin.
//!
//! Does nothing unless the `oracle-ffi` feature is enabled, so the workspace
//! builds and tests on every supported host with no oracle installed.
//!
//! When the feature is on, the prefix must be the output of
//! `tools/oracle/build-oracle.sh`. A prefix whose `oracle-pin.txt` does not
//! carry the frozen pin reference is refused, so a stray libpinyin on the
//! search path cannot be linked by accident.

use std::path::{Path, PathBuf};

// Single source of truth, shared with the crate at run time.
include!("src/pin_ref.rs");

fn main() {
    println!("cargo::rerun-if-env-changed=PINYIN_ORACLE_PREFIX");
    println!("cargo::rerun-if-env-changed=PKG_CONFIG_PATH");

    if std::env::var_os("CARGO_FEATURE_ORACLE_FFI").is_none() {
        return;
    }

    let prefix = match locate_prefix() {
        Ok(prefix) => prefix,
        Err(reason) => {
            eprintln!("error: {reason}");
            eprintln!();
            eprintln!("The `oracle-ffi` feature links the pinned libpinyin, which is only the");
            eprintln!("output of tools/oracle/build-oracle.sh. Build it, then point at it:");
            eprintln!();
            eprintln!(
                "  bash tools/oracle/build-oracle.sh \\\n    --prefix \"$HOME/{DEFAULT_PREFIX_SUFFIX}\" --jobs \"$(nproc)\""
            );
            eprintln!();
            eprintln!("  PINYIN_ORACLE_PREFIX=$HOME/{DEFAULT_PREFIX_SUFFIX} \\");
            eprintln!("  LD_LIBRARY_PATH=$HOME/{DEFAULT_PREFIX_SUFFIX}/lib \\");
            eprintln!("  cargo test -p pinyin-oracle --features oracle-ffi");
            std::process::exit(1);
        }
    };

    let manifest = prefix.join(MANIFEST_FILE_NAME);
    println!("cargo::rerun-if-changed={}", manifest.display());

    let lib_dir = library_dir(&prefix).unwrap_or_else(|| prefix.join("lib"));
    println!("cargo::rustc-link-search=native={}", lib_dir.display());
    println!("cargo::rustc-link-lib=dylib=pinyin");
    println!("cargo::rustc-link-lib=dylib=glib-2.0");

    // Let the test binary find the pinned shared object without the caller
    // exporting LD_LIBRARY_PATH by hand.
    println!("cargo::rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());
}

/// Default prefix suffix under `$HOME`, mirrored from the crate root.
const DEFAULT_PREFIX_SUFFIX: &str = ".local/opt/pinyin-oracle";

/// Locates a pin-verified prefix.
///
/// Order: `PINYIN_ORACLE_PREFIX`, then each `PKG_CONFIG_PATH` entry walked up to
/// its prefix root, then `$HOME/.local/opt/pinyin-oracle`.
fn locate_prefix() -> Result<PathBuf, String> {
    let mut tried: Vec<String> = Vec::new();

    if let Some(raw) = std::env::var_os("PINYIN_ORACLE_PREFIX") {
        let candidate = PathBuf::from(raw);
        return match verify(&candidate) {
            Ok(()) => Ok(candidate),
            Err(reason) => Err(format!(
                "PINYIN_ORACLE_PREFIX is set but unusable: {reason}"
            )),
        };
    }

    if let Some(raw) = std::env::var_os("PKG_CONFIG_PATH") {
        for entry in std::env::split_paths(&raw) {
            // .../lib/pkgconfig or .../lib64/pkgconfig -> prefix root
            let Some(candidate) = entry.parent().and_then(Path::parent) else {
                continue;
            };
            match verify(candidate) {
                Ok(()) => return Ok(candidate.to_path_buf()),
                Err(reason) => tried.push(format!("  {}: {reason}", candidate.display())),
            }
        }
    }

    if let Some(home) = std::env::var_os("HOME") {
        let candidate = PathBuf::from(home).join(DEFAULT_PREFIX_SUFFIX);
        match verify(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(reason) => tried.push(format!("  {}: {reason}", candidate.display())),
        }
    }

    if tried.is_empty() {
        return Err("no oracle prefix found (PINYIN_ORACLE_PREFIX unset)".to_owned());
    }
    Err(format!(
        "no pin-verified oracle prefix found. Candidates tried:\n{}",
        tried.join("\n")
    ))
}

/// Accepts a prefix only if it carries the frozen pin and a shared object.
fn verify(prefix: &Path) -> Result<(), String> {
    let manifest = prefix.join(MANIFEST_FILE_NAME);
    let text = std::fs::read_to_string(&manifest)
        .map_err(|error| format!("cannot read {}: {error}", manifest.display()))?;

    let pin_ref = text
        .lines()
        .find_map(|line| line.strip_prefix("pin_ref="))
        .ok_or_else(|| format!("{} has no pin_ref field", manifest.display()))?
        .trim();

    if pin_ref != EXPECTED_PIN_REF {
        return Err(format!(
            "off-pin prefix: pin_ref is {pin_ref:?}, expected {EXPECTED_PIN_REF:?}"
        ));
    }

    if library_dir(prefix).is_none() {
        return Err(format!(
            "no libpinyin shared object under {}/lib or {}/lib64",
            prefix.display(),
            prefix.display()
        ));
    }

    Ok(())
}

/// Returns the prefix-local directory holding the pinned shared object.
///
/// The recipe exports both `lib/pkgconfig` and `lib64/pkgconfig`, so either
/// layout is valid.
fn library_dir(prefix: &Path) -> Option<PathBuf> {
    ["lib", "lib64"].into_iter().find_map(|name| {
        let dir = prefix.join(name);
        let entries = std::fs::read_dir(&dir).ok()?;
        let found = entries.filter_map(Result::ok).any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|file| file.starts_with("libpinyin.so"))
        });
        found.then_some(dir)
    })
}
