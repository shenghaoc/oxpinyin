# GLib and libc call-site audit

Regenerates [glib-libc-call-sites.tsv](../findings/glib-libc-call-sites.tsv), the
call-site list behind the [inventory](../findings/glib-libc-inventory.md) and the
[verification ledger](../findings/glib-libc-verification.md). The list describes the
pin, so it changes only when the pin, the generator or the family table changes.

## Tools

The audit runs natively on macOS. Install these from Homebrew:

```bash
brew install llvm@21 bear glib gettext autoconf automake libtool tkrzw kyoto-cabinet berkeley-db@5
```

`llvm@21` is the keg whose libclang matches the bindings the recipe pins (`clang==21.1.7`
from PyPI; PyPI has no later release). `berkeley-db@5` is the major version
libpinyin's configure asks for. The recipe checks for each tool and names any that is
missing; it installs nothing itself.

On Linux the same recipe needs `autoconf automake libtool bear g++ make pkg-config`,
`libclang-21-dev`, and the development packages of GLib, Berkeley DB, Kyoto Cabinet and
Tkrzw. Debian trixie has `libclang-21-dev` in `trixie-backports`.

## Run

Use a clean checkout of the pin, then:

```bash
just audit-glib-call-sites <pin-checkout>
```

The recipe takes the heavy slot and accepts `--work DIR` (default
`target/glib-call-sites`), `--jobs N`, `--reuse` (read the trees already under `--work`),
`--output FILE`, `--unplaced FILE` and `--check` (compare with the committed list).

## What it does

For each backend (`bdb`, `kc`, `tkrzw`) the generator builds the pin twice:

1. `autoreconf`, `configure --disable-static --enable-libzhuyin --with-dbm=…` and
   `bear -- make -C src`, with the pin's own flags. libclang parses each translation
   unit of the two shared libraries with the recorded flags. A parse error stops the run.
   macOS adds `-std=gnu++17`, because Tkrzw's headers need it and it is GCC's default.
2. The same configure at `-O0`, so a reached function stays out of line, then
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

A call to a GLib or libc function missing from the table stops the run, naming each
call. So does a table function with no site. A macro produces a row only when the table
names it, as `g_array_index`; `assert` and `G_N_ELEMENTS` produce none.
A call inside a system macro belongs to the macro.

A function is matched by clang's mangled name. A `static` function in a `.cpp` file must
also come from that file's object, because `pinyin_lookup2.cpp` and `phrase_lookup.cpp`
define functions with one mangled name. Libclang cannot mangle a template that has not been
instantiated, so a member of a template matches on its qualified name and parameter count
without template arguments.

On macOS the second build rewrites `src/libpinyin.exp` and `src/libzhuyin.exp` from the
`.ver` version scripts. At the pin, `libpinyin.exp` omits five exports `libpinyin.ver` has:
`pinyin_begin_get_bigram_phrases`, `pinyin_bigram_iterator_get_next_phrase`,
`pinyin_bigram_iterator_has_next_phrase`, `pinyin_end_get_bigram_phrases` and
`pinyin_guess_predicted_candidates_with_punctuations`. The version script is the ABI, so both
platforms' links start from the same exports.

## Result to check

The run prints the sites the linker cannot place: functions in no link's map, kept or
removed, usually an inline function nothing calls. They render as `none`. Compare the
list with the [inventory](../findings/glib-libc-inventory.md#reachability-and-negative-coverage).

After a generator or table change, also run the recipe once on Linux and compare the two
lists row by row. A difference is a finding to record, not a reason to require Linux.

`just test-tools` runs the merge and render tests. Its end-to-end test runs where the tools
above are installed and otherwise skips, naming what is missing. No CI job installs libclang.
