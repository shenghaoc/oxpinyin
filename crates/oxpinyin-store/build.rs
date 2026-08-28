//! Builds the optional backends' native glue.
//!
//! Both are off by default: with neither feature enabled this script does
//! nothing, so the default build compiles no C or C++ and links no extra
//! library.
//!
//! * `tkrzw` — compiles the C++ shim through `cxx-build`.
//! * `bdb` — generates Rust declarations from the system `db.h` with
//!   bindgen and links the system `libdb`.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "tkrzw")]
    tkrzw::build();
    #[cfg(feature = "bdb")]
    bdb::build();
}

/// Berkeley DB 5.3 bindings.
///
/// # Why bindgen and not a crate
///
/// The published `libdb`/`libdb-sys` crates statically link a vendored
/// Berkeley DB. That is the wrong shape here: the whole point of this
/// backend is to interoperate with files the user's own libpinyin wrote
/// through the *system* libdb, so the declarations must describe the
/// library actually installed, and the link must name it.
///
/// # Why generated fresh, not checked in
///
/// `DB`, `DBT` and `DBC` are ABI structs whose layout is version-specific.
/// A checked-in `bindings.rs` would freeze 5.3.28's layout and silently
/// misread the fields of any other libdb a distro might ship — writing
/// through a struct whose fields have moved corrupts the user's profile
/// without any error, which is precisely the failure this backend must
/// not have. Generating from the installed header makes the layout right
/// by construction, and the version gate below refuses anything the
/// format survey did not cover instead of guessing.
///
/// The cost is a build-time libclang, which is acceptable because this
/// backend is opt-in: a build without `--features bdb` needs neither
/// bindgen nor libdb.
#[cfg(feature = "bdb")]
mod bdb {
    use std::path::PathBuf;

    pub fn build() {
        println!("cargo:rerun-if-changed=src/bdb/wrapper.h");
        // Repointing the build at a different libdb must regenerate the
        // declarations, not reuse the cached ones.
        println!("cargo:rerun-if-env-changed=BINDGEN_EXTRA_CLANG_ARGS");
        println!("cargo:rerun-if-env-changed=OXPINYIN_BDB_INCLUDE_DIR");
        println!("cargo:rerun-if-env-changed=OXPINYIN_BDB_LIB_DIR");

        if let Ok(dir) = std::env::var("OXPINYIN_BDB_LIB_DIR") {
            println!("cargo:rustc-link-search=native={dir}");
            println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
        }
        // The system Berkeley DB, linked by name. Never a vendored copy:
        // the files being read were written by this same library.
        println!("cargo:rustc-link-lib=db");

        let mut builder = bindgen::Builder::default()
            .header("src/bdb/wrapper.h")
            // Only the small surface libpinyin itself uses. Every `open`
            // below passes NULL for both environment and transaction, so
            // no transaction, environment or secondary-index type is
            // needed and none is generated.
            .allowlist_function("db_create")
            .allowlist_function("db_strerror")
            .allowlist_function("db_version")
            .allowlist_type("DB")
            .allowlist_type("DBC")
            .allowlist_type("DBT")
            .allowlist_type("DBTYPE")
            .allowlist_var("DB_.*")
            .allowlist_var("DB_VERSION_.*")
            // No layout tests: they would compile a second copy of every
            // struct into the crate's test binary for no benefit here,
            // and this crate's tests exercise the real library instead.
            .layout_tests(false)
            .derive_debug(false)
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));

        if let Ok(dir) = std::env::var("OXPINYIN_BDB_INCLUDE_DIR") {
            builder = builder.clang_arg(format!("-I{dir}"));
        }

        let bindings = match builder.generate() {
            Ok(bindings) => bindings,
            Err(error) => panic!(
                "libdb required: the `bdb` feature needs Berkeley DB 5.3 and its header \
                 db.h, and bindgen could not read them ({error}). Install the distro's \
                 development package (Debian/Ubuntu: libdb5.3-dev; Fedora: libdb-devel; \
                 Arch: db) — the same one libpinyin build-depends on — or point \
                 OXPINYIN_BDB_INCLUDE_DIR and OXPINYIN_BDB_LIB_DIR at an installation. \
                 Generating these declarations also needs libclang (Debian/Ubuntu: \
                 libclang-dev). Build without --features bdb to skip all of it."
            ),
        };

        let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
        bindings
            .write_to_file(out.join("bdb_bindings.rs"))
            .expect("write generated Berkeley DB declarations");
    }
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
