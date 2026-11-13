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

// The link step `oxpinyin-capi`'s build script uses. `tools/dev/packaging.py`
// computes both `.pc` files; no build script writes them.
#[path = "../oxpinyin-capi/build_capi.rs"]
mod build_capi;

fn main() {
    build_capi::run(&build_capi::Library {
        soname: "libzhuyin.so.15",
        shared_source: "../oxpinyin-capi/build_capi.rs",
    });
}
