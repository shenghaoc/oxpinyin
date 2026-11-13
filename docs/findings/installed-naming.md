# The installed tree takes libpinyin's name; the source tree keeps ours

Status: **implemented**.

The source repository keeps its own name. The installed artifacts take
the name that the ecosystem already knows. Other projects do the same:

- LibreSSL installs `libssl.so` and `libcrypto.so`.
- MariaDB installs `libmysqlclient.so` and `mysqlclient.pc`.
- Mesa installs `libGL.so`.

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

Note two details:

- The module name is **`libpinyin`**, but the link flag is **`-lpinyin`**.
- `database_format` is one word with no space (`KyotoCabinet`).

`pkgdatadir` points at `@libdir@/libpinyin`, one level *above* where the
data lives (`@libdir@/libpinyin/data`).

## What the install produces

The version is `libpinyin_tag` in `tools/oracle/oracle-pin.txt`
(2.11.92). `just packaging install` reads it. It also rejects a
`[package.metadata.capi]` table that disagrees, as
`just packaging check-pc-version` does.
`just packaging pc-version` prints the version for package assembly.

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

## The mechanism — a wrapper generates and installs the `.pc`

cargo-c **cannot produce** `pkgdatadir`, `database_format`,
`libpinyinincludedir` or `libpinyin_binary_version`. This is a fixed limit
of cargo-c, read from the cargo-c 0.10.24 source:

- `build.rs:358`: the `PkgConfigCApiConfig` struct has exactly seven
  fields (`name`, `filename`, `description`, `version`, `requires`,
  `requires_private`, `strip_include_path_components`) and no
  custom-variable map. The parser is lenient, so it silently ignores extra
  keys, `generate` included.
- No install prefix reaches build scripts. The crate's only `set_var`
  calls are `CARGO_C_CARGO` (`config.rs:11`) and `INLINE_C_RS_CFLAGS`
  (`build.rs:1402`). A build script cannot know `--prefix`.
- `install.rs:216-220` always installs cargo-c's own `.pc` into
  `<libdir>/pkgconfig`, and does so first. `install.rs:233-236` installs
  build-generated data assets under `datadir`, never in the pkg-config
  directory.

**Why it matters.** fcitx-libpinyin reads three of these at cmake
configure time (`CMakeLists.txt:12-14`):

```cmake
pkg_get_variable(LIBPINYIN_PKGDATADIR      "libpinyin" "pkgdatadir")
pkg_get_variable(LIBPINYIN_EXECPREFIX      "libpinyin" "exec_prefix")
pkg_get_variable(LIBPINYIN_DATABASE_FORMAT "libpinyin" "database_format")
```

`exec_prefix` is a cargo-c built-in. A missing pkg-config variable
**resolves to the empty string with exit status 0**. The consumer then
silently configures with an empty system-data path and an empty backend
name.

**The mechanism.** The work splits in two:

- **`tools/dev/packaging.py` owns the content.** It fills the build-time
  fields of `crates/oxpinyin-capi/libpinyin.pc.in`. These fields are
  `@VERSION@`, `@LIBPINYIN_BINARY_VERSION@` (15.0) and
  `@DATABASE_FORMAT@`. It drops the template's `#` header. No build
  script writes the file, and nothing is read from Cargo's build
  directory. Below its header, `libpinyin.pc.in` is the pin's
  template line for line.
- **`just packaging install` owns placement** (`tools/dev/packaging.py`).
  For the one library named on its command line (`libpinyin` or
  `libzhuyin`) it runs `cargo cinstall`, then fills that crate's
  template and **overwrites** the incomplete `.pc` that cargo-c installed.
  It fills `@prefix@`, `@exec_prefix@`, `@libdir@` and `@includedir@` as
  the pin's configure does: `--libdir`/`--includedir` verbatim when given,
  otherwise `${exec_prefix}/lib` and `${prefix}/include`.
  `just packaging check-pc-metadata` compares the installed tree with a
  pin install, field for field.

The wrapper is the **supported installation path**. A plain
`cargo capi install` leaves cargo-c's incomplete `.pc`, without the four
variables. Use the wrapper for any install that a consumer will configure
against. `Cargo.toml` carries `generate = false` to state intent. The
current cargo-c ignores it, so the wrapper overwrites the file.

**`database_format`.** `oxpinyin-store`'s build script selects the
library's token once from its backend feature, as the pin's
`configure.ac:134-135` sets one `DATABASE_FORMAT`. Both `.pc` files take
the token of the installed backend from `BACKEND_FORMATS` in
`tools/dev/cell.py`. There is no build-time override. The
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

All five are non-empty. The version is a libpinyin version, not the
crate version. It satisfies consumers' `>=` constraints, such as fcitx's
`libpinyin>=2.6.0`.

## Notes

- **The `.pc` mirrors `Requires: glib-2.0` on purpose.** The real `.pc`
  has it. Without it, a consumer that relies on libpinyin to pull in GLib
  would lose GLib. `pinyin.h` includes `novel_types.h` and
  `pinyin_custom2.h`, which include `glib.h`. All three headers ship
  verbatim.
- **Consumers find the library as `libpinyin`.** There is no `oxpinyin`
  pkg-config module. A consumer uses
  `pkg_check_modules(LibPinyin "libpinyin>=2.6.0" REQUIRED IMPORTED_TARGET)`
  plus the same `pkg_get_variable` reads as fcitx-libpinyin.
- **fcitx-libpinyin also probes for libpinyin's utility binaries**
  (`gen_binary_files`, `gen_unigram`, `import_interpolation`) under
  `${LIBPINYIN_EXECPREFIX}/bin`. It sets `LIBPINYIN_TOOLS_FOUND 0` when
  they are absent. This is not fatal and affects the build only. The
  library install does not place them. `oxpinyin-utils` builds them under
  the pin's names ([trainer layout](../packaging.md#trainer-utility-layout)).
