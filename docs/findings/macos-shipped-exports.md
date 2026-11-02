# macOS shipped-library export control

Date: 2026-11-02 UTC. Base: `dcc8a593`. Related: [#1533](https://github.com/shenghaoc/oxpinyin/pull/1533).

## Linker constraints and decision

Native host: Apple M5, arm64, macOS 27.2 (26B123), Xcode 27.2,
Apple linker 27106.1. Rust is the repository-selected 1.99.1.
The baseline used cargo-c 0.10.24; the nightly provisioning uses 0.10.25.
Native dependencies for the measurement were tkrzw 1.0.32 and GLib 2.90.1.

Before this change, cargo-c's Rust-linked dylib was installed unchanged.
Both release libraries already matched their `.ver` sets: libpinyin 79,
libzhuyin 52, with zero missing or extra symbols in both `nm -gU` and
`dyld_info -exports`. Both loaded. That result depended on the dependency
graph adding no public symbol names outside the facades.

Native probes using the existing `tools/dev/measure/fixtures/pages.c`
established the following:

- `--exclude-libs` is rejected by the Apple linker.
- Nonempty `-exported_symbols_list` and `-unexported_symbols_list` cannot
  be combined. Empty-list probes are insufficient: this linker accepted
  two empty lists.
- Two exported-symbols lists are combined as a union. Lists containing
  `_pages_first` and `_pages_second` produced both exports, rather than
  an intersection or last-list replacement. Consequently a second list
  passed through build.rs cannot narrow rustc's generated cdylib list.
- List names carry the leading underscore; `.ver` globals do not.

[Apple's export guidance](https://developer.apple.com/library/archive/documentation/DeveloperTools/Conceptual/DynamicLibraries/100-Articles/DynamicLibraryDesignGuidelines.html)
and [ld64 option handling](https://github.com/apple-oss-distributions/ld64/blob/main/src/ld/Options.cpp)
describe the list semantics. The union was also checked against the
installed linker, rather than inferred from those sources alone.

The installation therefore relinks cargo-c's existing staticlib under one
list owned by packaging. This makes the restriction part of the dylib's
link, independently of rustc's transitive `#[export_name]` collection.
Only `.ver` globals become list entries, sorted and prefixed with `_`:
79 `_pinyin_*` and 52 `_zhuyin_*`, without fixture hooks or version nodes.

## Correction: targeted strip can remove exports here

The initial claim that strip cannot remove global Mach-O dylib exports was
too broad. With a dylib exporting `_pages_first` and `_pages_second`,
`strip -x` preserved both. However, `strip -s keep.txt -u`, with only
`_pages_first` in the keep file, removed `_pages_second` from both `nm`
and the export trie. The output passed code-signature verification,
loaded, resolved `pages_first` and did not resolve `pages_second`.

The export trie is emitted at link time, but is not immutable afterwards.
[Apple's strip source](https://github.com/apple-oss-distributions/cctools/blob/main/misc/strip.c)
contains explicit trie pruning and restrictions on removing coalesced
symbols used through symbol pointers. The historical warning against
removing globals does not establish that every current strip operation
leaves the trie unchanged.

Relinking was chosen over targeted strip because it establishes the
visibility of every archive symbol during linking, including referenced
runtime helpers, and uses the existing packaging pattern. It avoids
depending on post-link pruning handling every transitive CXX symbol class.
The successful strip probe is evidence about this toolchain and fixture;
it was not a CXX-runtime compatibility test.

## Installed artifact and gate

`tools.dev.packaging.install_library` invokes `relink_darwin` after
cargo-c installation and before its existing load check. `DarwinIdentity`
reads the original install name, compatibility/current versions,
architecture and minimum macOS version. The replacement must preserve
all five exactly. The linker uses the selected backend and GLib link
flags, the Rust staticlib, libc++, iconv and resolver support.

The temporary dylib lives beside its destination. Before atomic
replacement, its full external defined symbol set and export trie must
both equal the `.ver` set, and its identity must equal the original.
The existing symlinks remain unchanged. The replaced native file must
load and supply `pinyin_init` or `zhuyin_init` from that image.

`tools.dev.abi.check_exports` now includes Darwin data, absolute and weak
exports. It compares nm with the export trie, rejects disagreements and
unexpected tool diagnostics, and applies the same exact-set check to the
installed files supplied by the nightly job. Linux behavior is unchanged.

## Verification and added nightly cost

The clean staged release build passed with exactly 79 and 52 symbols in
both inspections. The install names remain `/usr/local/lib/libpinyin.15.dylib`
and `/usr/local/lib/libzhuyin.15.dylib`; compatibility and current versions
remain `15.0.0`, architecture `arm64`, and deployment target `11.0`.
Both replaced files load and their unversioned/major-version symlinks are
unchanged. These fields are compared against the corresponding cargo-c
outputs, not inferred from the command line alone.

The added nightly steps were timed natively on the host above, with
`RUSTFLAGS='-D warnings'`, a fresh cargo-c installation prefix and empty
Cargo build targets. The Cargo registry/download caches were retained.
The two libraries share their release dependency build, as in the job.

| Added work | Wall time |
| --- | ---: |
| Provision cargo-c 0.10.25 with `cargo install --locked` under heavy admission | 422.10 s |
| Both staged release installs, including both relinks, identity/export/load checks, and `just abi exports --shipped` | 62.19 s |
| Total added step execution | **484.29 s (8 min 4.29 s)** |

This is a cold-build local measurement, not a GitHub-hosted runner timing
or a claim about a warm Rust cache. It includes provisioning and both
release builds, rather than quoting only the subsecond symbol check.
No new model data or oracle build is involved.

After the [native setup](../runbooks/setup.md), reproduce the new steps
with a fresh temporary directory. Homebrew's libraries must be on the
normal native compiler search path as described in that setup:

```bash
export RUNNER_TEMP="$(mktemp -d)"
export RUSTFLAGS='-D warnings'
time {
  CARGO_TARGET_DIR="$RUNNER_TEMP/cargo-c-target" \
    just heavy -- cargo install cargo-c --version 0.10.25 --locked --root "$RUNNER_TEMP/cargo-c"
  export PATH="$RUNNER_TEMP/cargo-c/bin:$PATH"
  export CARGO_TARGET_DIR="$RUNNER_TEMP/target"
  stage="$RUNNER_TEMP/macos-shipped"
  just packaging install libpinyin --prefix=/usr/local --destdir="$stage" -- --release --locked --no-default-features --features tkrzw,shipped
  just packaging install libzhuyin --prefix=/usr/local --destdir="$stage" -- --release --locked --no-default-features --features tkrzw
  OXPINYIN_CAPI_SO="$stage/usr/local/lib/libpinyin.15.0.0.dylib" \
  OXPINYIN_ZHUYIN_SO="$stage/usr/local/lib/libzhuyin.15.0.0.dylib" \
    just abi exports --shipped
}
```

For G `.ver` globals and E inspected symbols, Python list generation takes
O(G log G) time and O(G) space; set checks take expected O(G + E) time and
O(G + E) space. G is 79 or 52 here. These bounds exclude the external
compiler, linker and inspection tools. Runtime algorithms are unchanged.

## Transitive dependency negative control

The local mutation used one function and one immutable data symbol in
`crates/oxpinyin-store/src/lib.rs`. Store is a transitive dependency of
both C ABI crates; neither C ABI crate is edited. Temporarily append:

```rust
/// Temporary transitive-export negative control; never committed.
#[allow(unsafe_code)]
#[unsafe(export_name = "oxpinyin_dependency_export_probe")]
pub extern "C" fn dependency_export_probe() -> u32 { 1533 }

/// Temporary data-export negative control; never committed.
#[allow(unsafe_code)]
#[unsafe(export_name = "oxpinyin_dependency_export_data")]
pub static DEPENDENCY_EXPORT_DATA: u32 = 1533;
```

The scoped unsafe-code allowances permit the symbol-name attributes for
this disposable mutation only. They are not production lint changes.
Run the two install commands above again. Point the same shipped gate at
the unrelinked cargo-c outputs to exercise the defective build:

```bash
triple=$(rustc -vV | sed -n 's/^host: //p')
OXPINYIN_CAPI_SO="$CARGO_TARGET_DIR/$triple/release/libpinyin.dylib" \
OXPINYIN_ZHUYIN_SO="$CARGO_TARGET_DIR/$triple/release/libzhuyin.dylib" \
  just abi exports --shipped
```

Remove the temporary mutation after the check. The normal gate command
continues to inspect the installed relinked files rather than these raw
cargo-c outputs.

Observed results:

| Mutated build | libpinyin exports | libzhuyin exports | Shipped gate |
| --- | ---: | ---: | --- |
| Raw cargo-c dylibs | 81 | 54 | FAIL, exit 1; both added names reported for each library |
| Installed relinked dylibs | 79 | 52 | PASS; both replaced files load |

The added data symbol was type `S` and the function type `T` in `nm -gU`.
The control therefore also exercises the former text-only parser gap.
The mutation was removed and is not in the PR. `wip/tkrzw-cxx` was not
built for this check.

Focused verification:

```bash
uv run --no-project --managed-python python -X lazy_imports=normal tools/dev/packaging.test.py
uv run --no-project --managed-python python -X lazy_imports=normal tools/dev/abi.test.py
```

All 35 packaging and 14 ABI tests passed. In particular,
`test_macos_relink_preserves_identity_exports_and_symlinks` checks both
facades, `test_macos_relink_rejects_bad_exports_or_identity_before_replacement`
checks failure atomicity, and `test_shipped_gate_rejects_dependency_data_exports`
and `test_shipped_gate_rejects_exports_hidden_from_nm` check leak detection.

Limitation: the full `just test-tools` run failed in the unchanged
`IdentityTests.test_observe_normalises_fresh_directories_and_reports_leaks`
native-executable probe (one execution returned SIGKILL). Its isolated
9-test suite passed through `just test-tools tools/dev/runner_identity.test.py`
on both the untouched base and this branch. The full-suite failure is not
reported as a pass.
