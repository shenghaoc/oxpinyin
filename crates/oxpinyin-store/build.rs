//! Generates the C backends' bindings.
//!
//! tkrzw is the workspace's default selected backend, so a normal
//! build runs bindgen over `tkrzw_langc.h` and links libtkrzw. With
//! `--no-default-features` — and with no C backend feature enabled —
//! this script does nothing: no bindgen, no extra library. Selecting a
//! peer backend explicitly (`--no-default-features --features
//! {kyotocabinet|bdb}`) skips the C-binding step for the ones
//! it does not build.
//!
//! * `kyotocabinet` — the C entry points (`shim.h`) of a C++ shim over
//!   Kyoto Cabinet's classes, which this script also compiles with `cc`.
//! * `tkrzw` — the tkrzw C API (`tkrzw_langc.h`), on by default.
//! * `bdb` — the Berkeley DB C API (`db.h`), from the system libdb.
//!
//! # Discovery
//!
//! `system-deps` is the one discovery layer. The backends are declared in
//! this package's `[package.metadata.system-deps]`, and the
//! `SYSTEM_DEPS_<NAME>_*` variables are the only overrides
//! (`docs/runbooks/backends.md`). It emits the link-search and link-lib
//! directives. This script reads the resolved include and link paths from
//! it for what `system-deps` does not configure: bindgen's and `cc`'s
//! include paths, and the rpath.
//!
//! Each backend is found as libpinyin's `configure.ac` finds it: tkrzw and
//! Kyoto Cabinet through their `.pc` file (`PKG_CHECK_MODULES`), Berkeley
//! DB without pkg-config (`AC_CHECK_HEADER([db.h])` and
//! `AC_SEARCH_LIBS([db_create], [db])`).

#[cfg(any(feature = "tkrzw", feature = "kyotocabinet", feature = "bdb"))]
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "bdb")]
    bdb::pin_defaults();
    let dependencies = probe();
    #[cfg(feature = "tkrzw")]
    tkrzw::build(&dependencies);
    #[cfg(feature = "kyotocabinet")]
    kyotocabinet::build(&dependencies);
    #[cfg(feature = "bdb")]
    bdb::build(&dependencies);
}

/// With no C backend selected there is nothing to find or bind.
#[cfg(not(any(feature = "tkrzw", feature = "kyotocabinet", feature = "bdb")))]
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
}

/// Probes the selected backend's `[package.metadata.system-deps]` entry.
/// A failure is fatal: no backend has a silent fallback.
#[cfg(any(feature = "tkrzw", feature = "kyotocabinet", feature = "bdb"))]
fn probe() -> system_deps::Dependencies {
    system_deps::Config::new().probe().unwrap_or_else(|error| {
        let hint = if cfg!(feature = "tkrzw") {
            "libtkrzw required: the `tkrzw` feature (the default backend) finds tkrzw \
             through pkg-config. Make tkrzw.pc visible to pkg-config (PKG_CONFIG_PATH), or \
             select another backend with `--no-default-features --features bdb` or \
             `--features kyotocabinet`."
        } else if cfg!(feature = "kyotocabinet") {
            "the `kyotocabinet` feature finds Kyoto Cabinet through kyotocabinet.pc, as \
             libpinyin's configure does (PKG_CHECK_MODULES). Install its development \
             package (Debian/Ubuntu: libkyotocabinet-dev; Fedora: kyotocabinet-devel), or, \
             for an installation with no .pc file, set \
             SYSTEM_DEPS_KYOTOCABINET_NO_PKG_CONFIG=1 and SYSTEM_DEPS_KYOTOCABINET_LIB=kyotocabinet \
             (with _SEARCH_NATIVE and _INCLUDE for a non-default prefix); see \
             docs/runbooks/backends.md."
        } else {
            "the `bdb` feature links the system Berkeley DB without pkg-config; see \
             docs/runbooks/backends.md for the SYSTEM_DEPS_BDB_* variables."
        };
        panic!("native dependency discovery failed ({error}). {hint}")
    })
}

