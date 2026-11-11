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
//! not by a linker script. (Without `--features shipped` the crate also
//! exports its own test-only `oxpinyin_test_set_user_bigram` hook.)
//!
//! # The complete `libpinyin.pc`
//!
//! cargo-c generates a `libpinyin.pc` from
//! `[package.metadata.capi.pkg_config]`, but its field set is closed and it
//! cannot emit the four variables real consumers read (`pkgdatadir`,
//! `database_format`, `libpinyinincludedir`, `libpinyin_binary_version`); it
//! also exposes no install prefix to build scripts and installs only its own
//! `.pc` into the pkg-config dir. So this script bakes the build-time-known
//! fields of `libpinyin.pc.in` into `$OUT_DIR/libpinyin.pc.in.baked`, leaving
//! the install-time `@prefix@` / `@libdir@` placeholders for the packaging
//! wrapper (`just packaging install`) to fill after `cargo cinstall`. See
//! `docs/findings/installed-naming.md` for the full rationale and the
//! verified cargo-c limits. `database_format` is the token `oxpinyin-store`'s
//! build script selects from the backend, the value `libzhuyin.pc` carries
//! too.
//!
//! The steps themselves are shared with `oxpinyin-zhuyin-capi` in
//! `build_capi.rs`.

mod build_capi;
mod build_pin_version;

fn main() {
    build_capi::run(&build_capi::Library {
        name: "libpinyin",
        template: "libpinyin.pc.in",
        soname: "libpinyin.so.15",
        shared_source: "build_capi.rs",
        pin_version_source: "build_pin_version.rs",
    });
}
