//! Link-time stamping of the zhuyin C ABI's identity.
//!
//! # SONAME
//!
//! Upstream's `libzhuyin.so.15` SONAME comes from libtool `-version-info 15:0`
//! (the same `@LT_VERSION_INFO@` as libpinyin — `src/Makefile.am:118-121` at
//! the pin 0c5e80e1; `configure.ac`'s `libpinyin_abi_current=15`). Emitted as
//! a `cdylib` link arg.
//!
//! # Why there is no version script here
//!
//! A Rust `cdylib` cannot combine a named version script with rustc's own
//! anonymous script (GNU ld rejects the pair, rust-lld refuses to reassign
//! the symbols) — the same constraint recorded in `oxpinyin-capi`'s
//! `build.rs:26-41`. Symbol scope is therefore enforced in the SOURCE (only
//! the 52 `#[unsafe(no_mangle)]` `zhuyin_*` symbols), and the checked-in
//! `libzhuyin.ver` is consumed by `just packaging relink-versioned`
//! (run by `just packaging install` on Linux) when it relinks the
//! installed object from the staticlib under upstream's symbol versioning.

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Keep the Linux link setup in sync with crates/oxpinyin-capi/build.rs.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("linux") {
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
