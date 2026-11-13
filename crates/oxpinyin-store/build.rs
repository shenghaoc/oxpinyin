//! Generates the backends' native bindings.
//!
//! tkrzw is the workspace's default selected backend, so a normal
//! build generates and compiles the cxx bridge over tkrzw's classes and
//! links libtkrzw. With `--no-default-features` — and with no backend
//! feature enabled — this script does nothing: no bindgen, no C++, no
//! extra library. Selecting a peer backend explicitly
//! (`--no-default-features --features {kyotocabinet|bdb}`) skips the
//! binding steps of the ones it does not build.
//!
//! * `kyotocabinet` — the cxx bridge over Kyoto Cabinet's concrete classes
//!   (`src/kyotocabinet/bridge.rs`, `src/kyotocabinet/adapter.cc`).
//! * `tkrzw` — the cxx bridge over tkrzw's concrete classes
//!   (`src/tkrzw/bridge.rs`, `src/tkrzw/adapter.cc`), on by default.
//! * `bdb` — the Berkeley DB C API (`db.h`), from the system libdb.
//!
//! # Discovery
//!
//! `system-deps` is the one discovery layer. The backends are declared in
//! this package's `[package.metadata.system-deps]`, and the
//! `SYSTEM_DEPS_<NAME>_*` variables are the only overrides
//! (`docs/runbooks/backends.md`). It emits the link-search and link-lib
//! directives. This script passes the resolved include paths to bindgen
//! and `cxx-build`.
//!
//! Each backend is found as libpinyin's `configure.ac` finds it: tkrzw and
//! Kyoto Cabinet through their `.pc` file (`PKG_CHECK_MODULES`), Berkeley
//! DB without pkg-config (`AC_CHECK_HEADER([db.h])` and
//! `AC_SEARCH_LIBS([db_create], [db])`).

/// The backend's `database format` token, the pin's `DATABASE_FORMAT`:
/// `configure.ac:134-135` sets it once from `--with-dbm` and substitutes it
/// into both `libpinyin.pc.in` and `libzhuyin.pc.in`, and `user.conf`'s
/// conformance check compares the same string (`table_info.cpp`'s
/// `to/from_table_database_format_type`: exactly `BerkeleyDB`,
/// `KyotoCabinet`, `Tkrzw` upstream). This is the one mapping from backend
/// feature to token. The library reads it as `DEFAULT_STORE_DB_FORMAT`, and
/// the C-ABI crates' build scripts read it as
/// `DEP_OXPINYIN_STORE_DATABASE_FORMAT` (this package's `links`). The
/// exactly-one-backend guard in `src/lib.rs` refuses any build that enables
/// two backend features, so the chain's order picks nothing.
#[cfg(any(feature = "tkrzw", feature = "kyotocabinet", feature = "bdb"))]
const DATABASE_FORMAT: &str = if cfg!(feature = "kyotocabinet") {
    "KyotoCabinet"
} else if cfg!(feature = "tkrzw") {
    "Tkrzw"
} else {
    "BerkeleyDB"
};

#[cfg(any(feature = "tkrzw", feature = "kyotocabinet", feature = "bdb"))]
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:database_format={DATABASE_FORMAT}");
    println!("cargo:rustc-env=OXPINYIN_STORE_DATABASE_FORMAT={DATABASE_FORMAT}");
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

