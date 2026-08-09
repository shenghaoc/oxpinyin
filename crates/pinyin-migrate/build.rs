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
}
