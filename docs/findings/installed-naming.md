# The installed tree takes libpinyin's name; the source tree keeps ours

Date: 2026-08-28 · Status: **implemented** · Branch:
`feat/libpinyin-installed-naming`.

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

Two details worth stating because they are easy to get wrong: the module
name is **`libpinyin`** while the link flag is **`-lpinyin`** (pkg-config
name and library name differ), and `database_format` is **`KyotoCabinet`**,
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

## The resolution — build.rs generates the `.pc`, a wrapper installs it

`pkgdatadir`, `database_format`, `libpinyinincludedir` and
`libpinyin_binary_version` are **not producible through cargo-c**. This
is not a configuration miss; it is a fixed limit, verified against
cargo-c 0.10.24's own source:

- `build.rs:358` — the `PkgConfigCApiConfig` struct has exactly seven
  fields (`name`, `filename`, `description`, `version`, `requires`,
  `requires_private`, `strip_include_path_components`) and no
  custom-variable map. The table is parsed leniently, so extra keys —
  `generate` included — are silently ignored, which is how the probe for
  `pkgdatadir` / `database_format` failed quietly rather than erroring.
- No install prefix reaches build scripts: the crate's only `set_var`
  calls are `CARGO_C_CARGO` (`config.rs:11`) and `INLINE_C_RS_CFLAGS`
  (`build.rs:1402`). There is no `CARGO_CAPI_PREFIX` — a build script
  cannot know `--prefix`.
- `install.rs:216-220` installs cargo-c's own `.pc` into
  `<libdir>/pkgconfig` unconditionally and first; `install.rs:233-236`
  installs build-generated data assets under `datadir`, never the
  pkg-config dir. A `.pc` a build script writes cannot be routed into
  place through `cargo cinstall`.

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
backend name, silently. That is the failure mode this closes.

**The chosen mechanism — option (iii) with a placement wrapper.** Because
cargo-c can neither be told to skip its `.pc` (no `generate = false`), be
handed a build-generated one (data assets land in `datadir`), nor expose
the prefix to build.rs, the work splits in two:

- **`build.rs` owns the content.** It bakes the build-time fields of
  `crates/oxpinyin-capi/libpinyin.pc.in` — `@VERSION@` (2.11.91),
  `@LIBPINYIN_BINARY_VERSION@` (15.0) and `@DATABASE_FORMAT@` — into
  `libpinyin.pc.in.baked`, dropping the template's `#` header (it
  describes the source, not the installed file) and leaving the
  install-time `@prefix@` / `@libdir@` for the wrapper. It writes to
  `$OUT_DIR` and mirrors the file to `<target>/<profile>/` so the wrapper
  can read it without discovering the build-hash directory.
- **`tools/packaging/install.sh` owns placement.** It runs
  `cargo cinstall --prefix=… --libdir=…`, then substitutes the real
  `prefix`/`libdir` into the baked template and **overwrites** the
  incomplete `libpinyin.pc` cargo-c just installed.

The wrapper is the **supported installation path**. A plain
`cargo capi install` still leaves cargo-c's incomplete `.pc` — the
**accepted silent window**: the four variables are absent and
`pkg_get_variable` reads empty, exactly as before. Use the wrapper for
any install a consumer will configure against. `Cargo.toml` carries
`generate = false` for intent and forward compatibility; 0.10.24 ignores
it (hence the overwrite), but a future cargo-c that honours it would drop
the redundant first write.

**The `database_format` caveat.** build.rs sets it to the active backend
— `redb` by default (oxpinyin's native store, and until a backend feature
is forwarded onto `oxpinyin-capi`, the only one reachable here), `LMDB`
or `Tkrzw` when such a feature is enabled. A packager building the drop-in
against another engine's data (`KyotoCabinet`, `Tkrzw`) must set
`LIBPINYIN_DATABASE_FORMAT=<name>` at build time; otherwise the variable
reads `redb` — accurate for oxpinyin's own data, but not what fcitx's
probe expects when the shipped data is in a different format.

**Gate.** After `tools/packaging/install.sh --prefix=$P` (with
`PKG_CONFIG_PATH=$P/lib/pkgconfig`):

```console
$ pkg-config --variable=pkgdatadir libpinyin       → $P/lib/libpinyin
$ pkg-config --variable=database_format libpinyin  → redb
$ pkg-config --variable=exec_prefix libpinyin      → $P
$ pkg-config --libs libpinyin                      → -lpinyin -lglib-2.0
$ pkg-config --modversion libpinyin                → 2.11.91
```

All five come back non-empty — the silent misconfiguration the four
missing variables would otherwise cause is closed on the wrapper path.

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
