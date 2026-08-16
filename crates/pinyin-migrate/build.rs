#![allow(missing_docs)]
/// Build script: compiles the C++ bridges.
///
/// - `bridge.cpp` compiles unconditionally and links libtkrzw (the raw
///   Tkrzw HashDBM reader behind `convert`).
/// - `storage_dump.cpp` compiles only with `oracle-ffi` and links the pinned
///   `libstorage.a` convenience archive (the legacy user-data dump shim
///   behind `migrate`), plus glib-2.0.
///
/// Uses pkg-config only for include paths; links only what each bridge
/// directly needs to avoid pulling in transitive deps (lz4, lzma, etc.)
/// that may lack -devel packages.

fn main() {
    // --- tkrzw bridge (always) ---
    let tkrzw = pkg_config::Config::new()
        .atleast_version("1.0.0")
        .cargo_metadata(false)
        .probe("tkrzw")
        .expect("libtkrzw not found via pkg-config; install tkrzw-devel");

    let mut bridge = cc::Build::new();
    bridge.cpp(true).file("src/bridge.cpp").std("c++17");
    for path in &tkrzw.include_paths {
        bridge.include(path);
    }
    bridge.compile("tkrzw_bridge");

    for path in &tkrzw.link_paths {
        println!("cargo:rustc-link-search=native={}", path.display());
    }
    println!("cargo:rustc-link-lib=tkrzw");
    println!("cargo:rustc-link-lib=stdc++");

    // --- storage shim (oracle-ffi only) ---
    println!("cargo::rerun-if-env-changed=PINYIN_ORACLE_PREFIX");
    println!("cargo::rerun-if-env-changed=PKG_CONFIG_PATH");
    if std::env::var_os("CARGO_FEATURE_ORACLE_FFI").is_none() {
        return;
    }

    // The pin itself is verified by pinyin-oracle's build.rs, which runs
    // first (oracle-ffi implies pinyin-oracle/oracle-ffi). Here we only need
    // to locate the same prefix and find the internal headers and archive
    // that `make install` does not install by default.
    let prefix = locate_prefix()
        .unwrap_or_else(|reason| fail(&reason));

    let lib_dir = storage_lib_dir(&prefix).unwrap_or_else(|| {
        fail(&format!(
            "no libstorage.a under {}/lib or {}/lib64",
            prefix.display(),
            prefix.display()
        ))
    });
    let archive = lib_dir.join("libstorage.a");
    let include_dir = internal_include_dir(&prefix).unwrap_or_else(|| {
        fail(&format!(
            "no internal storage headers under {}/include/libpinyin-*",
            prefix.display()
        ))
    });

    // glib-2.0 headers (g_array_new, g_ucs4_to_utf8, g_free).
    let glib = pkg_config::Config::new()
        .atleast_version("2.0")
        .cargo_metadata(false)
        .probe("glib-2.0")
        .expect("glib-2.0 not found via pkg-config; install glib2-devel");

    let mut shim = cc::Build::new();
    shim.cpp(true).file("src/storage_dump.cpp").std("c++17");
    shim.include(&include_dir);
    for path in &glib.include_paths {
        shim.include(path);
    }
    for path in &tkrzw.include_paths {
        shim.include(path);
    }
    shim.compile("storage_dump_shim");

    // Wrap the archive in a linker group so its internal object cross-
    // references resolve in a single pass; glib-2.0 (needed only by the
    // archive, not by bridge.cpp) must follow the group, so it is emitted
    // as a raw link arg rather than via `rustc-link-lib`. tkrzw and stdc++
    // are already linked above and are kept by bridge.cpp's own references.
    println!("cargo:rustc-link-arg=-Wl,--start-group");
    println!("cargo:rustc-link-arg={}", archive.display());
    println!("cargo:rustc-link-arg=-Wl,--end-group");
    println!("cargo:rustc-link-arg=-lglib-2.0");

    // The pinned `libstorage.a` is a convenience archive of non-PIC objects
    // (the oracle builds it to assemble `libpinyin.so`, which is PIC, but the
    // archive members themselves are not). Linking them into Rust's default
    // PIE binary fails with R_X86_64_32 relocations, so disable PIE for this
    // one binary. Migration is Linux-first and oracle-gated, so a non-PIE
    // tool binary is acceptable here.
    println!("cargo:rustc-link-arg=-no-pie");

    // rpath so the binary finds the pinned libpinyin.so.15 for `export`;
    // pinyin-oracle's own rpath argument does not propagate to this binary.
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());
}

fn fail(reason: &str) -> ! {
    eprintln!("error: {reason}");
    eprintln!();
    eprintln!("The `oracle-ffi` feature links the pinned libpinyin and, for `migrate`,");
    eprintln!("the internal libstorage.a archive plus its headers. Build the oracle first:");
    eprintln!();
    eprintln!(
        "  bash tools/oracle/build-oracle.sh \\\n    --prefix \"$HOME/.local/opt/pinyin-oracle\" --jobs \"$(nproc)\""
    );
    eprintln!();
    eprintln!("  PINYIN_ORACLE_PREFIX=$HOME/.local/opt/pinyin-oracle \\");
    eprintln!("  cargo test -p pinyin-migrate --features oracle-ffi");
    std::process::exit(1);
}

/// Default prefix suffix under `$HOME`, mirrored from pinyin-oracle.
const DEFAULT_PREFIX_SUFFIX: &str = ".local/opt/pinyin-oracle";

/// Locates the oracle prefix.
///
/// Order: `PINYIN_ORACLE_PREFIX`, then each `PKG_CONFIG_PATH` entry walked up
/// to its prefix root, then `$HOME/.local/opt/pinyin-oracle`.
fn locate_prefix() -> Result<std::path::PathBuf, String> {
    if let Some(raw) = std::env::var_os("PINYIN_ORACLE_PREFIX") {
        return Ok(std::path::PathBuf::from(raw));
    }

    if let Some(raw) = std::env::var_os("PKG_CONFIG_PATH") {
        for entry in std::env::split_paths(&raw) {
            // .../lib/pkgconfig or .../lib64/pkgconfig -> prefix root
            if let Some(candidate) = entry.parent().and_then(std::path::Path::parent) {
                if storage_lib_dir(&candidate).is_some() {
                    return Ok(candidate.to_path_buf());
                }
            }
        }
    }

    if let Some(home) = std::env::var_os("HOME") {
        let candidate = std::path::PathBuf::from(home).join(DEFAULT_PREFIX_SUFFIX);
        if storage_lib_dir(&candidate).is_some() {
            return Ok(candidate);
        }
    }

    Err("no oracle prefix found (PINYIN_ORACLE_PREFIX unset)".to_owned())
}

/// Returns the prefix-local directory holding `libstorage.a`.
fn storage_lib_dir(prefix: &std::path::Path) -> Option<std::path::PathBuf> {
    ["lib", "lib64"].into_iter().find_map(|name| {
        let dir = prefix.join(name);
        dir.join("libstorage.a").exists().then_some(dir)
    })
}

/// Returns the versioned internal include directory (`include/libpinyin-*`).
fn internal_include_dir(prefix: &std::path::Path) -> Option<std::path::PathBuf> {
    let include = prefix.join("include");
    std::fs::read_dir(&include)
        .ok()?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("libpinyin-"))
        })
}
