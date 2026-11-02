# macOS export baseline — what the shipping libpinyin and libzhuyin dylibs export today

Date: 2026-11-02 UTC · Status: record (baseline before the tkrzw cxx rebinding, [#1529](https://github.com/shenghaoc/oxpinyin/issues/1529))

The 2026-11-02 Rust/C++ interoperability ruling (#1529) says the macOS
`cargo-c` library is checked for the tkrzw cxx rebinding. This finding records
what the libraries export before that change, so the change has a baseline.
Nothing in the build changed.

## Conclusion

- Both shipping dylibs export exactly their version-script set by name.
  Missing: 0 of 79 `pinyin_*` and 0 of 52 `zhuyin_*`. Extra: 0 in each, of any
  origin (Rust runtime, other crates, tkrzw, C++ runtime). `nm -gU` lists
  79 and 52 symbols, all type `T`, and nothing else.
- The development cdylibs export the same sets plus the two fixture hooks that
  `just abi exports` permits in `libpinyin_capi.dylib`:
  `oxpinyin_init_for_fixtures` and `oxpinyin_test_set_user_bigram`.
- Nothing in the repository's macOS packaging restricts exports. The exact set
  comes from rustc, which passes its own `-Wl,-exported_symbols_list` for a
  `cdylib` (seen with `--print link-args`) covering the crate's `#[no_mangle]`
  symbols. `build.rs` adds only the ELF SONAME, and the Linux versioned relink
  (docs/packaging.md#export-gate) is skipped on Darwin. Today's result
  therefore depends on no `extern "C"` item with a public symbol name
  existing anywhere in the cdylib's dependency graph.
- **Separate observation, not fixed here:** the release-profile dylibs built
  by `cargo cinstall` do not load on this host. `dlopen` and
  `dyld_info -exports` both refuse them with `mis-aligned LINKEDIT string
  pool`. The `nm` results above are therefore not evidence that the libraries
  run. The development-profile dylibs load. The cause is not isolated (see
  [Load failure](#load-failure)).

## Environment

| Item | Value |
| --- | --- |
| Base | `origin/main` f3d08577b7a5f159f0d1624c52aedfb7d8462c26 |
| Host | Apple M5, arm64, macOS 27.2 (build 26B123), Xcode 27.2 (27B52) |
| Toolchain | rustc 1.97.1 (8bab26f4f 2026-07-14), cargo 1.97.1, `aarch64-apple-darwin` |
| Linker / compiler | ld PROJECT:ld-27106.1, Apple clang 21.0.0 (clang-2100.4.1.1) |
| cargo-c | 0.10.24+cargo-0.98.0 |
| Backend | tkrzw (default); Homebrew formula tkrzw 1.0.32, `libtkrzw.1.dylib` current version 1.75.0 |
| Build | `--release --locked --no-default-features`, features `tkrzw,shipped` (libpinyin) and `tkrzw` (libzhuyin); fat LTO, one codegen unit |
| Deployment target | `LC_BUILD_VERSION` platform MACOS, minos 11.0 |
| Install name | `/usr/local/lib/libpinyin.15.dylib`, `/usr/local/lib/libzhuyin.15.dylib` |

SHA-256 of the built files: `libpinyin.15.0.0.dylib`
`82170cb6ec8f44a62d9e7a6dea113ba04e984a3dd47897ceecc565d234d956fa`,
`libzhuyin.15.0.0.dylib`
`9e3cc38ef07765e1f28debec5e4f736a75d82c385fe38610916766baf7f0bf6d`.
The cargo-c output in `target/<triple>/release` is byte-identical to the
installed file for both.

## Symbol tables

Shipping dylibs (`just packaging install`, release profile):

| Library | `nm -gU` symbols | Type | In `.ver` | Missing | Extra: Rust runtime | Extra: other crates | Extra: tkrzw | Extra: C++ runtime | Extra total |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| libpinyin | 79 | all `T` | 79 | 0 | 0 | 0 | 0 | 0 | 0 |
| libzhuyin | 52 | all `T` | 52 | 0 | 0 | 0 | 0 | 0 | 0 |

Development cdylibs (`just abi exports`, dev profile, which is
`optimized + debuginfo`):

| Library | `nm -gU` symbols | Missing | Extra | Extra names |
| --- | ---: | ---: | ---: | --- |
| libpinyin_capi.dylib | 81 | 0 | 2 | `oxpinyin_init_for_fixtures`, `oxpinyin_test_set_user_bigram` (fixture hooks, permitted) |
| libzhuyin_capi.dylib | 52 | 0 | 0 | none |

The 79 and 52 names are the `global:` lists of `crates/oxpinyin-capi/libpinyin.ver`
and `crates/oxpinyin-zhuyin-capi/libzhuyin.ver`. Names compare after removing
the leading underscore Mach-O adds. The sets are identical in both directions
(`comm` on the sorted lists is empty on both sides).

`dyld_info -exports` agrees on the development dylibs (81 and 52 exported
names that start `_pinyin_`/`_oxpinyin` and `_zhuyin_`). On the release dylibs
it prints only `mis-aligned LINKEDIT string pool, fileOffset=...`, so `nm` is
the only export evidence for the shipping files.

## Existing gate

`just abi exports` and `just abi exports --shipped` ran on macOS with no
change and both print `PASS: both libraries export exactly their version
script's set`. Three limits apply to what that proves on macOS:

1. It checks the dev-profile cdylibs under `target/<dir>/debug`, not the
   release dylib `cargo cinstall` installs. This finding compared the release
   file by hand.
2. On Darwin `exported_symbols` in `tools/dev/abi.py` accepts only `nm` type
   `T`. Linux accepts `T`, `t`, `W` and `w`. A data export (`D`, `S`, `B`) or
   an absolute symbol on macOS would be invisible to the gate.
3. It never loads the library, so it passes on the dylibs that `dlopen`
   rejects.

Changing any of these needs a tooling change, which this PR does not make.

## What an exact set needs

It already holds, by rustc's generated list, so the proposal is a guard
against #1529 and later changes. Untested proposals, not implemented:

- cxx bridges emit `extern "C"` items with `export_name` for the generated C++
  glue. Rustc exports such upstream symbols from a `cdylib` when it keeps
  them. If the tkrzw rebinding adds any, the first place they would show is
  `nm -gU` on the release dylib. Run this finding's commands again after that
  change and compare to the tables above.
- A second `-exported_symbols_list` generated from the `.ver` files would not
  restrict anything: Apple `ld` merges multiple lists, so rustc's list stays in
  force. A restricting list needs a link step that rustc does not own, which is
  what the Linux relink does. The macOS analogue is a relink of the
  `libpinyin.a` / `libzhuyin.a` staticlib with `-dynamiclib
  -exported_symbols_list <list derived from the .ver files> -install_name ...`,
  checked in both directions as the Linux relink does. That is a packaging
  change and a new gate.
- Cheaper first step: make the macOS parser in `exported_symbols` take every
  external defined type, add a `nm` type table to its output, and point the
  gate at the installed release dylib. A `dlopen` plus `dlsym` of one
  symbol per library would also catch the load failure below.

## Load failure

`cc` smoke: `dlopen(path, RTLD_NOW)` on the release dylibs fails with
`mis-aligned LINKEDIT string pool, fileOffset=0x0020A344` (libpinyin) and
`0x001CC99C` (libzhuyin). The same call on the dev-profile dylibs succeeds and
`dlsym` finds `pinyin_init` / `zhuyin_init`. The release file's `LC_SYMTAB`
has `stroff` 2138948, which is 4 but not 8 byte aligned; the dev file's
`stroff` 3381624 is 8 byte aligned. Removing the code signature or running
`strip -x` on a copy does not change the failure. The repository's macOS
packaging runs no load or smoke test on Darwin (`release-stage` is Linux-only:
`readelf`, `.so` names), so nothing in CI covers this. Whether the fault is
the linker's output for fat LTO, this macOS 27.2 dyld being stricter than
earlier releases, or a Rust-side cause is not established. It needs its own
issue.

Fixed by #1537 (closes #1536): Rust 1.99.1, and an installed-dylib load check in `just packaging install`.

## Reproduction

From a checkout of `origin/main` f3d08577 on an Apple silicon Mac, after
[`docs/runbooks/setup.md`](../runbooks/setup.md) (`brew install tkrzw glib
pkgconf`, Homebrew Bash, rustup, `cargo-c`, `just doctor` clean):

```bash
export LIBRARY_PATH="$(brew --prefix)/lib" CARGO_TARGET_DIR=$PWD/target/tkrzw
S=$PWD/target/macos-stage
just packaging install libpinyin --prefix=/usr/local --destdir=$S -- --release --locked --no-default-features --features tkrzw,shipped
just packaging install libzhuyin --prefix=/usr/local --destdir=$S -- --release --locked --no-default-features --features tkrzw
```

`just packaging install` runs `cargo cinstall` inside the heavy slot.

```bash
L=$S/usr/local/lib
for l in pinyin zhuyin; do
  nm -gU $L/lib$l.15.0.0.dylib | awk '{print $2}' | sort | uniq -c   # type counts
  nm -gU $L/lib$l.15.0.0.dylib | awk '{print $3}' | sed 's/^_//' | sort > /tmp/$l.nm
done
awk '/global:/{a=1;next}/local:/{a=0}a&&/;/{gsub(/[ \t;]/,"");print}' crates/oxpinyin-capi/libpinyin.ver | sort > /tmp/pinyin.ver
awk '/global:/{a=1;next}/local:/{a=0}a&&/;/{gsub(/[ \t;]/,"");print}' crates/oxpinyin-zhuyin-capi/libzhuyin.ver | sort > /tmp/zhuyin.ver
for l in pinyin zhuyin; do comm -13 /tmp/$l.nm /tmp/$l.ver; comm -23 /tmp/$l.nm /tmp/$l.ver; done   # missing, then extra: both empty
dyld_info -exports $L/libpinyin.15.0.0.dylib          # prints the LINKEDIT error
otool -l $L/libpinyin.15.0.0.dylib | grep -A9 LC_SYMTAB | grep stroff
```

Development cdylibs and the existing gate (no tooling change):

```bash
just abi exports --shipped
just abi exports
nm -gU target/tkrzw/debug/libpinyin_capi.dylib | grep -v ' _pinyin_'
dyld_info -exports target/tkrzw/debug/libpinyin_capi.dylib
```

The load check compiles a four-line C program that calls
`dlopen(argv[1], RTLD_NOW)` then `dlsym(h, argv[2])` and runs it on each dylib.
The rustc link argument was seen with
`cargo rustc --locked -p oxpinyin-zhuyin-capi --lib --crate-type cdylib -- --print link-args`.
