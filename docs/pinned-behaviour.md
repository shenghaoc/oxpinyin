# Pinned behaviour

This is the libpinyin part of the product: the pin's behaviour, including defects.
Stage 2 reproduces it bug for bug.
The [improvements page](oxpinyin-improvements.md) records where oxpinyin may differ.
The [compatibility policy](findings/compatibility-policy.md) defines the exceptions.

## The pin

The pin is libpinyin 2.11.92 at `074a2219`, its default-branch tip on 2026-09-12.
`tools/oracle/oracle-pin.txt` is authoritative.
The reference build uses the pin's autoconf build and passes `--with-dbm=Tkrzw`, as `tools/oracle/build-oracle.sh` does by default; `--dbm` selects another backend.
A bare `./configure` selects Berkeley DB (`configure.ac:94`); that is libpinyin's autoconf default, not the reference build's.
`build-oracle.sh` uses autoconf's default `-g -O2` without `-DNDEBUG`.
Every assertion at the pin is active.

## Identity

- Preserve SONAME `libpinyin.so.15`, from libtool `-version-info 15:0`.
  Use `libzhuyin.so.15` for the `--enable-libzhuyin` facade.
- Install headers under `include/libpinyin-<pin version>/`.
  Preserve `libpinyin.pc` fields `pkgdatadir`, `database_format` and `exec_prefix`.
  The version follows the pin (`libpinyin_tag`; ruling 2026-09-26 UTC, #592).
- Preserve all 79 `pinyin_*` exports and all 52 Zhuyin exports.
  Consumer usage does not limit ABI scope.

## Behaviour

For every exported symbol, reproduce the pin's result for the same inputs and state:

- Return status, out-parameters and their pointed-to data.
- Written lengths and handle-state transitions.
- Written files and stderr bytes.
- Process-wide side effects.

State includes the on-disk user state of a same-backend user directory.
Use the pin's external input and output calls in the library and tools.
A Rust equivalent requires source proof for every input.
Preserve the pin utilities' binary names, options, help, messages, inputs and outputs.
Use backend DBMs for K-mixture working files.
The `scripts2/` Python build generators are outside this tool scope.

Do not compare `errno` left by a successful call unless the API gives it meaning (ruling 2026-10-08 UTC).

## Storage

Libpinyin selects Berkeley DB, Kyoto Cabinet or tkrzw through `--with-dbm`.
Bare `./configure` selects Berkeley DB.
Build oxpinyin with one of the same three backends, selected by cargo feature.
The default is `tkrzw` (maintainer decision 2026-10-30 UTC).
Do not use another backend.
Handle the selected backend's files natively and identically:

- **System data:** the directory opened by `pinyin_init`.
  It contains backend DBMs, `MemoryChunk` library files and `table.conf`.
  `table.conf` determines the libraries and their names, including valid non-stock configurations.
- **User data:** `user.conf`, `user_bigram`, user indexes, `user.bin` and `*.dbin` logs.
  Builds on the same backend use the same user directory.
  A different backend cannot read it and starts fresh (ruling 2026-09-09).
  Neither libpinyin nor oxpinyin built with tkrzw can read Berkeley DB user files.

## Pin defects

Reproduce observable pin defects.
Record them as [upstream defects](findings/upstream-report-drafts.md) instead of fixing them.
Existing examples include:

- `pinyin_init` and `zhuyin_init` leave process `LC_NUMERIC` at `"C"` (#539).
- Raw stderr is byte-identical, including `mmap … failed!` and `open … failed!` writers (#545).
- Behaviour on defective dependency builds is reproduced, including the tkrzw LTO sentinel (#677).
- Predicted-candidate ranking and training follow rules derived from the pin's source.
  Do not tune them until examples agree.

Only the classes and recorded internal language artefacts permit departures.
External I/O still uses the pin's call.
An undefined read follows its class (b) record.
An abort is reproduced with `std::process::abort()` (class (c) retired 2026-10-31 UTC).
Preserve the C ABI, ownership, on-disk formats and data ownership exactly.
See the [native Rust policy](findings/compatibility-policy.md#native-rust-implementation).

## Deriving behaviour

Read the pin's source first.
Then read disassembly from the pin-built object that contains the code.
Use the reference cell by default, the specific backend for backend-specific code, and `libzhuyin.so.15` for Zhuyin.
Follow the [source-reading procedure](source-policy.md#stage-2).
Never experiment against the oracle to discover behaviour.
Cite `path:line` at the pin, or the symbol and instruction offsets, for every reproduced behaviour.
