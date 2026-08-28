//! Generates the tkrzw C API bindings when the `tkrzw` feature is on.
//!
//! Off by default: with the feature disabled this script does nothing,
//! so the default build runs no bindgen and links no extra library.

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

    /// Locates `tkrzw_langc.h` under the discovered include path, falling
    /// back to the compiler's default include directory. The C API is the
    /// only header this build may bind: no C++ header and no other tkrzw
    /// API crosses the ABI.
    fn langc_header(cflags: &[String]) -> Option<std::path::PathBuf> {
        let mut dirs: Vec<std::path::PathBuf> = cflags
            .iter()
            .filter_map(|flag| flag.strip_prefix("-I"))
            .map(std::path::PathBuf::from)
            .collect();
        dirs.push(std::path::PathBuf::from("/usr/include"));
        dirs.into_iter()
            .map(|dir| dir.join("tkrzw_langc.h"))
            .find(|path| path.is_file())
    }

    pub fn build() {
        // The pkg-config lookup below decides the include path, the link
        // path and the embedded rpath; repointing it at a different tkrzw
        // installation must rerun this script, not reuse cached flags.
        // PKG_CONFIG_LIBDIR replaces the search directory list outright
        // and PKG_CONFIG_SYSROOT_DIR rewrites every discovered path, so
        // either one alone can select a different installation.
        println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
        println!("cargo:rerun-if-env-changed=PKG_CONFIG_LIBDIR");
        println!("cargo:rerun-if-env-changed=PKG_CONFIG_SYSROOT_DIR");

        let Some(cflags) = pkg_config("--cflags") else {
            panic!(
                "libtkrzw required: the `tkrzw` feature needs the tkrzw library with its \
                 C API header tkrzw_langc.h, and `pkg-config --cflags tkrzw` could not find \
                 them. Build tkrzw from source (https://dbmx.net/tkrzw/: ./configure \
                 --prefix=DIR && make && make install) and put DIR/lib/pkgconfig on \
                 PKG_CONFIG_PATH, or build without --features tkrzw.\n\n\
                 Do not use any Ubuntu libtkrzw-dev package. Ubuntu links every package \
                 with -Wl,-Bsymbolic-functions, which resolves libtkrzw's own references to \
                 its own copies of the comparators and so breaks tkrzw's pointer-identity \
                 protocol: removals store DBM::RecordProcessor::REMOVE as the record's \
                 value, a NOOP processor stores NOOP's bytes, Rebuild fails with \
                 CANCELED_ERROR, and tkrzw_dbm_util cannot reopen a TreeDBM it created \
                 (BROKEN_DATA_ERROR: invalid_key_comparator). Confirmed on noble's \
                 1.0.27-1.1build1 and resolute's 1.0.32-1build1; the same sources built \
                 from source, or with Debian's flags, are correct. Check any candidate \
                 with `readelf -rW .../libtkrzw.so.1 | grep -c KeyComparator` -- zero means \
                 broken. See docs/findings/tkrzw-distro-compat.md."
            );
        };
        let Some(libs) = pkg_config("--libs") else {
            panic!(
                "libtkrzw required: `pkg-config --cflags tkrzw` succeeded but \
                 `pkg-config --libs tkrzw` did not; the tkrzw installation looks incomplete."
            );
        };
        let Some(header) = langc_header(&cflags) else {
            panic!(
                "libtkrzw required: `pkg-config --cflags tkrzw` found include flags but no \
                 tkrzw_langc.h under them. The tkrzw backend binds only the plain-C API, so \
                 the installation must ship that header."
            );
        };
        println!("cargo:rerun-if-changed={}", header.display());

        // Exactly the entry points the backend's safe wrapper calls, and
        // the types they traffic in. Anything else in tkrzw_langc.h — the
        // async adapter, the index API, the string utilities — stays
        // unbound: an unbound API cannot be misused.
        let bindings = bindgen::Builder::default()
            .header(header.to_string_lossy())
            .allowlist_function("tkrzw_dbm_open")
            .allowlist_function("tkrzw_dbm_close")
            .allowlist_function("tkrzw_dbm_process")
            .allowlist_function("tkrzw_dbm_process_multi")
            .allowlist_function("tkrzw_dbm_synchronize")
            .allowlist_function("tkrzw_dbm_rebuild")
            .allowlist_function("tkrzw_dbm_make_iterator")
            .allowlist_function("tkrzw_dbm_iter_free")
            .allowlist_function("tkrzw_dbm_iter_jump")
            .allowlist_function("tkrzw_dbm_iter_process")
            .allowlist_function("tkrzw_dbm_iter_next")
            .allowlist_function("tkrzw_get_last_status")
            .allowlist_type("TkrzwDBM")
            .allowlist_type("TkrzwDBMIter")
            .allowlist_type("TkrzwStatus")
            .allowlist_type("TkrzwKeyProcPair")
            .allowlist_type("tkrzw_record_processor")
            .allowlist_var("TKRZW_REC_PROC_NOOP")
            .allowlist_var("TKRZW_REC_PROC_REMOVE")
            .allowlist_var("TKRZW_STATUS_SUCCESS")
            .allowlist_var("TKRZW_STATUS_SYSTEM_ERROR")
            .allowlist_var("TKRZW_STATUS_NOT_FOUND_ERROR")
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
            .generate()
            .expect("bindgen over tkrzw_langc.h must succeed");
        let out_dir = std::env::var("OUT_DIR").unwrap();
        bindings
            .write_to_file(std::path::Path::new(&out_dir).join("tkrzw_langc.rs"))
            .expect("writing the generated tkrzw bindings must succeed");

        for lib in &libs {
            if let Some(name) = lib.strip_prefix("-l") {
                println!("cargo:rustc-link-lib={name}");
            } else if let Some(path) = lib.strip_prefix("-L") {
                println!("cargo:rustc-link-search=native={path}");
                // A tkrzw outside the default loader path — the usual
                // case, since the library often has to be made by hand —
                // would otherwise link but fail to start.
                //
                // The unscoped rustc-link-arg reaches every binary cargo
                // links in this graph (workspace bins, tests, benches),
                // so they run without environment setup. That rpath is a
                // convenience, not the contract: code that consumes the
                // backend outside such a build, or strips runpaths from
                // its artifacts, must make the library findable itself
                // via LD_LIBRARY_PATH or its own rpath setting.
                println!("cargo:rustc-link-arg=-Wl,-rpath,{path}");
            }
        }
    }
}
