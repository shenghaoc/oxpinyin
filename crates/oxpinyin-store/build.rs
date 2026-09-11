//! Generates the C backends' bindings.
//!
//! Kyoto Cabinet is the workspace's default selected backend, so a normal
//! build runs bindgen over `kclangc.h` and links the system
//! libkyotocabinet. With `--no-default-features` — and with no C backend
//! feature enabled — this script does nothing: no bindgen, no extra
//! library. Selecting a peer backend explicitly
//! (`--features {redb|lmdb|tkrzw}`) skips the C-binding step for the
//! ones it does not build.
//!
//! * `kyotocabinet` — the Kyoto Cabinet C API (`kclangc.h`), on by default.
//! * `tkrzw` — the tkrzw C API (`tkrzw_langc.h`).
//! * `lmdb` — the LMDB C API (`lmdb.h`), from the system installation.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(feature = "tkrzw")]
    tkrzw::build();
    #[cfg(feature = "kyotocabinet")]
    kyotocabinet::build();
    #[cfg(feature = "lmdb")]
    lmdb::build();
}

/// Asks `pkg-config` for one field of `package`, split into individual
/// flags. Shared by both backend modules; shelling out avoids a
/// `pkg-config` crate dependency for the handful of calls this script makes.
/// Kyoto Cabinet does not always install a `.pc` file, so a miss is not
/// fatal there — the caller falls back to the library name.
#[cfg(any(feature = "kyotocabinet", feature = "tkrzw", feature = "lmdb"))]
fn pkg_config(flag: &str, package: &str) -> Option<Vec<String>> {
    let output = std::process::Command::new("pkg-config")
        .arg(flag)
        .arg(package)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(split_shell_words(&String::from_utf8_lossy(&output.stdout)))
}

/// Splits pkg-config output into flags the way a shell would read it:
/// whitespace separates words, a backslash escapes the next character,
/// and quotes group — so a pkg-config-escaped path containing spaces
/// (`-I/opt/my\ headers`) stays one flag instead of being cut in two.
#[cfg(any(feature = "kyotocabinet", feature = "tkrzw", feature = "lmdb"))]
fn split_shell_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut has_word = false;
    for c in line.chars() {
        if escaped {
            word.push(c);
            escaped = false;
            has_word = true;
            continue;
        }
        match quote {
            Some('\'') if c != '\'' => word.push(c),
            Some(q) if c == q => quote = None,
            Some(_) => {
                if c == '\\' {
                    escaped = true;
                } else {
                    word.push(c);
                }
                has_word = true;
            }
            None => match c {
                '\\' => escaped = true,
                '"' | '\'' => {
                    quote = Some(c);
                    has_word = true;
                }
                c if c.is_whitespace() => {
                    if has_word {
                        words.push(std::mem::take(&mut word));
                        has_word = false;
                    }
                }
                c => {
                    word.push(c);
                    has_word = true;
                }
            },
        }
    }
    if has_word {
        words.push(word);
    }
    words
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
/// the header: only libclang is added, and only when a build compiles a
/// C backend in (Kyoto Cabinet by default; tkrzw when asked for).
#[cfg(feature = "kyotocabinet")]
mod kyotocabinet {
    use std::path::PathBuf;

