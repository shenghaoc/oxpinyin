# Pinned behaviour

This is the libpinyin part of the product: the pin's behaviour, including defects.
Stage 2 reproduces it bug for bug.
The [improvements page](oxpinyin-improvements.md) records where oxpinyin may differ.
The [compatibility policy](findings/compatibility-policy.md) defines the exceptions.

## The pin

The pin is libpinyin 2.11.92 at `074a2219`, its default-branch tip on 2026-09-12.
`tools/oracle/oracle-pin.txt` is authoritative.
The reference build is the pin's autoconf build with `--with-dbm=Tkrzw`, the default of `tools/oracle/build.py`; `--dbm` selects another backend.
A bare `./configure` selects Berkeley DB (`configure.ac:94`); that is libpinyin's autoconf default, not the reference build's.
`build.py` uses autoconf's default `-g -O2` without `-DNDEBUG`, so every assertion at the pin is active.

## Identity

- The SONAME is `libpinyin.so.15`, from libtool `-version-info 15:0`, and `libzhuyin.so.15` for the `--enable-libzhuyin` facade.
- Headers install under `include/libpinyin-<pin version>/`.
  `libpinyin.pc` keeps the fields `pkgdatadir`, `database_format` and `exec_prefix`.
  The version follows the pin (`libpinyin_tag`).
- All 79 `pinyin_*` exports and all 52 Zhuyin exports are preserved.
  Consumer usage does not limit ABI scope.

[Drop-in identity](packaging.md#drop-in-identity) states what `dlopen("libpinyin.so.15")` requires of the installed library.

## Behaviour

For every exported symbol, oxpinyin reproduces the pin's result for the same inputs and state:

- Return status, out-parameters and their pointed-to data.
- Written lengths and handle-state transitions.
- Written files and stderr bytes.
- Process-wide side effects.

State includes the on-disk user state of a same-backend user directory.
On Linux, the library and tools use the pin's external input and output calls ([external I/O](findings/compatibility-policy.md#external-io)).
A Rust equivalent requires source proof for every input.
The utilities keep the pin's binary names, options, help, messages, inputs and outputs.
K-mixture working files are backend DBMs.
The `scripts2/` Python build generators are outside this tool scope.

The `errno` that a successful call leaves is not compared unless the API gives it meaning.

## Storage

libpinyin selects Berkeley DB, Kyoto Cabinet or tkrzw through `--with-dbm`.
Bare `./configure` selects Berkeley DB.
oxpinyin builds with one of the same three backends, selected by cargo feature, and with no other.
The [default cell](runbooks/backends.md) follows Cargo.
oxpinyin handles the selected backend's files natively and identically:

- **System data:** the directory opened by `pinyin_init`.
  It contains backend DBMs, `MemoryChunk` library files and `table.conf`.
  `table.conf` determines the libraries and their names, including valid non-stock configurations.
- **User data:** `user.conf`, `user_bigram`, user indexes, `user.bin` and `*.dbin` logs.
  Builds on the same backend use the same user directory.
  Opening the directory with a different backend wipes the nonconforming profile files in place.
  Neither libpinyin nor oxpinyin built with tkrzw can read Berkeley DB user files.

## Pin defects

oxpinyin reproduces observable pin defects and records them as [upstream defects](findings/upstream-report-drafts.md) instead of fixing them.
They include:

- `pinyin_init` and `zhuyin_init` leave process `LC_NUMERIC` at `"C"`.
- On Linux, the `mmap … failed!` and `open … failed!` writers reproduce the pin’s stderr bytes where the registered dispositions require them.
- Behaviour on defective dependency builds is reproduced, including the tkrzw LTO sentinel.
- Predicted-candidate ranking and training follow rules derived from the pin's source, not tuning until examples agree.

Only the grounds that the compatibility policy lists permit departures ([the default is the pin](findings/compatibility-policy.md#the-default-is-the-pin)).
An undefined read follows its class (b) entry in the [register](findings/upstream-divergences.md).
Where the pin aborts, oxpinyin aborts with `std::process::abort()` ([failure behaviour](findings/compatibility-policy.md#failure-behaviour)).
The C ABI, ownership, on-disk formats and data ownership are preserved exactly.
See the [native Rust policy](findings/compatibility-policy.md#native-rust-implementation).

## Deriving behaviour

Behaviour comes from the pin's source first, and from the disassembly of the pin-built object that contains the code when the source does not settle the question.
The reference cell is the default; backend-specific code uses its backend, and Zhuyin uses `libzhuyin.so.15`.
The [Stage 2 policy](source-policy.md#stage-2) defines the compatibility target.
