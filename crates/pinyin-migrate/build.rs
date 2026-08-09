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

    // When oracle-ffi is enabled, also link the pin-built libpinyin and glib
    // for the phrase_index conversion that uses pinyin_token_get_phrase.
    if std::env::var("CARGO_FEATURE_ORACLE_FFI").is_ok() {
        // Use the same prefix discovery as pinyin-oracle's build.rs
        let prefix = std::env::var("PINYIN_ORACLE_PREFIX")
            .map(std::path::PathBuf::from)
            .ok()
            .or_else(|| {
                std::env::var("PKG_CONFIG_PATH").ok().and_then(|s| {
                    std::env::split_paths(&s)
                        .find_map(|p| p.parent().and_then(|p| p.parent()).map(|p| p.to_path_buf()))
                })
            })
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| std::path::PathBuf::from(h).join(".local/opt/pinyin-oracle"))
            });
        if let Some(prefix) = prefix {
            let lib_dir = ["lib", "lib64"]
                .iter()
                .map(|n| prefix.join(n))
                .find(|p| p.join("libpinyin.so").exists() || p.join("libpinyin.so.15").exists())
                .unwrap_or(prefix.join("lib"));
            if lib_dir.exists() {
                println!("cargo:rustc-link-search=native={}", lib_dir.display());
                println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());
            }
        }
        println!("cargo:rustc-link-lib=pinyin");
        println!("cargo:rustc-link-lib=glib-2.0");
        // Also need to find glib via pkg-config for rpath
        if let Ok(glib) = pkg_config::Config::new()
            .cargo_metadata(false)
            .probe("glib-2.0")
        {
            for path in glib.link_paths {
                println!("cargo:rustc-link-search=native={}", path.display());
            }
        }
    }
}
