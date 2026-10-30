//! Runtime rpath for this package's Kyoto Cabinet link.
//!
//! A build script's `cargo:rustc-link-arg` is package-scoped: the one
//! oxpinyin-store emits lands on the store crate's own artifacts, not on
//! this binary — so a Kyoto Cabinet outside the default loader path
//! (`OXPINYIN_KC_LIB_DIR`, or a pkg-config `-L` from a custom prefix)
//! would link here and then fail to start. This script repeats the same
//! rpath for the binaries this package links, mirroring oxpinyin-store's
//! and oxpinyin-datagen's discovery exactly; with the `kyotocabinet`
//! feature off it emits nothing. As in the store, the rpath
//! is a convenience, not the contract: an artifact built without it must
//! still find the library via `LD_LIBRARY_PATH` or its own rpath setting.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // The same selector the store's build script tracks, for the same
    // reason: it can point the discovery at a different Kyoto Cabinet. The
    // probe tracks the PKG_CONFIG_* variables itself.
    println!("cargo:rerun-if-env-changed=OXPINYIN_KC_LIB_DIR");
    #[cfg(feature = "kyotocabinet")]
    emit_kc_rpath();
}

/// Emits `-Wl,-rpath` for every custom Kyoto Cabinet library directory
/// the store's build script would have found. Link search and library
/// names stay the store's job (those propagate graph-wide); only the
/// package-scoped rpath has to be repeated here.
#[cfg(feature = "kyotocabinet")]
fn emit_kc_rpath() {
    if let Ok(dir) = std::env::var("OXPINYIN_KC_LIB_DIR") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
    }
    // Only the directories are wanted: the store links the library, so the
    // probe emits no link metadata. System directories stay out, as in the
    // store, so no rpath names one.
    if let Ok(library) = pkg_config::Config::new()
        .cargo_metadata(false)
        .probe_cflags(false)
        .print_system_libs(false)
        .probe("kyotocabinet")
    {
        for path in &library.link_paths {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", path.display());
        }
    }
}