    pub fn build() {
        println!("cargo:rerun-if-changed=src/kyotocabinet/wrapper.h");
        println!("cargo:rerun-if-env-changed=BINDGEN_EXTRA_CLANG_ARGS");
        println!("cargo:rerun-if-env-changed=OXPINYIN_KC_INCLUDE_DIR");
        println!("cargo:rerun-if-env-changed=OXPINYIN_KC_LIB_DIR");
        // All three pkg-config selectors, as the tkrzw branch tracks:
        // PKG_CONFIG_LIBDIR replaces the search-directory list outright and
        // PKG_CONFIG_SYSROOT_DIR rewrites every discovered path, so either one
        // alone can point pkg-config at a different Kyoto Cabinet.
        println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
        println!("cargo:rerun-if-env-changed=PKG_CONFIG_LIBDIR");
        println!("cargo:rerun-if-env-changed=PKG_CONFIG_SYSROOT_DIR");

        let mut clang_args: Vec<String> = Vec::new();
        if let Some(cflags) = super::pkg_config("--cflags", "kyotocabinet") {
            clang_args.extend(cflags);
        }
        if let Ok(dir) = std::env::var("OXPINYIN_KC_INCLUDE_DIR") {
            clang_args.push(format!("-I{dir}"));
        }

        // A `rustc-link-search` reaches every later link in the graph, but a
        // build script's `rustc-link-arg` is package-scoped: cargo applies
        // it only to the targets it builds from THIS package — this crate's
        // lib and its own test artifacts — never to another package's
        // binaries. The rpath below therefore covers `cargo test -p
        // oxpinyin-store` and nothing else; every workspace package that
        // links a final binary against Kyoto Cabinet (oxpinyin-datagen,
        // oxpinyin-dictool, and the four training tools) mirrors this
        // rpath from its own build script. That rpath is a convenience,
        // not the contract: any other artifact built without it must still
        // find the library via LD_LIBRARY_PATH or its own rpath setting.
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
            .allowlist_function("tkrzw_dbm_count")
            .allowlist_function("tkrzw_dbm_synchronize")
            .allowlist_function("tkrzw_dbm_rebuild")
            .allowlist_function("tkrzw_dbm_make_iterator")
            .allowlist_function("tkrzw_dbm_iter_free")
            .allowlist_function("tkrzw_dbm_iter_jump")
            .allowlist_function("tkrzw_dbm_iter_first")
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
                // A tkrzw outside the default loader path — the usual case,
                // since the library often has to be made by hand — would
                // otherwise link but fail to start.
                //
                // Package-scoped, like every build-script `rustc-link-arg`
                // (see the Kyoto Cabinet module): this rpath lands on the
                // targets cargo builds from THIS package — its lib and its
                // own test artifacts — not on other packages' binaries,
                // which must make the library findable themselves via
                // LD_LIBRARY_PATH or their own rpath setting.
                println!("cargo:rustc-link-arg=-Wl,-rpath,{path}");
            }
        }
    }
}

/// LMDB bindings, generated from the **system** `lmdb.h`.
///
/// # Why the system library and not a vendored copy
///
/// LMDB is a C library every target distribution already packages
/// (Debian `liblmdb0` at runtime, `liblmdb-dev` to build; Fedora
/// `lmdb-libs` / `lmdb-devel`; Arch `lmdb`). oxpinyin links that
/// installation instead of compiling a second copy of `mdb.c` into its
/// own artifact: one LMDB per system, patched by the distribution's
/// security process rather than pinned inside a Rust dependency, and
/// nothing for a downstream packager to un-vendor. This is the same
/// arrangement the Kyoto Cabinet and tkrzw backends already have.
///
/// # Why generated fresh, not checked in
///
/// Unlike Kyoto Cabinet's opaque one-pointer handles, LMDB's ABI
/// exposes real struct layout: `MDB_val` (`{ size_t mv_size; void
/// *mv_data; }`) crosses every read and write, and `MDB_stat` carries
/// six integer fields that `is_empty` reads. A checked-in binding could
/// silently misread those against a differently-built library. The
/// error codes are the other half — `MDB_NOTFOUND`, `MDB_MAP_FULL`,
/// `MDB_DBS_FULL` are `#define`s the backend branches on, and the
/// generated constants come from the same header as the linked `.so`.
///
/// Generating costs a build-time libclang, which this crate already
/// requires for its other two C backends, and adds no new Rust
/// dependency: `bindgen` is already the `tkrzw`/`kyotocabinet`
/// build-dependency.
#[cfg(feature = "lmdb")]
mod lmdb {
    /// Locates `lmdb.h` under the discovered include path, falling back
    /// to the compiler's default include directory. LMDB's `.pc` on
    /// Debian carries no `-I` at all (the header lands in
    /// `/usr/include`), so the fallback is the normal case, not a
    /// rescue path; a Homebrew `lmdb.pc` does carry one, and
    /// `OXPINYIN_LMDB_INCLUDE_DIR` prepends an explicit directory ahead
    /// of both.
    fn header(clang_args: &[String]) -> Option<std::path::PathBuf> {
        let mut dirs: Vec<std::path::PathBuf> = clang_args
            .iter()
            .filter_map(|flag| flag.strip_prefix("-I"))
            .map(std::path::PathBuf::from)
            .collect();
        dirs.push(std::path::PathBuf::from("/usr/include"));
        dirs.into_iter()
            .map(|dir| dir.join("lmdb.h"))
            .find(|path| path.is_file())
    }

