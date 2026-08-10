#![allow(missing_docs)]
/// Build script: compiles the C++ bridge against libtkrzw.
///
/// Uses pkg-config only for include paths; links only tkrzw + stdc++
/// to avoid pulling in transitive deps (lz4, lzma, etc.) that may
/// lack -devel packages.
fn main() {
    // Probe tkrzw for include paths and link search paths.
    // We intentionally do NOT use the full probe() output because it
    // emits link directives for transitive C libs that may lack .so
    // symlinks (only .so.N).  Those are resolved at runtime via
    // libtkrzw.so's NEEDED entries.
    let lib = pkg_config::Config::new()
        .atleast_version("1.0.0")
        .cargo_metadata(false)
        .probe("tkrzw")
        .expect("libtkrzw not found via pkg-config; install tkrzw-devel");

    let mut build = cc::Build::new();
    build.cpp(true).file("src/bridge.cpp").std("c++17");

    for path in &lib.include_paths {
        build.include(path);
    }

    build.compile("tkrzw_bridge");

    // Link only what the bridge directly needs.
    for path in &lib.link_paths {
        println!("cargo:rustc-link-search=native={}", path.display());
    }
    println!("cargo:rustc-link-lib=tkrzw");
    println!("cargo:rustc-link-lib=stdc++");

    // With oracle-ffi the binary links the pinned libpinyin through
    // pinyin-oracle's build script, but that script's rpath argument does
    // not propagate to this crate's binary: without its own rpath the
    // dynamic loader would resolve libpinyin.so.15 from the system and the
    // exporter would drive the wrong library. Mirror the prefix discovery
    // order of pinyin-oracle's build.rs.
    println!("cargo::rerun-if-env-changed=PINYIN_ORACLE_PREFIX");
    if std::env::var_os("CARGO_FEATURE_ORACLE_FFI").is_some() {
        let prefix = std::env::var_os("PINYIN_ORACLE_PREFIX")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("PKG_CONFIG_PATH").and_then(|raw| {
                    std::env::split_paths(&raw).find_map(|entry| {
                        let root = entry.parent()?.parent()?.to_path_buf();
                        has_libpinyin(&root).then_some(root)
                    })
                })
            })
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| std::path::PathBuf::from(home).join(".local/opt/pinyin-oracle"))
            });
        let lib_dir = prefix.and_then(|prefix| {
            ["lib", "lib64"]
                .into_iter()
                .map(|name| prefix.join(name))
                .find(|dir| dir.join("libpinyin.so").exists())
        });
        if let Some(lib_dir) = lib_dir {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());
        }
    }
}

/// Whether `prefix` holds a libpinyin shared object under lib or lib64.
fn has_libpinyin(prefix: &std::path::Path) -> bool {
    ["lib", "lib64"]
        .into_iter()
        .any(|name| prefix.join(name).join("libpinyin.so").exists())
}
