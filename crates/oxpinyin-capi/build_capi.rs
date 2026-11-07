//! The build steps the two C-ABI crates share: the Linux link setup and
//! the bake of the crate's `.pc` template.
//!
//! Shared by the `build.rs` of `oxpinyin-capi` and `oxpinyin-zhuyin-capi`
//! (the latter includes it by `#[path]`, as it does `build_pin_version`).
//! The per-crate parameters are a [`Library`]: the template, the SONAME
//! and the library's own name.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::build_pin_version;

/// Mirrors the libtool `current.revision` behind
/// `[package.metadata.capi.library].version` in each crate's Cargo.toml.
/// The `.pc` version is not restated here: `build_pin_version` reads it
/// from the pin record and checks the manifest's header subdirectory and
/// pkg_config version against it.
const PC_BINARY_VERSION: &str = "15.0";

/// One C-ABI library's identity.
pub struct Library {
    /// The library's name, `libpinyin` or `libzhuyin`: the baked file is
    /// `<name>.pc.in.baked`.
    pub name: &'static str,
    /// The `.pc` template beside the crate's manifest.
    pub template: &'static str,
    /// The ELF SONAME, `libpinyin.so.15` or `libzhuyin.so.15`.
    pub soname: &'static str,
    /// This shared module's path relative to the crate's manifest, for
    /// `cargo:rerun-if-changed`.
    pub shared_source: &'static str,
    /// `build_pin_version.rs`'s path relative to the crate's manifest.
    pub pin_version_source: &'static str,
}

/// Runs the shared build steps for `library`.
pub fn run(library: &Library) {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"));
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", library.shared_source);
    println!("cargo:rerun-if-changed={}", library.pin_version_source);
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed={}", library.template);
    println!(
        "cargo:rerun-if-changed={}",
        build_pin_version::pin_record_path(&manifest_dir).display()
    );

    link_linux(library);

    // glib-2.0 linking is handled by glib-sys's build script (system-deps).
    // Constrained builders: use SYSTEM_DEPS_GLIB_2_0_SEARCH_NATIVE,
    // SYSTEM_DEPS_GLIB_2_0_LIB, and SYSTEM_DEPS_GLIB_2_0_NO_PKG_CONFIG
    // in place of the former GLIB_LIBS override.

    let pc_version = build_pin_version::pin_version(&manifest_dir);
    bake_pkg_config_template(library, &manifest_dir, &pc_version);
}

/// The SONAME and, under the cxx backends, `--exclude-libs`.
///
/// SONAME is an ELF concept. The crates are Linux-first by design but must
/// not fail to build elsewhere.
fn link_linux(library: &Library) {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,{}", library.soname);
    // The tkrzw and Kyoto Cabinet backends' cxx bridges link the `cxx`
    // crate, whose Rust runtime defines `#[export_name]` symbols
    // (`cxxbridge1$slice$*`, `cxxbridge1$string$*`, ...) that rustc lists
    // as C-level exports of every cdylib in the graph. They are not part
    // of the libpinyin ABI (docs/packaging.md#export-gate). The symbols
    // live in rlib archives, and GNU ld's `--exclude-libs` gives every symbol
    // defined in an archive hidden visibility, which a version script
    // cannot override; the crate's own `pinyin_*` / `zhuyin_*` symbols come
    // from its object files and are untouched. The shipped library is
    // relinked under its `.ver` script and needs none of this.
    if env::var_os("CARGO_FEATURE_TKRZW").is_some()
        || env::var_os("CARGO_FEATURE_KYOTOCABINET").is_some()
    {
        println!("cargo:rustc-cdylib-link-arg=-Wl,--exclude-libs,ALL");
    }
}

/// Bakes the build-time fields of the template — `@VERSION@`,
/// `@LIBPINYIN_BINARY_VERSION@`, `@DATABASE_FORMAT@` — leaving the
/// install-time `@prefix@` / `@libdir@` for the wrapper. Writes the result to
/// `$OUT_DIR/<name>.pc.in.baked` and mirrors it to
/// `<target>/<profile>/<name>.pc.in.baked`, an un-hashed path the wrapper
/// can read without discovering the build-hash directory.
fn bake_pkg_config_template(library: &Library, manifest_dir: &Path, pc_version: &str) {
    let template_path = manifest_dir.join(library.template);
    let template = fs::read_to_string(&template_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", template_path.display()));

    let substituted = template
        .replace("@VERSION@", pc_version)
        .replace("@LIBPINYIN_BINARY_VERSION@", PC_BINARY_VERSION)
        .replace("@DATABASE_FORMAT@", &database_format());

    // Drop the template's `#` header: it documents the source file, not the
    // installed one, and the real `.pc` carries no such block. The
    // remaining `key=value` / `Field:` lines (and the blank separators between
    // them) are what a genuine drop-in ships.
    let body: String = substituted
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    let baked = format!("{}\n", body.trim_start_matches('\n'));

    let baked_name = format!("{}.pc.in.baked", library.name);
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    let out_path = Path::new(&out_dir).join(&baked_name);
    fs::write(&out_path, &baked).unwrap_or_else(|e| panic!("write {}: {e}", out_path.display()));

    // Best-effort mirror to <target>/<profile>; the wrapper falls back to a
    // `find` under the target dir when this is absent.
    if let Some(profile_dir) = target_profile_dir(&out_dir) {
        let _ = fs::write(profile_dir.join(&baked_name), &baked);
    }
}

/// The `@DATABASE_FORMAT@` value: the token `oxpinyin-store`'s build script
/// selected from the backend it compiles, read through its `links`
/// metadata. Both `.pc` files read this one value, as the pin's
/// `configure.ac:134-135` substitutes one `DATABASE_FORMAT` into both.
fn database_format() -> String {
    env::var("DEP_OXPINYIN_STORE_DATABASE_FORMAT").expect(
        "DEP_OXPINYIN_STORE_DATABASE_FORMAT is set by oxpinyin-store's build script \
         for a build with one backend feature",
    )
}

/// `<target>/<profile>` derived from `OUT_DIR`, whose shape is
/// `<target>/<profile>/build/<pkg>-<hash>/out`.
fn target_profile_dir(out_dir: &str) -> Option<PathBuf> {
    // out -> <pkg>-<hash> -> build -> <profile>
    Path::new(out_dir).ancestors().nth(3).map(Path::to_path_buf)
}
