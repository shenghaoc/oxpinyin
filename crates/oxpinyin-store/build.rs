//! Generates the optional backends' bindings.
//!
//! Both are off by default: with neither feature enabled this script does
//! nothing, so the default build runs no bindgen and links no extra
//! library.
//!
//! * `tkrzw` — the tkrzw C API (`tkrzw_langc.h`).
//! * `kyotocabinet` — the Kyoto Cabinet C API (`kclangc.h`).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "tkrzw")]
    tkrzw::build();
    #[cfg(feature = "kyotocabinet")]
    kyotocabinet::build();
}

/// Asks `pkg-config` for one field of `package`, split into individual
/// flags. Shared by both backend modules; shelling out avoids a
/// `pkg-config` crate dependency for the handful of calls this script makes.
/// Kyoto Cabinet does not always install a `.pc` file, so a miss is not
/// fatal there — the caller falls back to the library name.
#[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
fn pkg_config(flag: &str, package: &str) -> Option<Vec<String>> {
    let output = std::process::Command::new("pkg-config")
        .arg(flag)
        .arg(package)
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

/// Kyoto Cabinet bindings.
///
/// # Why the C API and not the C++ classes
///
/// Kyoto Cabinet is C++ internally — libpinyin instantiates
/// `kyotocabinet::HashDB` and `kyotocabinet::TreeDB` directly — but it
/// ships a complete C API in `kclangc.h`, so bindgen reads a C header and
/// no `cxx` bridge is needed. The C API is the polymorphic `PolyDB`,
/// which has one consequence that shapes the whole backend; see
/// `src/kyotocabinet/mod.rs` on the `#type=` path suffix.
///
/// # Why generated fresh, not checked in
///
/// A weaker argument than the Berkeley DB backend's, and worth stating
/// honestly: `KCDB` and `KCCUR` are **opaque** one-pointer wrappers
/// (`kclangc.h:48-58`), so unlike Berkeley DB's `DB`/`DBT`/`DBC` there is
/// no exposed struct layout to get wrong, and a checked-in binding could
/// not silently misread a field. What is still baked from the header are
/// the open-mode `enum` constants (`KCOREADER`, `KCOWRITER`, `KCOCREATE`,
/// …) — stable across Kyoto Cabinet's life, but values a checked-in
/// binding would carry from the machine that generated them to a machine
/// with a different library.
///
/// Generating keeps the declarations and the linked library in lockstep
/// by construction, which is what makes the `KCVERSION` gate meaningful
/// at all. The cost is a build-time libclang, and it is small here
/// because linking already requires the development package that carries
/// the header: only libclang is added, and only for an opt-in feature.
#[cfg(feature = "kyotocabinet")]
mod kyotocabinet {
    use std::path::PathBuf;

    pub fn build() {
        println!("cargo:rerun-if-changed=src/kyotocabinet/wrapper.h");
        println!("cargo:rerun-if-env-changed=BINDGEN_EXTRA_CLANG_ARGS");
        println!("cargo:rerun-if-env-changed=OXPINYIN_KC_INCLUDE_DIR");
        println!("cargo:rerun-if-env-changed=OXPINYIN_KC_LIB_DIR");
        println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");

        let mut clang_args: Vec<String> = Vec::new();
        if let Some(cflags) = super::pkg_config("--cflags", "kyotocabinet") {
            clang_args.extend(cflags);
        }
        if let Ok(dir) = std::env::var("OXPINYIN_KC_INCLUDE_DIR") {
            clang_args.push(format!("-I{dir}"));
        }

        if let Ok(dir) = std::env::var("OXPINYIN_KC_LIB_DIR") {
            println!("cargo:rustc-link-search=native={dir}");
            println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
        }
        match super::pkg_config("--libs", "kyotocabinet") {
            Some(libs) => {
                for lib in &libs {
                    if let Some(name) = lib.strip_prefix("-l") {
                        println!("cargo:rustc-link-lib={name}");
                    } else if let Some(path) = lib.strip_prefix("-L") {
                        println!("cargo:rustc-link-search=native={path}");
                        println!("cargo:rustc-link-arg=-Wl,-rpath,{path}");
                    }
                }
            }
            // No .pc file: name the library directly. A distro package
            // puts it on the default search path.
            None => println!("cargo:rustc-link-lib=kyotocabinet"),
        }

        let mut builder = bindgen::Builder::default()
            .header("src/kyotocabinet/wrapper.h")
            // Only the surface this backend uses. `kclangc.h` is the whole
            // polymorphic API; allowlisting keeps the generated file to
            // what is actually called and makes an accidental new
            // dependency visible as a compile error.
            .allowlist_function("kcdbnew")
            .allowlist_function("kcdbopen")
            .allowlist_function("kcdbclose")
            .allowlist_function("kcdbdel")
            .allowlist_function("kcdbget")
            .allowlist_function("kcdbset")
            .allowlist_function("kcdbremove")
            .allowlist_function("kcdbsync")
            .allowlist_function("kcdbcount")
            .allowlist_function("kcdbcursor")
            .allowlist_function("kcdbbegintran")
            .allowlist_function("kcdbendtran")
            .allowlist_function("kccurecode")
            .allowlist_function("kccurdel")
            .allowlist_function("kccurjump")
            .allowlist_function("kccurjumpkey")
            .allowlist_function("kccurstep")
            .allowlist_function("kccurget")
            .allowlist_function("kcdbecode")
            .allowlist_function("kcdbemsg")
            .allowlist_function("kcecodename")
            .allowlist_function("kcfree")
            .allowlist_type("KCDB")
            .allowlist_type("KCCUR")
            .allowlist_var("KCVERSION")
            .allowlist_var("KCO.*")
            .allowlist_var("KCE.*")
            .layout_tests(false)
            .derive_debug(false)
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));

        for arg in clang_args {
            builder = builder.clang_arg(arg);
        }

        let bindings = match builder.generate() {
            Ok(bindings) => bindings,
            Err(error) => panic!(
                "libkyotocabinet required: the `kyotocabinet` feature needs Kyoto \
                 Cabinet and its C header kclangc.h, and bindgen could not read them \
                 ({error}). Install the distro's development package (Debian/Ubuntu: \
                 libkyotocabinet-dev; Fedora: kyotocabinet-devel), or point \
                 OXPINYIN_KC_INCLUDE_DIR and OXPINYIN_KC_LIB_DIR at an installation. \
                 Generating these declarations also needs libclang (Debian/Ubuntu: \
                 libclang-dev). Build without --features kyotocabinet to skip all of it."
            ),
        };

        let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
        bindings
            .write_to_file(out.join("kc_bindings.rs"))
            .expect("write generated Kyoto Cabinet declarations");
    }
}

