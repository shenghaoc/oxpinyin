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

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "tkrzw")]
    tkrzw::build();
    #[cfg(feature = "kyotocabinet")]
    kyotocabinet::build();
    #[cfg(feature = "bdb")]
    bdb::build();
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

    pub fn build() {
        println!("cargo:rerun-if-changed=src/kyotocabinet/wrapper.h");
        println!("cargo:rerun-if-env-changed=BINDGEN_EXTRA_CLANG_ARGS");
        println!("cargo:rerun-if-env-changed=OXPINYIN_KC_INCLUDE_DIR");
        println!("cargo:rerun-if-env-changed=OXPINYIN_KC_LIB_DIR");

        // A `rustc-link-search` reaches every later link in the graph, but a
        // build script's `rustc-link-arg` is package-scoped: cargo applies
        // it only to the targets it builds from THIS package — this crate's
        // lib and its own test artifacts — never to another package's
        // binaries. The rpath below therefore covers `cargo test -p
        // oxpinyin-store` and nothing else; every workspace package that
        // links a final binary against Kyoto Cabinet (oxpinyin-datagen and
        // the four training tools) mirrors this
        // rpath from its own build script. That rpath is a convenience,
        // not the contract: any other artifact built without it must still
        // find the library via LD_LIBRARY_PATH or its own rpath setting.
        if let Ok(dir) = std::env::var("OXPINYIN_KC_LIB_DIR") {
            println!("cargo:rustc-link-search=native={dir}");
            println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
        }
        // The probe emits the link-search paths and libraries itself, after
        // the override above. Kyoto Cabinet does not always install a .pc
        // file, so a miss is not fatal: name the library directly. A distro
        // package puts it on the default search path. System link paths stay
        // out, as pkg-config itself leaves them, so no rpath names one.
        let include_paths = match pkg_config::Config::new()
            .print_system_libs(false)
            .probe("kyotocabinet")
        {
            Ok(library) => {
                for path in &library.link_paths {
                    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", path.display());
                }
                library.include_paths
            }
            Err(_) => {
                println!("cargo:rustc-link-lib=kyotocabinet");
                Vec::new()
            }
        };
        // The override goes ahead of pkg-config's directories, as for BDB.
        // The shim compiles Kyoto Cabinet's headers into the facade, so they
        // must come from the installation OXPINYIN_KC_LIB_DIR links, whose
        // link-search path is likewise emitted first (docs/packaging.md).
        let include_paths: Vec<PathBuf> = std::env::var_os("OXPINYIN_KC_INCLUDE_DIR")
            .map(PathBuf::from)
            .into_iter()
            .chain(include_paths)
            .collect();

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
        for dir in &include_paths {
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

        for dir in &include_paths {
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
                 and a C++ compiler, or OXPINYIN_KC_INCLUDE_DIR and OXPINYIN_KC_LIB_DIR \
                 pointed at an installation. Build without --features kyotocabinet to \
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
    pub fn build() {
        println!("cargo:rerun-if-env-changed=BINDGEN_EXTRA_CLANG_ARGS");

        // tkrzw is found only through `pkg-config tkrzw`, as libpinyin's
        // configure finds it (`PKG_CHECK_MODULES(TKRZW, [tkrzw])`, no
        // version constraint). The probe emits the link-search paths and
        // libraries, and reruns this script when a PKG_CONFIG_* variable
        // changes. System link paths stay out, as pkg-config itself leaves
        // them, so no rpath names one.
        let library = pkg_config::Config::new()
            .print_system_libs(false)
            .probe("tkrzw")
            .unwrap_or_else(|error| {
                panic!(
                    "libtkrzw required: the `tkrzw` feature (the default backend) finds \
                     tkrzw through pkg-config, which failed ({error}). Make tkrzw.pc \
                     visible to pkg-config, or select another backend with \
                     `--no-default-features --features bdb` or `--features kyotocabinet`."
                )
            });

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

    /// Locates `db.h` under the discovered include path. Neither target
    /// distro ships a `db.pc` (Debian's `libdb5.3-dev` and Fedora's
    /// `libdb-devel` both install headers and no pkg-config file), so
    /// the fixed directories are the normal case and pkg-config a
    /// bonus: `/usr/include/db.h` (Debian, and Fedora's top-level
    /// symlink), `/usr/include/libdb/db.h` (Fedora's real location),
    /// `/usr/include/db5/db.h` (historical Ubuntu).
    fn db_header(clang_args: &[String]) -> Option<PathBuf> {
        let mut dirs: Vec<PathBuf> = clang_args
            .iter()
            .filter_map(|flag| flag.strip_prefix("-I"))
            .map(PathBuf::from)
            .collect();
        for extra in ["/usr/include", "/usr/include/libdb", "/usr/include/db5"] {
            let dir = PathBuf::from(extra);
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
        dirs.into_iter()
            .map(|dir| dir.join("db.h"))
            .find(|path| path.is_file())
    }

    pub fn build() {
        println!("cargo:rerun-if-changed=src/bdb/wrapper.h");
        println!("cargo:rerun-if-env-changed=BINDGEN_EXTRA_CLANG_ARGS");
        println!("cargo:rerun-if-env-changed=OXPINYIN_BDB_INCLUDE_DIR");
        println!("cargo:rerun-if-env-changed=OXPINYIN_BDB_LIB_DIR");
        println!("cargo:rerun-if-env-changed=OXPINYIN_BDB_LIB_NAME");

        // pkg-config first (a hit is a bonus — no target distro ships a
        // db.pc), then the explicit override ahead of it, then the fixed
        // header directories. The probe only reads: the library is linked
        // below, after the OXPINYIN_BDB_LIB_NAME override has had its say,
        // so it emits no link metadata. It tracks the PKG_CONFIG_*
        // variables itself. System paths stay out, as for `pkg-config
        // --cflags` and `--libs` without the allow-system variables.
        let library = ["libdb", "db"].iter().find_map(|name| {
            pkg_config::Config::new()
                .cargo_metadata(false)
                .print_system_cflags(false)
                .print_system_libs(false)
                .probe(name)
                .ok()
        });
        let mut clang_args: Vec<String> = Vec::new();
        if let Some(library) = &library {
            clang_args.extend(
                library
                    .include_paths
                    .iter()
                    .map(|dir| format!("-I{}", dir.display())),
            );
            clang_args.extend(library.defines.iter().map(|(name, value)| match value {
                Some(value) => format!("-D{name}={value}"),
                None => format!("-D{name}"),
            }));
        }
        if let Ok(dir) = std::env::var("OXPINYIN_BDB_INCLUDE_DIR") {
            clang_args.insert(0, format!("-I{dir}"));
        }
        if let Ok(dir) = std::env::var("OXPINYIN_BDB_LIB_DIR") {
            println!("cargo:rustc-link-search=native={dir}");
            // Package-scoped, like every build-script `rustc-link-arg`
            // (see the Kyoto Cabinet module): reaches this package's own
            // lib, test and bench artifacts and nothing else.
            // The rpath is GNU-ld/ELF syntax; PE targets have no rpath, and
            // the raw flag reaches the Windows linker as at best a warning
            // it ignores — link.exe's LNK4044 "unrecognized option; ignored"
            // (documented) or lld-link's "ignoring unknown argument"
            // (observed) — so it is emitted only where it means something.
            // CARGO_CFG_TARGET_OS describes the TARGET cargo is building
            // for, unlike `cfg!`, which describes this build script's host.
            let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
            if target_os != "windows" {
                println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
            }
        }
        let Some(header) = db_header(&clang_args) else {
            panic!(
                "libdb required: the `bdb` feature links the SYSTEM Berkeley DB and needs its \
                 header db.h, which was found nowhere on the include path. Install the \
                 platform's development package (Debian/Ubuntu: libdb-dev, which resolves to \
                 libdb5.3-dev; Fedora: libdb-devel; Arch: db) — the same one libpinyin's \
                 --with-dbm=BerkeleyDB build uses — or point OXPINYIN_BDB_INCLUDE_DIR and \
                 OXPINYIN_BDB_LIB_DIR at an installation directly. oxpinyin deliberately does \
                 NOT vendor or compile its own copy of Berkeley DB, so there is no fallback to \
                 download or build one -- see docs/runbooks/backends.md."
            );
        };
        println!("cargo:rerun-if-changed={}", header.display());

        // An installation whose library carries a name the pkg-config and
        // `-ldb` paths below cannot express (vcpkg's `berkeleydb` port
        // installs `libdb48.lib`): name it directly, in the same shape as
        // the INCLUDE_DIR/LIB_DIR overrides above. Precedence, exactly:
        // this override (if set) skips the probed libraries entirely;
        // otherwise pkg-config's `libdb`/`db` is used, then the bare
        // `-ldb` fallback.
        if let Ok(name) = std::env::var("OXPINYIN_BDB_LIB_NAME") {
            println!("cargo:rustc-link-lib={name}");
        } else {
            match &library {
                Some(library) => {
                    for name in &library.libs {
                        println!("cargo:rustc-link-lib={name}");
                    }
                    for path in &library.link_paths {
                        println!("cargo:rustc-link-search=native={}", path.display());
                        // Package-scoped, like every build-script
                        // `rustc-link-arg` (see the Kyoto Cabinet module).
                        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", path.display());
                    }
                }
                // No `.pc` on either target distro: name the library directly
                // (both ship an unversioned `libdb.so` dev symlink) and let the
                // loader's default path resolve it.
                None => println!("cargo:rustc-link-lib=db"),
            }
        }

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
                "libdb required: bindgen could not read the system db.h at {} ({error}). \
                 Generating these declarations needs libclang (Debian/Ubuntu: libclang-dev; \
                 Fedora: clang-devel). oxpinyin does not vendor Berkeley DB; there is no \
                 fallback to a bundled copy.",
                header.display()
            ),
        };

        let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
        bindings
            .write_to_file(out.join("bdb_bindings.rs"))
            .expect("write generated Berkeley DB declarations");
    }
}
