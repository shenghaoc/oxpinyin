//! Runtime rpath for this package's Kyoto Cabinet link.
//!
//! A build script's `cargo:rustc-link-arg` is package-scoped: the one
//! oxpinyin-store emits lands on the store crate's own artifacts, not on
//! this binary — so a Kyoto Cabinet outside the default loader path (a
//! `SYSTEM_DEPS_KYOTOCABINET_SEARCH_NATIVE`, or a pkg-config `-L` from a
//! custom prefix) would link here and then fail to start. This package does
//! not discover Kyoto Cabinet: oxpinyin-store's build script publishes the
//! directories it resolved as `lib_dir` links metadata, and this script
//! repeats them as an rpath for the binaries this package links. Only the
//! store's Kyoto Cabinet build publishes that metadata, so under the other
//! backends the variable is absent and this emits nothing. As in the store, the rpath
//! is a convenience, not the contract: an artifact built without it must
//! still find the library via `LD_LIBRARY_PATH` or its own rpath setting.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    emit_kc_rpath();
}

/// Emits `-Wl,-rpath` for every non-system Kyoto Cabinet library directory
/// the store resolved. Link search and library names stay the store's job
/// (those propagate graph-wide); only the package-scoped rpath has to be
/// repeated here.
fn emit_kc_rpath() {
    // Absent under every other backend, and under Kyoto Cabinet when every
    // directory is a system one: no rpath names one.
    let Some(dirs) = std::env::var_os("DEP_OXPINYIN_STORE_LIB_DIR") else {
        return;
    };
    for path in std::env::split_paths(&dirs) {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", path.display());
    }
}
