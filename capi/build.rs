//! Link-time stamping of the C ABI's identity.
//!
//! # SONAME
//!
//! The real library's is `libpinyin.so.15` — libtool `-version-info 15:0`
//! from `libpinyin_abi_current` in the pin's `configure.ac`, confirmed with
//! `readelf -d` on Ubuntu's shipped `libpinyin15`. A consumer's `DT_NEEDED`
//! records that string, so a drop-in replacement has to answer to it.
//! Plain Cargo builds set the SONAME on the Linux cdylib and hide CXX
//! archive symbols for the tkrzw and Kyoto Cabinet backends.
//! cargo-c enables `capi` and sets the SONAME for its own Linux cdylib.
//! With `capi` enabled, this script emits no cdylib link arguments.
//!
//! # Why there is no version script here
//!
//! The real library defines its symbols *versioned* — `pinyin_init@@LIBPINYIN`
//! — so every consumer linked against it records `File: libpinyin.so.15 ->
//! Name: LIBPINYIN` in `.gnu.version_r`. Three library shapes behave
//! differently against a consumer carrying that reference:
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
//! On Linux, `just packaging install` uses `just packaging relink-versioned`
//! to link the shipped shared library from the cargo-c staticlib. The relink
//! sets the SONAME, applies `libpinyin.ver`, and verifies the exact export set
//! (docs/packaging.md#drop-in-identity states the requirement).
//!
//! # The complete `libpinyin.pc`
//!
//! cargo-c generates a `libpinyin.pc` from
//! `[package.metadata.capi.pkg_config]`, but its field set is closed and it
//! cannot emit the four variables real consumers read (`pkgdatadir`,
//! `database_format`, `libpinyinincludedir`, `libpinyin_binary_version`).
//! This script writes no `.pc` content. `just packaging install`
//! (`tools/dev/packaging.py`) computes the complete file from
//! `libpinyin.pc.in` after `cargo cinstall` and reads nothing from Cargo's
//! build directory. See `docs/findings/installed-naming.md` for the
//! rationale and the verified cargo-c limits.

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Keep the Linux link setup in sync with zhuyin-capi/build.rs.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux")
        || env::var_os("CARGO_FEATURE_CAPI").is_some()
    {
        return;
    }
    println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libpinyin.so.15");
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