/// What the two cxx bridges share: how a native library's include
/// directories reach the C++ compiler, the address-sanitizer flags, and
/// the link order after the bridge archive.
#[cfg(any(feature = "tkrzw", feature = "kyotocabinet"))]
mod native {
    /// The directories the configured C++ compiler searches for `<...>`
    /// includes by default, canonicalised: the list it prints between
    /// `#include <...> search starts here:` and `End of search list.` for
    /// `-E -x c++ -v` on an empty input (GCC and Clang alike). `None` when
    /// the compiler cannot be run or prints no list.
    fn default_include_dirs(compiler: &cc::Tool) -> Option<Vec<std::path::PathBuf>> {
        let mut command = compiler.to_command();
        let output = command
            .args(["-E", "-x", "c++", "-v", "-"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stderr);
        let mut dirs = Vec::new();
        let mut listing = false;
        for line in text.lines() {
            if line.starts_with("#include <...> search starts here:") {
                listing = true;
            } else if line.starts_with("End of search list.") {
                break;
            } else if listing {
                let dir = std::path::PathBuf::from(line.trim());
                dirs.push(std::fs::canonicalize(&dir).unwrap_or(dir));
            }
        }
        (!dirs.is_empty()).then_some(dirs)
    }

    /// Adds a library's include directories to a bridge build.
    ///
    /// The library's headers are third-party: with `-isystem` their own
    /// `-Wall -Wextra` warnings are suppressed. A directory the compiler
    /// already searches by default must not be passed that way: GCC puts
    /// an `-isystem` directory ahead of its own list, so `/usr/include`
    /// lands before libstdc++'s directory and `<cstdlib>`'s
    /// `#include_next <stdlib.h>` finds nothing (GCC, "Directory Options"
    /// and "Wrapper Headers"). `-I` would be ignored for such a directory
    /// but would not silence the warnings from a self-built prefix. So the
    /// compiler is asked for its default search list once, and each probed
    /// directory is skipped when it is already there and `-isystem`
    /// otherwise; a compiler that does not answer, or is not GCC-like, gets
    /// `cc`'s ordinary include flag.
    pub fn add_includes(build: &mut cc::Build, include_paths: &[std::path::PathBuf]) {
        let compiler = build.get_compiler();
        let gnu_like = compiler.is_like_gnu() || compiler.is_like_clang();
        let defaults = if gnu_like {
            default_include_dirs(&compiler)
        } else {
            None
        };
        for dir in include_paths {
            let canonical = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.clone());
            match &defaults {
                Some(defaults) if defaults.contains(&canonical) => {}
                Some(_) => {
                    build.flag(format!("-isystem{}", dir.display()));
                }
                None => {
                    build.include(dir);
                }
            }
        }
    }

    /// The compilation both bridges use: `-O2`, the pin's autoconf
    /// default, as C++17, with hidden visibility so the libraries export
    /// nothing beyond their version scripts
    /// (`docs/packaging.md#export-gate`). The address-sanitizer flags match
    /// the Linux sanitizer gate explicitly (#1531), so the bridge objects
    /// are instrumented whatever `cc`'s own defaults do; rustc links the
    /// sanitizer runtime. Without ASan the flags are unchanged.
    pub fn configure(build: &mut cc::Build) {
        build
            .std("c++17")
            .opt_level(2)
            .flag_if_supported("-fvisibility=hidden")
            .flag_if_supported("-fvisibility-inlines-hidden");
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux")
            && std::env::var("CARGO_CFG_SANITIZE")
                .unwrap_or_default()
                .split(',')
                .any(|sanitizer| sanitizer == "address")
        {
            build
                .flag("-fsanitize=address")
                .flag("-fno-omit-frame-pointer");
        }
    }

    /// Names `library`'s libraries again; call it after the bridge's
    /// `compile`. `system-deps` named them before `compile` named the
    /// bridge archive, and rustc keeps that order. A linker that resolves
    /// left to right and drops an `--as-needed` library nothing has yet
    /// referenced (GNU ld) then discards the native library before the
    /// archive that needs it, and every one of its symbols is undefined.
    /// Naming the libraries again after the archive fixes the order; an
    /// order-independent linker (ld64, lld) is unaffected.
    pub fn link_after_archive(library: &system_deps::Library) {
        for lib in &library.libs {
            println!("cargo:rustc-link-lib={}", lib.name);
        }
    }
}

