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
//! So a version script here would turn a working drop-in into a broken one.
//! Reaching upstream's shape needs the cdylib linked manually from the
//! staticlib in the packaging step, where rustc's anonymous script is not in
//! the link line. That step exists: `just packaging relink-versioned`,
//! run by `just packaging install` on Linux after `cargo cinstall`,
//! replaces the installed object with the versioned relink and verifies the
//! export set against this crate's `libpinyin.ver` in both directions
//! (docs/packaging.md#drop-in-identity states the requirement).
//!
//! Symbol scope is therefore enforced in the source — the shipped
//! artifact defines every symbol in `libpinyin.ver` and nothing else —
//! not by a linker script. (The build without `--features shipped`
//! exports the same set.)
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
//!
//! The link step is shared with `oxpinyin-zhuyin-capi` in `build_capi.rs`.

mod build_capi;

fn main() {
    build_capi::run(&build_capi::Library {
        soname: "libpinyin.so.15",
        shared_source: "build_capi.rs",
    });
}
