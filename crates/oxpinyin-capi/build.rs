//! Link-time stamping of the C ABI's identity, plus the drop-in
//! `libpinyin.pc` content the packaging wrapper installs.
//!
//! # SONAME
//!
//! The real library's is `libpinyin.so.15` — libtool `-version-info 15:0`
//! from `libpinyin_abi_current` in the pin's `configure.ac`, confirmed with
//! `readelf -d` on Ubuntu's shipped `libpinyin15`. A consumer's `DT_NEEDED`
//! records that string, so a drop-in replacement has to answer to it. Emitted
//! as a `cdylib` link arg, so the `staticlib` and `rlib` artifacts are
//! unaffected.
//!
//! # Why there is no version script here
//!
//! The real library defines its symbols *versioned* — `pinyin_init@@LIBPINYIN`
//! — so every consumer linked against it records `File: libpinyin.so.15 ->
//! Name: LIBPINYIN` in `.gnu.version_r`. Three library shapes were measured
//! against a consumer carrying that reference:
//!
//! | shape | result |
//! |---|---|
//! | no version definitions at all | loads and runs; glibc warns `no version information available` once per start |
//! | symbols versioned under `LIBPINYIN` | loads and runs clean — upstream's shape |
//! | version definitions present, symbols *unversioned* | **hard failure**: `version 'LIBPINYIN' not found` |
//!
//! Passing `--version-script` from here produces the third shape, not the
//! second: rustc always emits its own *anonymous* version script for a cdylib
//! (`-Wl,--version-script=.../list`), and a second, named script cannot be
//! combined with it — GNU ld rejects the pair outright ("anonymous version tag
//! cannot be combined with other version tags") and rust-lld accepts the
//! script's version *definitions* while refusing to reassign the symbols,
//! leaving exactly the shape that fails to load.
//!
//! So a version script here would turn a working drop-in into a broken one.
//! Reaching upstream's shape needs the cdylib linked manually from the
//! staticlib in the packaging step, where rustc's anonymous script is not in
//! the link line. Until then the shipped library carries no version
//! definitions and consumers load it with the glibc warning.
//!
//! Symbol scope is therefore enforced in the source, by `#[cfg]` on the
//! exports outside the consumer union, not by a linker script.
//!
//! # The complete `libpinyin.pc`
//!
//! cargo-c 0.10.24 generates a `libpinyin.pc` from
//! `[package.metadata.capi.pkg_config]`, but its field set is closed and it
//! cannot emit the four variables real consumers read (`pkgdatadir`,
//! `database_format`, `libpinyinincludedir`, `libpinyin_binary_version`); it
//! also exposes no install prefix to build scripts and installs only its own
//! `.pc` into the pkg-config dir. So this script bakes the build-time-known
//! fields of `libpinyin.pc.in` into `$OUT_DIR/libpinyin.pc.in.baked`, leaving
//! the install-time `@prefix@` / `@libdir@` placeholders for the packaging
//! wrapper (`tools/packaging/install.sh`) to fill after `cargo cinstall`. See
//! `docs/findings/installed-naming.md` for the full rationale and the
//! verified cargo-c limits.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

// Mirror `[package.metadata.capi.pkg_config].version` and the libtool
// `current.revision` behind `[package.metadata.capi.library].version` in
// Cargo.toml. build.rs cannot read those metadata tables, so keep these in
// sync with them (and with `header.subdirectory = libpinyin-2.11.91`).
const PC_VERSION: &str = "2.11.91";
const PC_BINARY_VERSION: &str = "15.0";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=libpinyin.pc.in");
    println!("cargo:rerun-if-env-changed=LIBPINYIN_DATABASE_FORMAT");

    // SONAME is an ELF concept. The crate is Linux-first by design but must
    // not fail to build elsewhere.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libpinyin.so.15");
    }

    // glib-2.0 linking is handled by glib-sys's build script (system-deps).
    // Constrained builders: use SYSTEM_DEPS_GLIB_2_0_SEARCH_NATIVE,
    // SYSTEM_DEPS_GLIB_2_0_LIB, and SYSTEM_DEPS_GLIB_2_0_NO_PKG_CONFIG
    // in place of the former GLIB_LIBS override.

    bake_pkg_config_template();
}