/// Kyoto Cabinet: a C++ shim and the bindgen declarations of its C entry
/// points.
///
/// # A shim over the classes, not `kclangc.h`
///
/// Kyoto Cabinet's database classes are header code. libpinyin constructs
/// `TreeDB`, `HashDB`, `ProtoTreeDB` and `StashDB` itself and compiles them
/// from the headers into its own text. `src/kyotocabinet/shim.cc` does the
/// same, behind a small C API of its own (`shim.h`). Its entry points are
/// `noexcept`: a C++ exception is always caught there and turned into the
/// entry point's failure return, and recording it as a `SYSTEM` error on
/// the database is best effort. `kclangc.h` reached every class through
/// `PolyDB`, whose dispatch and unopened classes kept libkyotocabinet text
/// resident (`docs/findings/kc-shim-1118.md`). The system libkyotocabinet
/// still supplies the out-of-line code, as it does for libpinyin.
///
/// The shim is compiled with `cc` at `-O2`, the pin's autoconf default,
/// with hidden visibility, so the libraries export nothing beyond their
/// version scripts. `cpp(true)` links libstdc++, which libkyotocabinet already maps.
///
/// # Why generated fresh, not checked in
///
/// `shim.h` is plain C: two opaque handle types, the class codes and open
/// modes (`OXKC_*`, which `shim.cc` checks against `kcdb.h` with
/// `static_assert`) and the entry points. Nothing in it depends on the
/// installed Kyoto Cabinet, so a checked-in binding could not drift from
/// the library. Generating it keeps the Rust declarations in lockstep with
/// `shim.h`, as the other C backends' bindings are, for the build-time
/// libclang those backends need already.
#[cfg(feature = "kyotocabinet")]
mod kyotocabinet {
    use std::path::PathBuf;

