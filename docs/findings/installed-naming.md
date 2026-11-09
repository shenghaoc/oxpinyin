# The installed tree takes libpinyin's name; the source tree keeps ours

Status: **implemented**.

Precedent is universal — LibreSSL installs `libssl.so`/`libcrypto.so`,
MariaDB installs `libmysqlclient.so` and `mysqlclient.pc`, Mesa installs
`libGL.so`. The source repository keeps its own identity; the installed
artifacts take the name the ecosystem already knows.

## What the real package looks like

Read off Ubuntu's `libpinyin15-dev`:

```
/usr/lib/x86_64-linux-gnu/pkgconfig/libpinyin.pc     ← libpinyin.pc, not pinyin.pc
/usr/include/libpinyin-2.8.1/pinyin.h                ← versioned dir, NO top-level symlink
/usr/lib/x86_64-linux-gnu/libpinyin.so.15
```

and the `.pc` itself:

```
prefix=/usr
exec_prefix=${prefix}
libdir=${prefix}/lib/x86_64-linux-gnu
includedir=${prefix}/include
pkgdatadir=${prefix}/lib/x86_64-linux-gnu/libpinyin
database_format=KyotoCabinet

libpinyinincludedir=${includedir}/libpinyin-2.8.1
libpinyin_binary_version=15.0

Name: libpinyin
Description: Library to deal with pinyin
Version: 2.8.1
Requires: glib-2.0
Libs: -L${libdir} -lpinyin
Cflags: -I${libpinyinincludedir}
```

Two details are easy to get wrong: the module name is **`libpinyin`**
while the link flag is **`-lpinyin`**, and `database_format` is one word
with no space (`KyotoCabinet`).

`pkgdatadir` points at `@libdir@/libpinyin`, one level *above* where the
data lives (`@libdir@/libpinyin/data`).

## What the install produces

The version is `libpinyin_tag` in `tools/oracle/oracle-pin.txt`
(2.11.92). Both capi build scripts read it
(`crates/oxpinyin-capi/build_pin_version.rs` also rejects a
`[package.metadata.capi]` table that disagrees), and
`just packaging pc-version` prints it for package assembly.

```
/usr/include/libpinyin-2.11.92/pinyin.h
/usr/include/libpinyin-2.11.92/novel_types.h
/usr/include/libpinyin-2.11.92/pinyin_custom2.h
/usr/lib/x86_64-linux-gnu/libpinyin.a
/usr/lib/x86_64-linux-gnu/libpinyin.so
/usr/lib/x86_64-linux-gnu/libpinyin.so.15
/usr/lib/x86_64-linux-gnu/libpinyin.so.15.0.0
/usr/lib/x86_64-linux-gnu/pkgconfig/libpinyin.pc
```

`find <prefix> -name '*oxpinyin*'` finds nothing. `libzhuyin` follows the
same pattern.

## The mechanism — build.rs generates the `.pc`, a wrapper installs it

`pkgdatadir`, `database_format`, `libpinyinincludedir` and
`libpinyin_binary_version` are **not producible through cargo-c**. This is
a fixed limit of cargo-c, read from cargo-c 0.10.24's source:

- `build.rs:358` — the `PkgConfigCApiConfig` struct has exactly seven
  fields (`name`, `filename`, `description`, `version`, `requires`,
  `requires_private`, `strip_include_path_components`) and no
  custom-variable map. The table is parsed leniently, so extra keys —
  `generate` included — are silently ignored.
- No install prefix reaches build scripts: the crate's only `set_var`
  calls are `CARGO_C_CARGO` (`config.rs:11`) and `INLINE_C_RS_CFLAGS`
  (`build.rs:1402`). A build script cannot know `--prefix`.
- `install.rs:216-220` installs cargo-c's own `.pc` into
  `<libdir>/pkgconfig` unconditionally and first; `install.rs:233-236`
  installs build-generated data assets under `datadir`, never the
  pkg-config dir.

**Why it matters.** fcitx-libpinyin reads three of these at cmake
configure time (`CMakeLists.txt:12-14`):

```cmake
pkg_get_variable(LIBPINYIN_PKGDATADIR      "libpinyin" "pkgdatadir")
pkg_get_variable(LIBPINYIN_EXECPREFIX      "libpinyin" "exec_prefix")
pkg_get_variable(LIBPINYIN_DATABASE_FORMAT "libpinyin" "database_format")
```

