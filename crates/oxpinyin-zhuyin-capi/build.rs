//! Link-time stamping of the zhuyin C ABI's identity, plus the baked
//! `libzhuyin.pc` content.
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
//! `libzhuyin.ver` is consumed by `tools/packaging/relink-versioned.sh`
//! (run by `tools/packaging/install.sh` on Linux) when it relinks the
//! installed object from the staticlib under upstream's symbol versioning.

// The pin-version reader and the build steps `oxpinyin-capi`'s build script
// uses, so both `.pc` files and both header subdirectories follow the one
// pin record, and both `.pc` files carry the one database-format token.
#[path = "../oxpinyin-capi/build_capi.rs"]
mod build_capi;
#[path = "../oxpinyin-capi/build_pin_version.rs"]
mod build_pin_version;

fn main() {
    build_capi::run(&build_capi::Library {
        name: "libzhuyin",
        template: "libzhuyin.pc.in",
        soname: "libzhuyin.so.15",
        shared_source: "../oxpinyin-capi/build_capi.rs",
        pin_version_source: "../oxpinyin-capi/build_pin_version.rs",
    });
}
