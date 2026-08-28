# The installed tree takes libpinyin's name; the source tree keeps ours

Date: 2026-08-28 · Status: **implemented, with one open decision** ·
Branch: `feat/libpinyin-installed-naming`.

Precedent is universal — LibreSSL installs `libssl.so`/`libcrypto.so`,
MariaDB installs `libmysqlclient.so` and `mysqlclient.pc`, Mesa installs
`libGL.so`. The source repository keeps its own identity; the installed
artifacts take the name the ecosystem already knows. PR 3 did the
SONAME; this completes the rest of the installed tree.

## What the real package actually looks like

Read off Ubuntu's `libpinyin15-dev` rather than assumed:

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
database_format=BerkeleyDB

libpinyinincludedir=${includedir}/libpinyin-2.8.1
libpinyin_binary_version=15.0

Name: libpinyin
Description: Library to deal with pinyin
Version: 2.8.1
Requires: glib-2.0
Libs: -L${libdir} -lpinyin
Cflags: -I${libpinyinincludedir}
```

Two details worth stating because they are easy to get wrong: the module
name is **`libpinyin`** while the link flag is **`-lpinyin`** (pkg-config
name and library name differ), and `database_format` is **`BerkeleyDB`**,
one word, no space.

`pkgdatadir` points at `@libdir@/libpinyin` — one level *above* where the
data actually lives (`@libdir@/libpinyin/data`).

## What this PR produces

```
/usr/include/libpinyin-2.11.91/pinyin.h
/usr/lib/x86_64-linux-gnu/libpinyin.a
/usr/lib/x86_64-linux-gnu/libpinyin.so
/usr/lib/x86_64-linux-gnu/libpinyin.so.15
/usr/lib/x86_64-linux-gnu/libpinyin.so.15.0.0
/usr/lib/x86_64-linux-gnu/pkgconfig/libpinyin.pc
```

`find <prefix> -name '*oxpinyin*'` → nothing. Measured gates:

| Gate | Result |
| --- | --- |
| `pkg-config --libs libpinyin` | `-lpinyin` (plus `-lglib-2.0` from Requires, as the real one) |
| `pkg-config --cflags libpinyin` | `-I…/include/libpinyin-2.11.91` |
| `pkg-config --modversion libpinyin` | `2.11.91` — satisfies fcitx's `libpinyin>=2.6.0` |
| `pkg-config --variable=exec_prefix libpinyin` | the prefix |
| `readelf -d libpinyin.so.15.0.0` | `SONAME libpinyin.so.15` |
| `cc $(pkg-config --cflags --libs libpinyin) test.c` | compiles, links, runs; `DT_NEEDED: libpinyin.so.15` |

## The open decision — four `.pc` variables cargo-c cannot emit

`pkgdatadir`, `database_format`, `libpinyinincludedir` and
`libpinyin_binary_version` are **not producible through cargo-c**. This
is not a configuration miss; it is a fixed limit, read from cargo-c
0.10.25's own source:

- `build.rs:619-666` reads exactly seven keys from
  `[package.metadata.capi.pkg_config]`: `name`, `filename`,
  `description`, `version`, `requires`, `requires_private`,
  `strip_include_path_components`. Anything else in the table is
  silently ignored — which is how the probe for `pkgdatadir` and
  `database_format` failed quietly rather than erroring.
- `pkg_config_gen.rs:61-80`'s `PkgConfig` struct has a closed field set
  (`prefix`, `exec_prefix`, `includedir`, `libdir`, `name`,
  `description`, `version`, `requires`, `requires_private`, `libs`,
  `libs_private`, `cflags`, `conflicts`). There is no custom-variable
  map to populate.

**Why it matters, and why it is worse than a build break.**
fcitx-libpinyin reads three of these at cmake configure time
(`CMakeLists.txt:12-14`):

```cmake
pkg_get_variable(LIBPINYIN_PKGDATADIR      "libpinyin" "pkgdatadir")
pkg_get_variable(LIBPINYIN_EXECPREFIX      "libpinyin" "exec_prefix")
pkg_get_variable(LIBPINYIN_DATABASE_FORMAT "libpinyin" "database_format")
```

`exec_prefix` is a cargo-c built-in and resolves. The other two do not,
and a missing pkg-config variable **resolves to the empty string with
exit status 0** — measured, not assumed. So the consumer does not fail
to configure; it configures with an empty system-data path and an empty
backend name, silently. That is the failure mode to avoid.

`crates/oxpinyin-capi/libpinyin.pc.in` is checked in carrying the exact
content that has to be installed. What is open is only **how** it gets
there:

1. a post-install substitution step in the packaging recipe, overwriting
   cargo-c's generated file;
2. a patch to cargo-c adding a custom-variable table (upstreamable — the
   limitation is general, not oxpinyin-specific);
3. dropping cargo-c for the `.pc` and installing the template directly.

This is the STOP the brief names ("cargo-c's naming options cannot
produce the required layout without a wrapper script"). Nothing else in
the naming work depends on the answer.

## Notes carried forward

- **`Requires: glib-2.0` is mirrored deliberately.** oxpinyin's
  `pinyin.h` is glib-free (`stddef`/`stdbool`/`stdint`), so we do not
  need it — but the real `.pc` has it, consumers reach us through
  cmake's `IMPORTED_TARGET`, and a consumer relying on libpinyin to pull
  glib in would otherwise lose it. One line to drop if we would rather
  not impose the dependency.
- **Version is `2.11.91`**, the pin oxpinyin targets, not the crate
  version. It has to be a libpinyin version to satisfy consumers'
  `>=` constraints.
- **fcitx5-oxpinyin follow-up, NOT changed here.** Its `CMakeLists.txt`
  has `pkg_check_modules(OXPINYIN oxpinyin)`, which stops resolving once
  the `.pc` is renamed. It must become
  `pkg_check_modules(LibPinyin "libpinyin>=2.6.0" REQUIRED IMPORTED_TARGET)`
  plus the same `pkg_get_variable` reads fcitx-libpinyin does. Separate
  repository, separate change.
- **Header completeness is a separate gap.** The real `pinyin.h`
  includes `novel_types.h` and `pinyin_custom2.h`; oxpinyin ships
  neither. A consumer relying on those transitively still breaks, which
  is an abi-subset §8 matter rather than a naming one.
- **fcitx-libpinyin also probes for libpinyin's utility binaries**
  (`gen_binary_files`, `gen_unigram`, `import_interpolation`) under
  `${LIBPINYIN_EXECPREFIX}/bin`, and sets `LIBPINYIN_TOOLS_FOUND 0` when
  they are absent. Not fatal, and a build-time concern only, but the
  drop-in does not provide them.
