//! Link-time stamping of the C ABI's identity.
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

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // SONAME is an ELF concept. The crate is Linux-first by design but must
    // not fail to build elsewhere.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libpinyin.so.15");
    }
}