    pub fn build(dependencies: &system_deps::Dependencies) {
        println!("cargo:rerun-if-changed=src/kyotocabinet/wrapper.h");
        println!("cargo:rerun-if-env-changed=BINDGEN_EXTRA_CLANG_ARGS");

        // `system-deps` emits the link-search paths and libraries. A
        // `rustc-link-arg` is package-scoped: cargo applies it only to the
        // targets it builds from THIS package — this crate's lib and its
        // own test artifacts — never to another package's binaries. The
        // rpath below therefore covers `cargo test -p oxpinyin-store` and
        // nothing else. `oxpinyin-training` links a final binary against
        // Kyoto Cabinet, so the directories are also published as the
        // `lib_dir` links metadata, which its build script reads
        // (`DEP_OXPINYIN_STORE_LIB_DIR`) for its own rpath. That rpath is a
        // convenience, not the contract: any other artifact built without
        // it must still find the library via LD_LIBRARY_PATH or its own
        // rpath setting. System link paths are not in `link_paths`, as
        // pkg-config itself leaves them out, so no rpath names one.
        let library = dependencies
            .get_by_name("kyotocabinet")
            .expect("system-deps probes kyotocabinet when the feature is on");
        for path in &library.link_paths {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", path.display());
        }
        if !library.link_paths.is_empty() {
            let joined = std::env::join_paths(&library.link_paths)
                .expect("a library directory holds no path-list separator");
            println!("cargo:lib_dir={}", joined.to_string_lossy());
        }
        // The shim compiles Kyoto Cabinet's headers into the facade, so they
        // come from the installation `system-deps` resolved.
        let include_paths: &[PathBuf] = &library.include_paths;

        // The shim (see the module comment): Kyoto Cabinet's headers and a
        // C++ compiler are what it needs; bindgen below reads only shim.h.
        println!("cargo:rerun-if-changed=src/kyotocabinet/shim.h");
        println!("cargo:rerun-if-changed=src/kyotocabinet/shim.cc");
        let mut shim = cc::Build::new();
        shim.cpp(true)
            .file("src/kyotocabinet/shim.cc")
            .opt_level(2)
            .flag_if_supported("-fvisibility=hidden")
            .flag_if_supported("-fvisibility-inlines-hidden");
        // Explicitly match the Linux address-sanitizer gate, even when cc's
        // default flags are disabled. rustc links the sanitizer runtime.
        // Without ASan, leave the native compiler and linker flags unchanged.
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux")
            && std::env::var("CARGO_CFG_SANITIZE")
                .unwrap_or_default()
                .split(',')
                .any(|sanitizer| sanitizer == "address")
        {
            shim.flag("-fsanitize=address")
                .flag("-fno-omit-frame-pointer");
        }
        for dir in include_paths {
            shim.include(dir);
        }
        shim.compile("oxpinyin_kc_shim");

        let mut builder = bindgen::Builder::default()
            .header("src/kyotocabinet/wrapper.h")
            // Only the shim's C entry points; allowlisting keeps the
            // generated file to what is actually called.
            .allowlist_function("oxkc_.*")
            .allowlist_type("oxkc_.*")
            .allowlist_var("OXKC_.*")
            .layout_tests(false)
            .derive_debug(false)
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));

        for dir in include_paths {
            builder = builder.clang_arg(format!("-I{}", dir.display()));
        }

        let bindings = match builder.generate() {
            Ok(bindings) => bindings,
            Err(error) => panic!(
                "the `kyotocabinet` feature generates the Kyoto Cabinet shim's \
                 declarations from src/kyotocabinet/shim.h, and bindgen could not read \
                 it ({error}). Generating them needs libclang (Debian/Ubuntu: \
                 libclang-dev). The shim itself needs Kyoto Cabinet's development \
                 package (Debian/Ubuntu: libkyotocabinet-dev; Fedora: kyotocabinet-devel) \
                 and a C++ compiler, or the SYSTEM_DEPS_KYOTOCABINET_* variables pointed at \
                 an installation (docs/runbooks/backends.md). Build without --features kyotocabinet to \
                 skip all of it."
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
    pub fn build(dependencies: &system_deps::Dependencies) {
        println!("cargo:rerun-if-env-changed=BINDGEN_EXTRA_CLANG_ARGS");

        // tkrzw is found only through `pkg-config tkrzw`, as libpinyin's
        // configure finds it (`PKG_CHECK_MODULES(TKRZW, [tkrzw])`, no
        // version constraint). `system-deps` emits the link-search paths
        // and libraries. System link paths are not in `link_paths`, as
        // pkg-config itself leaves them out, so no rpath names one.
        let library = dependencies
            .get_by_name("tkrzw")
            .expect("system-deps probes tkrzw when the feature is on");

        // The C API is the only header this build may bind: no C++ header
        // and no other tkrzw API crosses the ABI.
        let Some(header) = library
            .include_paths
            .iter()
            .map(|dir| dir.join("tkrzw_langc.h"))
            .find(|path| path.is_file())
        else {
            panic!(
                "libtkrzw required: pkg-config's include path for tkrzw names no directory \
                 holding the C API header tkrzw_langc.h."
            );
        };
        println!("cargo:rerun-if-changed={}", header.display());

        // Exactly the entry points the backend's safe wrapper calls, and
        // the types they traffic in. Anything else in tkrzw_langc.h — the
        // async adapter, the index API, the string utilities — stays
        // unbound: an unbound API cannot be misused.
        let mut builder = bindgen::Builder::default()
            .header(header.to_string_lossy())
            .allowlist_function("tkrzw_dbm_open")
            .allowlist_function("tkrzw_dbm_close")
            .allowlist_function("tkrzw_dbm_check")
            .allowlist_function("tkrzw_dbm_process")
            .allowlist_function("tkrzw_dbm_process_multi")
            .allowlist_function("tkrzw_dbm_process_each")
            .allowlist_function("tkrzw_dbm_set")
            .allowlist_function("tkrzw_dbm_count")
            .allowlist_function("tkrzw_dbm_is_healthy")
            .allowlist_function("tkrzw_dbm_synchronize")
            .allowlist_function("tkrzw_dbm_rebuild")
            .allowlist_function("tkrzw_dbm_make_iterator")
            .allowlist_function("tkrzw_dbm_iter_free")
            .allowlist_function("tkrzw_dbm_iter_jump")
            .allowlist_function("tkrzw_dbm_iter_first")
            .allowlist_function("tkrzw_dbm_iter_process")
            .allowlist_function("tkrzw_dbm_iter_next")
            .allowlist_function("tkrzw_get_last_status")
            .allowlist_function("tkrzw_get_last_status_code")
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
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));

        for dir in &library.include_paths {
            builder = builder.clang_arg(format!("-I{}", dir.display()));
        }

        let bindings = builder
            .generate()
            .expect("bindgen over tkrzw_langc.h must succeed");
        let out_dir = std::env::var("OUT_DIR").unwrap();
        bindings
            .write_to_file(std::path::Path::new(&out_dir).join("tkrzw_langc.rs"))
            .expect("writing the generated tkrzw bindings must succeed");

        // PE targets have no rpath, and link.exe reports the raw flag as
        // LNK4044, which the `linker_messages` lint turns into a failed
        // build under `-D warnings`; see the Berkeley DB module.
        if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "windows" {
            for path in &library.link_paths {
                // A library outside the loader's default path would
                // otherwise link but fail to start.
                //
                // Package-scoped, like every build-script `rustc-link-arg`
                // (see the Kyoto Cabinet module): this rpath reaches this
                // package's own lib, test and bench artifacts and nothing
                // else. Any other artifact finds the library through the
                // runtime library path or its own rpath.
                println!("cargo:rustc-link-arg=-Wl,-rpath,{}", path.display());
            }
        }
    }
}

