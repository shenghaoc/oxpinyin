# GLib and libc call-site audit

This procedure regenerates [glib-libc-call-sites.tsv](../findings/glib-libc-call-sites.tsv).
That call-site list supports the [inventory](../findings/glib-libc-inventory.md) and the
[verification ledger](../findings/glib-libc-verification.md). The list describes the
pin, so it changes only when the pin, the generator or the family table changes.

## Tools

The audit runs natively on macOS. The shebang of the script pins the clang bindings, and
`--print-llvm-major` prints the matching LLVM major. These tools come from Homebrew:

```bash
llvm_major=$(uv run --no-project --managed-python python tools/audit/glib_call_sites.py --print-llvm-major)
brew install "llvm@${llvm_major}" bear glib gettext autoconf automake libtool tkrzw kyoto-cabinet berkeley-db@5
```

The libclang is the `llvm@<major>` one that matches the script's pinned bindings. `berkeley-db@5` is the major
version that the configure of libpinyin asks for. The recipe checks for each tool and names any
tool that is missing. It installs nothing itself.

On Linux, the same recipe needs `autoconf automake libtool bear g++ make pkg-config`,
`libclang-<major>-dev`, and the development packages of GLib, Berkeley DB, Kyoto Cabinet
and Tkrzw. The libclang package is the Debian testing package for the major that the command above prints.

## Run

The run needs a clean checkout of the pin:

```bash
just audit-glib-call-sites <pin-checkout>
```

The recipe accepts these options:

- `--work DIR` (default `target/glib-call-sites`);
- `--jobs N`;
- `--reuse`: read the trees that are already under `--work`;
- `--output FILE`;
- `--unplaced FILE`;
- `--check`: compare with the committed list.

## What it does

For each backend (`bdb`, `kc`, `tkrzw`), the generator builds the pin twice:

1. `autoreconf`, `configure --disable-static --enable-libzhuyin --with-dbm=…` and
   `bear -- make -C src`, with the pin's own flags. libclang parses each translation
   unit of the two shared libraries with the recorded flags. A parse error stops the run.
   macOS adds `-std=gnu++17`, because the Tkrzw headers need it and it is the GCC default.
2. The same configure at `-O0`, so that a reached function stays out of line, then
   `make -C src` and `make -C utils`. The linker removes what no export or `main`
   reaches and writes a map: `-Wl,-dead_strip -Wl,-map,<file>` on macOS,
   `-ffunction-sections -Wl,--gc-sections -Wl,-Map=<file>` on Linux.

| Column | Source |
| --- | --- |
| `family` | [glib_call_sites.families.tsv](../../tools/audit/glib_call_sites.families.tsv), the one hand-kept file, one row per function |
| `library` | The header that declares the function or macro |
| `facade` | Which links keep the enclosing function: `pinyin`, `zhuyin`, `both`, `tools` (only `utils/` programs) or `none`, over all backends |
| `backend` | The file name: `_bdb`, `_kyotodb` or `_tkrzwdb`, otherwise `all` |
| `pin_site`, `pin_call`, `call` | Where the name is written, the name, and its text |

These conditions stop the run:

- a call to a GLib or libc function that is missing from the table (the message names each call);
- a table function with no site.

A macro produces a row only when the table names it, as `g_array_index`; `assert` and `G_N_ELEMENTS` produce none.
A call inside a system macro belongs to the macro.

The generator matches a function by its clang mangled name. A `static` function in a `.cpp` file must
also come from the object of that file, because `pinyin_lookup2.cpp` and `phrase_lookup.cpp`
define functions with one mangled name. Libclang cannot mangle a template that is not
instantiated. A member of a template therefore matches on its qualified name and parameter count,
without template arguments.

On macOS, the second build rewrites `src/libpinyin.exp` and `src/libzhuyin.exp` from the
`.ver` version scripts, so the links of both platforms start from the same exports
([the pin's `.exp` files](../findings/glib-libc-inventory.md#g4-and-g9--containers-allocation-and-the-abi)).

## Result to check

The run prints the sites that the linker cannot place. These are functions in the map of no link, kept or
removed, usually an inline function that nothing calls. They render as `none`. The
list is compared with the [inventory](../findings/glib-libc-inventory.md#reachability-and-negative-coverage).

After a generator or table change, the recipe also runs once on Linux and the two
lists are compared row by row. A difference is a finding to record, not a reason to require Linux.

`just test-tools` runs the merge and render tests. The end-to-end test runs on Linux and macOS,
and both the Linux harness job and the macOS nightly job require it.