/// Kyoto Cabinet: the cxx bridge over the pin's concrete classes, plus the
/// semantic adapter `src/kyotocabinet/adapter.cc`.
///
/// # The concrete classes, not `kclangc.h`
///
/// Kyoto Cabinet's database classes are header code. libpinyin constructs
/// `TreeDB`, `HashDB`, `ProtoTreeDB` and `StashDB` itself and compiles them
/// from the headers into its own text. The bridge (`src/kyotocabinet/
/// bridge.rs`) does the same: cxx generates the Rust/C++ glue, and
/// `adapter.cc` holds only what cxx cannot express — the `Visitor`
/// subclass, the empty-region pointer, the read of `BasicDB::error()`, the
/// constructors and constants, and the forwards through a shared reference
/// that the classes' own locking permits (Rust/C++ interoperability
/// ruling, 2026-11-02, `docs/findings/compatibility-policy.md`).
/// `kclangc.h` reached every class through `PolyDB`, whose dispatch and
/// unopened classes kept libkyotocabinet text resident. The system libkyotocabinet still
/// supplies the out-of-line code, as it does for libpinyin.
///
/// The bridge is compiled as `native::configure` describes; the include
/// paths come from the `system-deps` probe, added as `native::add_includes`
/// describes. `cxx` links libstdc++, which libkyotocabinet already maps.
#[cfg(feature = "kyotocabinet")]
mod kyotocabinet {
    pub fn build(dependencies: &system_deps::Dependencies) {
        println!("cargo:rerun-if-changed=src/kyotocabinet/bridge.rs");
        println!("cargo:rerun-if-changed=src/kyotocabinet/adapter.h");
        println!("cargo:rerun-if-changed=src/kyotocabinet/adapter.cc");

        let library = dependencies
            .get_by_name("kyotocabinet")
            .expect("system-deps probes kyotocabinet when the feature is on");
        let Some(header) = library
            .include_paths
            .iter()
            .map(|dir| dir.join("kcdb.h"))
            .find(|path| path.is_file())
        else {
            panic!(
                "the `kyotocabinet` feature compiles Kyoto Cabinet's C++ headers into the \
                 facade, and none of kyotocabinet.pc's include directories holds kcdb.h. \
                 Install its development package (Debian/Ubuntu: libkyotocabinet-dev; \
                 Fedora: kyotocabinet-devel), or point the SYSTEM_DEPS_KYOTOCABINET_* \
                 variables at an installation (docs/runbooks/backends.md)."
            );
        };
        println!("cargo:rerun-if-changed={}", header.display());

        let mut bridge = cxx_build::bridge("src/kyotocabinet/bridge.rs");
        bridge.file("src/kyotocabinet/adapter.cc");
        super::native::configure(&mut bridge);
        super::native::add_includes(&mut bridge, &library.include_paths);
        bridge.compile("oxpinyin_kc_bridge");
        super::native::link_after_archive(library);
    }
}

/// tkrzw: the cxx bridge over the pin's concrete classes, plus the
/// semantic adapter `src/tkrzw/adapter.cc`.
///
/// # The concrete classes, not `tkrzw_langc.h`
///
/// libpinyin allocates `TreeDBM`, `HashDBM`, `BabyDBM` and `TinyDBM` itself
/// and calls them through `DBM*`. The bridge (`src/tkrzw/bridge.rs`) does
/// the same: cxx generates the Rust/C++ glue, and `adapter.cc` holds only
/// what cxx cannot express — the concrete constructors, the byte-view and
/// status conversions, the `RecordProcessor` subclass of the key walk, and
/// the mutation through a shared reference that the classes' own locking
/// permits. The adapter calls the library; it copies none of it. The C API
/// (`PolyDBM`) is not used: it deletes a concrete object whose `Open`
/// failed and catches every C++ exception (Rust/C++ interoperability
/// ruling, 2026-11-02, `docs/findings/compatibility-policy.md`).
///
/// # Compilation
///
/// The adapter and the generated glue are compiled as `native::configure`
/// describes. tkrzw's include paths come from the `system-deps` probe,
/// which also emits the link directives (and, through the pkg-config
/// crate's `PKG_CONFIG_ALLOW_SYSTEM_CFLAGS`, reports a distro's
/// `/usr/include` too); `native::add_includes` passes them on.
#[cfg(feature = "tkrzw")]
mod tkrzw {
    pub fn build(dependencies: &system_deps::Dependencies) {
        println!("cargo:rerun-if-changed=src/tkrzw/bridge.rs");
        println!("cargo:rerun-if-changed=src/tkrzw/adapter.h");
        println!("cargo:rerun-if-changed=src/tkrzw/adapter.cc");

        let library = dependencies
            .get_by_name("tkrzw")
            .expect("system-deps probes tkrzw when the feature is on");
        let Some(header) = library
            .include_paths
            .iter()
            .map(|dir| dir.join("tkrzw_dbm.h"))
            .find(|path| path.is_file())
        else {
            panic!(
                "libtkrzw required: pkg-config's include path for tkrzw names no directory \
                 holding the C++ header tkrzw_dbm.h."
            );
        };
        println!("cargo:rerun-if-changed={}", header.display());

        let mut bridge = cxx_build::bridge("src/tkrzw/bridge.rs");
        bridge.file("src/tkrzw/adapter.cc");
        super::native::configure(&mut bridge);
        super::native::add_includes(&mut bridge, &library.include_paths);
        bridge.compile("oxpinyin_tkrzw_bridge");
        super::native::link_after_archive(library);
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
/// shape.
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
                #[expect(
                    unsafe_code,
                    reason = "set_var before any thread exists; see the SAFETY comment"
                )]
                // SAFETY: a build script's `main` is single-threaded, and
                // this runs first, before any thread (bindgen, `cc`,
                // `system-deps`) exists to read or write the environment.
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