/// Berkeley DB bindings, generated from the **system** `db.h`.
///
/// # Why the system library and not a vendored copy
///
/// Same arrangement as the tkrzw module above, with the interoperability
/// argument stronger still: the entire point of this backend is to read
/// and write the very `bigram.db`/`user_bigram.db` files the user's own
/// libpinyin wrote through the distro's libdb, so linking a second,
/// vendored Berkeley DB would read those files with the wrong writer.
/// No maintained Rust binding links the system library either — the
/// `libdb`/`libdb-sys` crates (last published 2020) compile and
/// statically link their own vendored copy, which is exactly the wrong
/// shape (`docs/findings/berkeleydb-compat-phase1.md`).
///
/// # Why generated fresh, not checked in
///
/// `DB`, `DBT` and `DBC` expose real struct layout — `DBT` crosses every
/// read and write, and `DB`'s member-function pointers are the whole
/// call surface. A checked-in binding would freeze one release's layout
/// and silently misread a differently-built library; the strong "we
/// write the user's profile with these calls" failure mode is why this
/// module also refuses, at run time, a libdb whose major.minor the
/// backend's format survey did not cover (see `src/bdb/ffi.rs`).
///
/// # Target release
///
/// libdb 5.3 — the last Sleepycat-licensed line, which is what both
/// Debian (`libdb-dev` → `libdb5.3-dev`) and Fedora (`libdb-devel`)
/// ship, and what libpinyin's `--with-dbm=BerkeleyDB` builds against.
/// Oracle relicensed 6.0+ to AGPL and the distros froze at 5.3.28; the
/// Homebrew formula carries 18.1, which this backend declines rather
/// than guesses at.
#[cfg(feature = "bdb")]
mod bdb {
    use std::path::PathBuf;

    /// The pin finds Berkeley DB without pkg-config
    /// (`AC_CHECK_HEADER([db.h])`, `AC_SEARCH_LIBS([db_create], [db])`), and
    /// no target distro ships a `db.pc`, so neither does this build. The
    /// override variables are `system-deps`', and they only take effect for
    /// a dependency it does not look up through pkg-config. Defaulting
    /// `NO_PKG_CONFIG` and `LIB` makes the no-override result the pin's:
    /// `db.h` on the compiler's default include path and `-ldb`. Anything
    /// the caller set stays (`SYSTEM_DEPS_BDB_LIB=libdb48` for vcpkg, say).
    pub fn pin_defaults() {
        for (name, value) in [
            ("SYSTEM_DEPS_BDB_NO_PKG_CONFIG", "1"),
            ("SYSTEM_DEPS_BDB_LIB", "db"),
        ] {
            if std::env::var_os(name).is_none() {
                // SAFETY: a build script's `main` is single-threaded, and
                // this runs first, before any thread (bindgen, `cc`,
                // `system-deps`) exists to read or write the environment.
                #[allow(unsafe_code)]
                unsafe {
                    std::env::set_var(name, value);
                }
            }
        }
    }