#[cfg(feature = "tkrzw")]
mod tkrzw {
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

        let Some(cflags) = super::pkg_config("--cflags", "tkrzw") else {
            panic!(
                "libtkrzw required: the `tkrzw` feature needs the tkrzw library with its \
                 C API header tkrzw_langc.h, and `pkg-config --cflags tkrzw` could not find \
                 them. Build tkrzw from source (https://dbmx.net/tkrzw/: ./configure \
                 --prefix=DIR && make && make install) and put DIR/lib/pkgconfig on \
                 PKG_CONFIG_PATH, or build without --features tkrzw.\n\n\
                 Do not use any Ubuntu libtkrzw-dev package. Ubuntu applies two build \
                 flags that each break tkrzw independently, silently, and in different \
                 ways; Debian applies neither, and neither fixes the other. (1) -flto \
                 duplicates the RecordProcessor NOOP/REMOVE backing literals across LTO \
                 partitions, so Remove() stores a tombstone instead of deleting and a NOOP \
                 processor overwrites the record. (2) -Wl,-Bsymbolic-functions resolves \
                 libtkrzw's references to its own copies of the key comparators, so a \
                 TreeDBM records comparator type 255 and can never be reopened \
                 (BROKEN_DATA_ERROR: invalid_key_comparator). Confirmed on noble's \
                 1.0.27-1.1build1 and resolute's 1.0.32-1build1. Arch enables LTO only, so \
                 it has defect 1 and not defect 2. Ubuntu LP #2142937 carries a patch that \
                 disables LTO: it resolves defect 1 and leaves defect 2 exactly as it was. \
                 Check a candidate with tools/tkrzw/distro-probe.sh, which tests both. See \
                 docs/findings/tkrzw-distro-compat.md."
            );
        };
        let Some(libs) = super::pkg_config("--libs", "tkrzw") else {
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
