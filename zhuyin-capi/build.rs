//! Link-time stamping of the zhuyin C ABI's identity.
//!
//! # SONAME
//!
//! Upstream's `libzhuyin.so.15` SONAME comes from libtool `-version-info 15:0`
//! (the same `@LT_VERSION_INFO@` as libpinyin — `src/Makefile.am:118-121` at
//! the pin 0c5e80e1; `configure.ac`'s `libpinyin_abi_current=15`).
//! Plain Cargo builds set the SONAME on the Linux cdylib and hide CXX
//! archive symbols for the tkrzw and Kyoto Cabinet backends.
//! cargo-c enables `capi` and sets the SONAME for its own Linux cdylib.
//! On Linux GNU targets every cdylib link, cargo-c's included, also packs its
//! relative relocations into `DT_RELR` (`-z pack-relative-relocs`;
//! docs/packaging.md#build-and-runtime-dependencies states the requirement).
//! With `capi` enabled, this script emits no other cdylib link argument.
//!
//! # Why there is no version script here
//!
//! A Rust `cdylib` cannot combine a named version script with rustc's own
//! anonymous script (GNU ld rejects the pair, rust-lld refuses to reassign
//! the symbols). On Linux, `just packaging install` uses
//! `just packaging relink-versioned` to link the shipped shared library from
//! the cargo-c staticlib. The relink sets the SONAME, applies `libzhuyin.ver`,
//! and verifies the exact export set.

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Keep the Linux link setup in sync with capi/build.rs.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
        return;
    }
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu") {
        println!("cargo:rustc-cdylib-link-arg=-Wl,-z,pack-relative-relocs");
    }
    if env::var_os("CARGO_FEATURE_CAPI").is_some() {
        return;
    }
    println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libzhuyin.so.15");
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