    pub fn build() {
        println!("cargo:rerun-if-changed=src/lmdb/wrapper.h");
        // Every input that can change the generated declarations or the
        // library they are generated against, so a change reruns this
        // script instead of leaving `lmdb_bindings.rs` stale. The same
        // set the Kyoto Cabinet module tracks.
        println!("cargo:rerun-if-env-changed=BINDGEN_EXTRA_CLANG_ARGS");
        println!("cargo:rerun-if-env-changed=OXPINYIN_LMDB_INCLUDE_DIR");
        println!("cargo:rerun-if-env-changed=OXPINYIN_LMDB_LIB_DIR");
        // All three pkg-config selectors: PKG_CONFIG_LIBDIR replaces the
        // search-directory list outright and PKG_CONFIG_SYSROOT_DIR
        // rewrites every discovered path, so either one alone can select
        // a different LMDB.
        println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
        println!("cargo:rerun-if-env-changed=PKG_CONFIG_LIBDIR");
        println!("cargo:rerun-if-env-changed=PKG_CONFIG_SYSROOT_DIR");

        // pkg-config first, then the explicit override ahead of it: an
        // installation outside the default prefix that ships no `.pc`
        // file is otherwise unreachable, which is why the Kyoto Cabinet
        // module carries the same pair.
        let mut clang_args: Vec<String> = super::pkg_config("--cflags", "lmdb").unwrap_or_default();
        if let Ok(dir) = std::env::var("OXPINYIN_LMDB_INCLUDE_DIR") {
            clang_args.insert(0, format!("-I{dir}"));
        }
        if let Ok(dir) = std::env::var("OXPINYIN_LMDB_LIB_DIR") {
            println!("cargo:rustc-link-search=native={dir}");
            // Package-scoped, like every build-script `rustc-link-arg`
            // (see the Kyoto Cabinet module): this rpath reaches this
            // package's own lib, test and bench artifacts and nothing
            // else. Any other artifact must find the library through
            // LD_LIBRARY_PATH or its own rpath.
            println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
        }
        let Some(header) = header(&clang_args) else {
            panic!(
                "liblmdb required: the `lmdb` feature links the SYSTEM LMDB and needs its \
                 header lmdb.h, which was found neither under `pkg-config --cflags lmdb` \
                 nor in the default include directory. Install the platform's development \
                 package (Debian/Ubuntu: liblmdb-dev; Fedora: lmdb-devel; Arch: lmdb; \
                 macOS Homebrew: lmdb, whose lmdb.pc needs \
                 PKG_CONFIG_PATH=\"$(brew --prefix)/lib/pkgconfig\" if pkg-config does not \
                 already search it), or point OXPINYIN_LMDB_INCLUDE_DIR and \
                 OXPINYIN_LMDB_LIB_DIR at an installation directly, or build a different \
                 backend. oxpinyin deliberately does NOT vendor or compile its own copy of \
                 LMDB, so there is no fallback to download or build one -- see \
                 docs/runbooks/backends.md."
            );
        };
        println!("cargo:rerun-if-changed={}", header.display());

        match super::pkg_config("--libs", "lmdb") {
            Some(libs) => {
                for lib in &libs {
                    if let Some(name) = lib.strip_prefix("-l") {
                        println!("cargo:rustc-link-lib={name}");
                    } else if let Some(path) = lib.strip_prefix("-L") {
                        println!("cargo:rustc-link-search=native={path}");
                        // Package-scoped, like every build-script
                        // `rustc-link-arg` (see the Kyoto Cabinet module):
                        // this rpath reaches this package's own lib, test
                        // and bench artifacts and nothing else.
                        println!("cargo:rustc-link-arg=-Wl,-rpath,{path}");
                    }
                }
            }
            // No usable `.pc`, but the header was found: name the library
            // directly and let the loader's default path resolve it.
            None => println!("cargo:rustc-link-lib=lmdb"),
        }

        // Exactly the entry points `src/lmdb/mod.rs` calls plus the types
        // and codes they traffic in. Everything else in lmdb.h — the
        // multi-value (`MDB_DUPSORT`) cursor surface, the reader table,
        // the custom comparators, `mdb_env_copy` — stays unbound: an
        // unbound API cannot be misused.
        let mut builder = bindgen::Builder::default()
            // The wrapper adds <unistd.h> beside the system <lmdb.h>;
            // `sysconf`/`_SC_PAGESIZE` are the only things it brings.
            .header("src/lmdb/wrapper.h")
            .allowlist_function("sysconf")
            .allowlist_var("_SC_PAGESIZE")
            .allowlist_function("mdb_env_create")
            .allowlist_function("mdb_env_open")
            .allowlist_function("mdb_env_close")
            .allowlist_function("mdb_env_sync")
            .allowlist_function("mdb_env_set_mapsize")
            .allowlist_function("mdb_env_set_maxdbs")
            .allowlist_function("mdb_txn_begin")
            .allowlist_function("mdb_txn_commit")
            .allowlist_function("mdb_txn_abort")
            .allowlist_function("mdb_dbi_open")
            .allowlist_function("mdb_drop")
            .allowlist_function("mdb_get")
            .allowlist_function("mdb_put")
            .allowlist_function("mdb_del")
            .allowlist_function("mdb_stat")
            .allowlist_function("mdb_cursor_open")
            .allowlist_function("mdb_cursor_close")
            .allowlist_function("mdb_cursor_get")
            .allowlist_function("mdb_strerror")
            .allowlist_type("MDB_env")
            .allowlist_type("MDB_txn")
            .allowlist_type("MDB_cursor")
            .allowlist_type("MDB_dbi")
            .allowlist_type("MDB_val")
            .allowlist_type("MDB_stat")
            .allowlist_type("MDB_cursor_op")
            .allowlist_var("MDB_.*")
            // `MDB_cursor_op::MDB_FIRST` reads as the C does at the call
            // site, where the default flat spelling would not. Scoped to
            // this one enum on purpose: as a global style it would also
            // wrap `<unistd.h>`'s anonymous `_SC_*` enum in a generated
            // module and hide `_SC_PAGESIZE`.
            .constified_enum_module("MDB_cursor_op")
            .derive_debug(false)
            .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));

        for arg in clang_args {
            builder = builder.clang_arg(arg);
        }

        let bindings = match builder.generate() {
            Ok(bindings) => bindings,
            Err(error) => panic!(
                "liblmdb required: bindgen could not read the system lmdb.h at {} ({error}). \
                 Generating these declarations needs libclang (Debian/Ubuntu: libclang-dev). \
                 oxpinyin does not vendor LMDB; there is no fallback to a bundled copy.",
                header.display()
            ),
        };

        let out =
            std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
        bindings
            .write_to_file(out.join("lmdb_bindings.rs"))
            .expect("write generated LMDB declarations");
    }
}