/// Bakes the build-time fields of `libpinyin.pc.in` — `@VERSION@`,
/// `@LIBPINYIN_BINARY_VERSION@`, `@DATABASE_FORMAT@` — leaving the
/// install-time `@prefix@` / `@libdir@` for the wrapper. Writes the result to
/// `$OUT_DIR/libpinyin.pc.in.baked` and mirrors it to
/// `<target>/<profile>/libpinyin.pc.in.baked`, an un-hashed path the wrapper
/// can read without discovering the build-hash directory.
fn bake_pkg_config_template() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
    let template_path = Path::new(&manifest_dir).join("libpinyin.pc.in");
    let template = fs::read_to_string(&template_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", template_path.display()));

    let substituted = template
        .replace("@VERSION@", PC_VERSION)
        .replace("@LIBPINYIN_BINARY_VERSION@", PC_BINARY_VERSION)
        .replace("@DATABASE_FORMAT@", &database_format());

    // Drop the template's `#` header: it documents the source file, not the
    // installed one, and the real libpinyin.pc carries no such block. The
    // remaining `key=value` / `Field:` lines (and the blank separators between
    // them) are what a genuine drop-in ships.
    let body: String = substituted
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    let baked = format!("{}\n", body.trim_start_matches('\n'));

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    let out_path = Path::new(&out_dir).join("libpinyin.pc.in.baked");
    fs::write(&out_path, &baked).unwrap_or_else(|e| panic!("write {}: {e}", out_path.display()));

    // Best-effort mirror to <target>/<profile>; the wrapper falls back to a
    // `find` under the target dir when this is absent.
    if let Some(profile_dir) = target_profile_dir(&out_dir) {
        let _ = fs::write(profile_dir.join("libpinyin.pc.in.baked"), &baked);
    }
}

/// The `@DATABASE_FORMAT@` value. An explicit `LIBPINYIN_DATABASE_FORMAT`
/// wins — a packager shipping data in another engine's format sets it so
/// fcitx's cmake probe reads the right backend. Otherwise, the active
/// peer backend feature of THIS crate, matching
/// `oxpinyin_store::DefaultStore`: Kyoto Cabinet under the default
/// features, redb / LMDB / tkrzw when their `--no-default-features
/// --features <peer>` is selected.
fn database_format() -> String {
    if let Ok(explicit) = env::var("LIBPINYIN_DATABASE_FORMAT")
        && !explicit.trim().is_empty()
    {
        return explicit;
    }
    // `CARGO_FEATURE_<NAME>` is set for each enabled feature of THIS crate,
    // which forwards the backend selection down the chain. The order
    // mirrors `oxpinyin_store::DefaultStore` so a multi-feature build
    // resolves deterministically (kyotocabinet > tkrzw > lmdb > redb —
    // a tie-break, not a hierarchy).
    if env::var_os("CARGO_FEATURE_KYOTOCABINET").is_some() {
        "KyotoCabinet".to_owned()
    } else if env::var_os("CARGO_FEATURE_TKRZW").is_some() {
        "Tkrzw".to_owned()
    } else if env::var_os("CARGO_FEATURE_LMDB").is_some() {
        "LMDB".to_owned()
    } else {
        "redb".to_owned()
    }
}

/// `<target>/<profile>` derived from `OUT_DIR`, whose shape is
/// `<target>/<profile>/build/<pkg>-<hash>/out`.
fn target_profile_dir(out_dir: &str) -> Option<PathBuf> {
    // out -> <pkg>-<hash> -> build -> <profile>
    Path::new(out_dir).ancestors().nth(3).map(Path::to_path_buf)
}
