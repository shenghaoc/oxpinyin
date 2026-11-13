//! The build step the two C-ABI crates share: the Linux link setup.
//!
//! Shared by the `build.rs` of `oxpinyin-capi` and `oxpinyin-zhuyin-capi`
//! (the latter includes it by `#[path]`). The per-crate parameters are a
//! [`Library`]. Neither build script writes a `.pc` file or reads Cargo's
//! build directory: `tools/dev/packaging.py` computes the `.pc` values.

use std::env;

/// One C-ABI library's identity.
pub struct Library {
    /// The ELF SONAME, `libpinyin.so.15` or `libzhuyin.so.15`.
    pub soname: &'static str,
    /// This shared module's path relative to the crate's manifest, for
    /// `cargo:rerun-if-changed`.
    pub shared_source: &'static str,
}

/// Runs the shared build steps for `library`.
pub fn run(library: &Library) {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", library.shared_source);

    link_linux(library);

    // glib-2.0 linking is handled by glib-sys's build script (system-deps).
    // Constrained builders: use SYSTEM_DEPS_GLIB_2_0_SEARCH_NATIVE,
    // SYSTEM_DEPS_GLIB_2_0_LIB, and SYSTEM_DEPS_GLIB_2_0_NO_PKG_CONFIG
    // in place of the former GLIB_LIBS override.
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