    pub fn build(dependencies: &system_deps::Dependencies) {
        println!("cargo:rerun-if-changed=src/bdb/wrapper.h");
        println!("cargo:rerun-if-env-changed=BINDGEN_EXTRA_CLANG_ARGS");

        // `system-deps` emits the link-search paths and the library.
        let library = dependencies
            .get_by_name("bdb")
            .expect("system-deps probes bdb when the feature is on");
        // The rpath is GNU-ld/ELF syntax; PE targets have no rpath, and the
        // raw flag reaches the Windows linker as at best a warning it
        // ignores — link.exe's LNK4044 "unrecognized option; ignored"
        // (documented) or lld-link's "ignoring unknown argument"
        // (observed) — so it is emitted only where it means something.
        // CARGO_CFG_TARGET_OS describes the TARGET cargo is building for,
        // unlike `cfg!`, which describes this build script's host.
        //
        // Package-scoped, like every build-script `rustc-link-arg` (see the
        // Kyoto Cabinet module): reaches this package's own lib, test and
        // bench artifacts and nothing else.
        if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "windows" {
            for path in &library.link_paths {
                println!("cargo:rustc-link-arg=-Wl,-rpath,{}", path.display());
            }
        }
        // With no `SYSTEM_DEPS_BDB_INCLUDE`, `<db.h>` is resolved on the
        // compiler's default include path, as the pin's configure does.
        let clang_args: Vec<String> = library
            .include_paths
            .iter()
            .map(|dir| format!("-I{}", dir.display()))
            .collect();

        // Exactly the entry points `src/bdb/ffi.rs` calls plus the types
        // and codes they traffic in — the small surface libpinyin itself
        // uses, where every `open` passes NULL for both the environment
        // and the transaction. Everything else in db.h — environments,
        // transactions, replication, secondary indices, the statistics
        // and verification surface — stays unbound: an unbound API
        // cannot be misused.
        let mut builder = bindgen::Builder::default()
            .header("src/bdb/wrapper.h")
            .allowlist_function("db_create")
            .allowlist_function("db_strerror")
            .allowlist_function("db_version")
            .allowlist_type("DB")
            .allowlist_type("DBC")
            .allowlist_type("DBT")
            .allowlist_type("DBTYPE")
            .allowlist_var("DB_.*")
            .allowlist_var("DB_VERSION_.*")
            .layout_tests(false)
            .derive_debug(false)
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));

        for arg in clang_args {
            builder = builder.clang_arg(arg);
        }

        let bindings = match builder.generate() {
            Ok(bindings) => bindings,
            Err(error) => panic!(
                "libdb required: the `bdb` feature links the SYSTEM Berkeley DB and needs its \
                 header db.h on the compiler's default include path, and bindgen could not read \
                 it ({error}). Install the platform's development package (Debian/Ubuntu: \
                 libdb-dev, which resolves to libdb5.3-dev; Fedora: libdb-devel; Arch: db) — \
                 the same one libpinyin's --with-dbm=BerkeleyDB build uses — or point \
                 SYSTEM_DEPS_BDB_INCLUDE and SYSTEM_DEPS_BDB_SEARCH_NATIVE at an installation \
                 (docs/runbooks/backends.md). Generating the declarations also needs libclang \
                 (Debian/Ubuntu: libclang-dev; Fedora: clang-devel). oxpinyin deliberately does \
                 NOT vendor or compile its own copy of Berkeley DB, so there is no fallback to \
                 download or build one."
            ),
        };

        let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
        bindings
            .write_to_file(out.join("bdb_bindings.rs"))
            .expect("write generated Berkeley DB declarations");
    }
}
