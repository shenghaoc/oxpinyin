# Utility compatibility cases

The pin's standalone utilities are checked by 331 ordinary Rust integration tests.
Their results were frozen from the pin in #1656 at `b1b4c6d9` and are written in the tests as Rust values.
Running a test never captures the pin, reads a data file of expected results, or writes an expected value.

```sh
cargo test --locked -p oxpinyin-utils
cargo nextest list --locked -p oxpinyin-utils
```

The default cell is tkrzw. Select a peer with `--no-default-features --features bdb` or `--no-default-features --features kyotocabinet`.
These tests need neither the pin nor downloaded model data. They reuse the committed `fixtures/w3/<backend>` and other shared inputs.
As before, the frozen cases run on Unix; the existing portable utility tests also run on Windows.

## Test layout

Each original case ID is one test, named by the ID with hyphens replaced by underscores.
Cases with the same shape are `#[case::<name>]` arguments of one `#[rstest]` function,
whose nextest names are `frozen::<utility>::captured_cases::case_<number>_<name>`.
Accept rstest's numeric prefixes, including zero padding for larger groups.
The four unique save-refusal scenarios remain plain `#[test]` functions,
named `frozen::<utility>::<name>`.
The binary containing the test is its existing integration suite:

| Suite | Utilities |
|---|---|
| `segmentation` | `ngseg`, `spseg`, `mergeseq` |
| `kmm_tools` | The seven K-mixture-model tools |
| `training` | N-gram generation, unigram generation, interpolation estimation and correction evaluation |
| `storage_tools` | Binary-file generation, interpolation import/export, and the parser, lookup, table, chunk and phrase-index test programs |
| `dbm_programs` | `test_ngram`, `test_flexible_ngram`, `test_punct_table` |
| `gen_pinyin_table`, `gen_zhuyin_table` | Their respective table generators |
| `k_mixture_model_to_interpolation` | The interpolation converter |

Each case declares its arguments, stdin, input bytes or shared fixture, and its expected `Run`.
Comments above each case give its original ID and pin source citation at `074a2219`,
any policy record, and any platform exception.
Shared setup uses `#[fixture]` and `assert_fs`; commands use
`assert_cmd::cargo::cargo_bin_cmd!`, with the existing 300-second timeout.
The group function sets up its files, runs its utility, then calls `check`.
There is no invocation interpreter, custom test runner or fixture-update mode.
`tests/common/mod.rs` contains only the complete file inventory, DBM row reader
and narrow masks, with their internal traversal and digest operations.
The backend w3 copy and save-refusal setup in `tests/fixtures/mod.rs` are shared rstest fixtures.
Groups with nested working directories place that copy under `data/`.
The unigram group retains its explicit subset of the bdb chunk inputs.

## Expectations

`tests/common/expected.rs` defines `Run` and `check`:

- `status` is `Exit(code)` or `Signal(number)`.
- `stdout` is `Exact(bytes)`, or `GlibOption(text)` for GLib help.
  `GlibOption` holds the pin's `[OPTION?]` spelling and accepts exactly that text
  with `[OPTION…]` or `[OPTION...]` in its place, the spellings of other GLib versions.
- `stderr` is the exact bytes.
- `files` lists each path whose state after the run is not its pre-run bytes:
  `Sha256(digest, length)` of the bytes, or `Rows(digest)` of a DBM file's sorted rows.

`check` compares the status and both streams byte for byte.
It then expands `snapshot_inputs()`, taken before the run, into the expected tree:
every pre-run file unchanged, every pre-run directory present, and the paths in `files`.
That tree must equal the complete inventory after the run, including paths and file types.
A read-only case therefore has `files: &[]`, and any changed, missing or extra path fails.

Cell, OS and architecture differences are explicit arms inside the value:
`match CELL`, `cfg!(target_os = "macos")` and `cfg!(target_arch = "aarch64")`.
`CELL` is derived from the default store extension, as before.
No entry is selected at run time, and no platform falls back to another's value.

Short streams are byte literals; non-ASCII text is a string literal with `.as_bytes()`.
Values over 200 bytes are files under `tests/fixtures/<utility>/`, named by content
and included with `include_bytes!` or, for GLib help, `include_str!`.
Byte-identical values share one file.
A value that more than one expectation uses is a named constant in the narrowest scope
holding its uses: the utility's module, the suite's `frozen` module, or `common/expected.rs`.
A shared value that depends on the cell is a function, because a constant cannot match a `&str`.

The row digest retains keys, values and empty values.
The tests keep the original cell-specific choice between logical DBM rows and container bytes.

## Masks

Volatile text uses the existing masks, not snapshot redactions.
Two masks act on binary data that a text redaction cannot express:
the pinyin-index padding in DBM row values and the tkrzw clock bytes in container files.

- Print-time clock fields, retaining the operation count.
- GLib diagnostic process ID and time.
- Odd-length `PinyinIndexItem2` tail padding in the named DBM files.
- Eight tkrzw HashDBM clock bytes in the named files after the registered abort.
- Only the test's private `OXPINYIN_TEST_TMP` prefix back to `/tmp` in diagnostics.

The fixed-name programs run under private scratch directories; the default remains `/tmp`.
Sealed inputs remain sealed memory files on Linux (including when tests run as root), and read-only files on macOS.
`frozen_observations.rs` tests the masks, the row reader and `check`.

The macOS notes remain beside their cases: libc error numbers, stdio flushing on abort and page-size-dependent container sizes.
Architecture-dependent NaN spellings and the three accepted GLib help spellings remain unchanged.
Policy-accepted cases cite their records, which state the pin's result and its source derivation.

## Pin bumps and recapture

Recapture is a separate, explicit maintenance step outside the test binaries.
Open item: provide and document that external step for a future pin bump, including repeat-run stability checks and review of architecture, OS and policy variants.
This change does not implement recapture tooling or take new pin captures.
Do not regenerate expected values from oxpinyin test output.
