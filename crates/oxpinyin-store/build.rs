//! Builds the tkrzw C++ shim when the `tkrzw` feature is on.
//!
//! Off by default: with the feature disabled this script does nothing,
//! so the default build compiles no C++ and links no extra library.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "tkrzw")]
    tkrzw::build();
}

#[cfg(feature = "tkrzw")]
mod tkrzw {
    use std::process::Command;

    /// Asks `pkg-config` for one field of the `tkrzw` package.
    ///
    /// Shelling out rather than taking a `pkg-config` crate dependency:
    /// the two calls below are all this script needs.
    fn pkg_config(flag: &str) -> Option<Vec<String>> {
        let output = Command::new("pkg-config")
            .arg(flag)
            .arg("tkrzw")
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        Some(
            String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        )
    }

    pub fn build() {
        println!("cargo:rerun-if-changed=src/tkrzw/shim.cc");
        println!("cargo:rerun-if-changed=src/tkrzw/shim.h");
        println!("cargo:rerun-if-changed=src/tkrzw/bridge.rs");
        // The pkg-config lookup below decides the include path, the
        // link path and the embedded rpath; repointing it at a
        // different tkrzw installation must rerun this script, not
        // reuse the cached flags.
        println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");

        let Some(cflags) = pkg_config("--cflags") else {
            panic!(
                "libtkrzw required: the `tkrzw` feature needs the tkrzw C++ library and its \
                 headers, and `pkg-config --cflags tkrzw` could not find them. Build tkrzw \
                 from source (https://dbmx.net/tkrzw/: ./configure --prefix=DIR && make && \
                 make install) and put DIR/lib/pkgconfig on PKG_CONFIG_PATH, or build \
                 without --features tkrzw.\n\n\
                 Do not use any Ubuntu libtkrzw-dev package. Ubuntu builds every package \
                 with two flags that each break tkrzw's pointer-identity protocol, \
                 independently and silently. -flto duplicates the NOOP/REMOVE backing \
                 literals across LTO partitions, so removals store \
                 DBM::RecordProcessor::REMOVE as the record's value and a NOOP processor \
                 stores NOOP's bytes; -Wl,-Bsymbolic-functions resolves libtkrzw's \
                 comparator references to its own copies, so tkrzw_dbm_util cannot reopen \
                 a TreeDBM it created (BROKEN_DATA_ERROR: invalid_key_comparator). \
                 Confirmed on noble's 1.0.27-1.1build1 and resolute's 1.0.32-1build1. \
                 Debian enables neither and is correct, as is any ./configure && make \
                 build. Arch enables LTO only, so it is expected to have the first fault \
                 and not the second. Run tools/tkrzw/distro-probe.sh against a candidate; \
                 it checks both. Tracked as Ubuntu LP #2142937, whose attached patch \
                 disables LTO only and so fixes just the first; see \
                 docs/findings/tkrzw-distro-compat.md."
            );
        };
        let Some(libs) = pkg_config("--libs") else {
            panic!(
                "libtkrzw required: `pkg-config --cflags tkrzw` succeeded but \
                 `pkg-config --libs tkrzw` did not; the tkrzw installation looks incomplete."
            );
        };

        let mut build = cxx_build::bridge("src/tkrzw/bridge.rs");
        build.file("src/tkrzw/shim.cc").std("c++17");
        for flag in &cflags {
            // tkrzw's headers are included as system headers so their
            // own warnings (unused parameters in inline overrides, and
            // the like) do not drown out warnings about the shim.
            match flag.strip_prefix("-I") {
                Some(dir) => build.flag(format!("-isystem{dir}")),
                None => build.flag(flag),
            };
        }
        build.compile("oxpinyin_tkrzw_shim");

        for lib in &libs {
            if let Some(name) = lib.strip_prefix("-l") {
                println!("cargo:rustc-link-lib={name}");
            } else if let Some(path) = lib.strip_prefix("-L") {
                println!("cargo:rustc-link-search=native={path}");
                // A tkrzw outside the default loader path — the usual
                // case, since a correct build often has to be made by
                // hand — would otherwise link but fail to start.
                //
                // The unscoped rustc-link-arg reaches every binary
                // cargo links in this graph (workspace bins, tests,
                // benches), so they run without environment setup.
                // That rpath is a convenience, not the contract: code
                // that consumes the shim outside such a build, or
                // strips runpaths from its artifacts, must make the
                // library findable itself via LD_LIBRARY_PATH or its
                // own rpath setting.
                println!("cargo:rustc-link-arg=-Wl,-rpath,{path}");
            }
        }
    }
}