`exec_prefix` is a cargo-c built-in. A missing pkg-config variable
**resolves to the empty string with exit status 0**. The consumer then
configures with an empty system-data path and an empty backend name,
silently.

**The mechanism.** The work splits in two:

- **`build.rs` owns the content.** It bakes the build-time fields of
  `crates/oxpinyin-capi/libpinyin.pc.in` — `@VERSION@`,
  `@LIBPINYIN_BINARY_VERSION@` (15.0) and `@DATABASE_FORMAT@` — into
  `libpinyin.pc.in.baked`. It drops the template's `#` header and leaves
  the install-time `@prefix@`, `@exec_prefix@`, `@libdir@` and
  `@includedir@` for the wrapper. It writes to `$OUT_DIR` and mirrors the
  file to `<target>/<profile>/`, so the wrapper can read it without
  discovering the build-hash directory. Below its header,
  `libpinyin.pc.in` is the pin's template line for line.
- **`just packaging install` owns placement** (`tools/dev/packaging.py`).
  For the one library named on its command line (`libpinyin` or
  `libzhuyin`) it runs `cargo cinstall`, then fills that crate's baked
  template and **overwrites** the incomplete `.pc` that cargo-c installed.
  It fills `@prefix@`, `@exec_prefix@`, `@libdir@` and `@includedir@` as
  the pin's configure does: `--libdir`/`--includedir` verbatim when given,
  otherwise `${exec_prefix}/lib` and `${prefix}/include`.
  `just packaging check-pc-metadata` compares the installed tree with a
  pin install, field for field.

The wrapper is the **supported installation path**. A plain
`cargo capi install` still leaves cargo-c's incomplete `.pc`, with the
four variables absent. Use the wrapper for any install a consumer will
configure against. `Cargo.toml` carries `generate = false` for intent; the
current cargo-c ignores it, so the wrapper overwrites the file.

**`database_format`.** `oxpinyin-store`'s build script selects the token
once from its backend feature, as the pin's `configure.ac:134-135` sets
one `DATABASE_FORMAT`. Both `.pc` files read it through
`DEP_OXPINYIN_STORE_DATABASE_FORMAT`; there is no build-time override. The
default backend is tkrzw, so a default-feature build gives `Tkrzw`
([compatibility policy, "Backends and user data"](compatibility-policy.md#backends-and-user-data)).
A `--no-default-features --features <peer>` build gives its own backend's
token (`BerkeleyDB`, `KyotoCabinet`).

**Gate.** After `just packaging install libpinyin --prefix=$P` on a
default-feature build (with `PKG_CONFIG_PATH=$P/lib/pkgconfig`):

```console
$ pkg-config --variable=pkgdatadir libpinyin       → $P/lib/libpinyin
$ pkg-config --variable=database_format libpinyin  → Tkrzw
$ pkg-config --variable=exec_prefix libpinyin      → $P
$ pkg-config --libs libpinyin                      → -lpinyin -lglib-2.0
$ pkg-config --modversion libpinyin                → 2.11.92
```

All five are non-empty. The version satisfies consumers' `>=`
constraints, such as fcitx's `libpinyin>=2.6.0`; it is a libpinyin
version, not the crate version.

## Notes

- **`Requires: glib-2.0` is mirrored deliberately.** The real `.pc` has
  it, and a consumer that relies on libpinyin to pull GLib in would
  otherwise lose it. `pinyin.h` includes `novel_types.h` and
  `pinyin_custom2.h`, which include `glib.h`; all three ship verbatim.
- **Consumers find the library as `libpinyin`.** There is no `oxpinyin`
  pkg-config module. A consumer uses
  `pkg_check_modules(LibPinyin "libpinyin>=2.6.0" REQUIRED IMPORTED_TARGET)`
  plus the same `pkg_get_variable` reads as fcitx-libpinyin.
- **fcitx-libpinyin also probes for libpinyin's utility binaries**
  (`gen_binary_files`, `gen_unigram`, `import_interpolation`) under
  `${LIBPINYIN_EXECPREFIX}/bin`, and sets `LIBPINYIN_TOOLS_FOUND 0` when
  they are absent. This is not fatal and is a build-time concern only.
  The library install does not place them; `oxpinyin-utils` builds them
  under the pin's names ([trainer layout](../packaging.md#trainer-utility-layout)).
