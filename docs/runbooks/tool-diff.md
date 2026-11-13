# Utility compatibility cases

This page describes how to run and extend the frozen compatibility cases of the pin's standalone utilities.
Ordinary Rust integration tests check these utilities.
Their results are frozen from the pin and written in the tests as Rust values.
A test run never captures the pin, reads a data file of expected results, or writes an expected value.

To run the cases, or to list them:

```sh
cargo test --locked -p oxpinyin-utils
cargo nextest list --locked -p oxpinyin-utils
```

The default cell is tkrzw. `--no-default-features --features bdb` or `--no-default-features --features kyotocabinet` selects a peer.
These tests need neither the pin nor downloaded model data. They reuse the committed `fixtures/w3/<backend>` and other shared inputs.
The frozen cases run on Unix; the portable utility tests also run on Windows.

## Test layout

Each original case ID is one test. Its name is the ID with hyphens replaced by underscores.
Cases with the same shape are `#[case::<name>]` arguments of one `#[rstest]` function.
Their nextest names are `frozen::<utility>::captured_cases::case_<number>_<name>`.
The numeric prefixes of rstest, including zero padding for larger groups, are accepted.
The four unique save-refusal scenarios stay plain `#[test]` functions,
named `frozen::<utility>::<name>`.
Each test is in its existing integration suite:

| Suite | Utilities |
|---|---|
| `segmentation` | `ngseg`, `spseg`, `mergeseq` |
| `kmm_tools` | The seven K-mixture-model tools |
| `training` | N-gram generation, unigram generation, interpolation estimation and correction evaluation |
| `storage_tools` | Binary-file generation and interpolation import/export |
| `oxpinyin-repl` / `interactive` | Ten interactive modes, with retained product outputs and saved-file witnesses; obsolete timing lines are excluded |
| `gen_pinyin_table`, `gen_zhuyin_table` | Their respective table generators |
| `k_mixture_model_to_interpolation` | The interpolation converter |

Each case declares its arguments, stdin, input bytes or shared fixture, and its expected `Run`.
The comments above each case give its original ID and its pin source citation at `074a2219`,
any policy record, and any platform exception.
Shared setup uses `#[fixture]` and `assert_fs`. Commands use
`assert_cmd::cargo::cargo_bin_cmd!`, with the existing 300-second timeout.
The group function sets up its files, runs its utility, then calls `check`.
There is no invocation interpreter, custom test runner or fixture-update mode.
`tests/common/mod.rs` contains only the complete file inventory, the DBM row reader
and the narrow masks, with their internal traversal and digest operations.
The backend w3 copy and the save-refusal setup in `tests/fixtures/mod.rs` are shared rstest fixtures.
Groups with nested working directories put that copy under `data/`.
The unigram group keeps its explicit subset of the bdb chunk inputs.

## Expectations

`tests/common/expected.rs` defines `Run` and `check`:

- `status` is `Exit(code)` or `Signal(number)`.
- `stdout` is `Exact(bytes)`, or `GlibOption(text)` for GLib help.
  `GlibOption` holds the pin's `[OPTION?]` spelling. It accepts exactly that text
  with `[OPTION…]` or `[OPTION...]` in its place, which are the spellings of other GLib versions.
- `stderr` is the exact bytes.
- `files` lists each path whose state after the run is not its pre-run bytes:
  `Sha256(digest, length)` of the bytes, or `Rows(digest)` of the sorted rows of a DBM file.

`check` compares the status and both streams byte for byte.
It then expands `snapshot_inputs()`, taken before the run, into the expected tree:
every pre-run file unchanged, every pre-run directory present, and the paths in `files`.
That tree must equal the complete inventory after the run, including paths and file types.
A read-only case therefore has `files: &[]`, and any changed, missing or extra path fails.

Cell, OS and architecture differences are explicit arms inside the value:
`match CELL`, `cfg!(target_os = "macos")` and `cfg!(target_arch = "aarch64")`.
`CELL` comes from the default store extension.
No entry is selected at run time, and no platform falls back to the value of another platform.

Short streams are byte literals. Non-ASCII text is a string literal with `.as_bytes()`.
Values over 200 bytes are files under `tests/fixtures/<utility>/`, named by content.
The tests include them with `include_bytes!` or, for GLib help, `include_str!`.
Byte-identical values share one file.
A value that more than one expectation uses is a named constant in the narrowest scope
that holds its uses: the module of the utility, the `frozen` module of the suite, or `common/expected.rs`.
A shared value that depends on the cell is a function, because a constant cannot match a `&str`.

The row digest keeps keys, values and empty values.
The tests keep the original cell-specific choice between logical DBM rows and container bytes.

## Masks

Volatile text uses the existing masks, not snapshot redactions.
Two masks act on binary data that a text redaction cannot express:
the pinyin-index padding in DBM row values, and the tkrzw clock bytes in container files.
The masks are:

- Print-time clock fields, keeping the operation count.
- GLib diagnostic process ID and time.
- Odd-length `PinyinIndexItem2` tail padding in the named DBM files.
- Eight tkrzw HashDBM clock bytes in the named files after the registered abort.
- Only the private `OXPINYIN_TEST_TMP` prefix of the test, back to `/tmp` in diagnostics.

The fixed-name programs run under private scratch directories; the default stays `/tmp`.
Sealed inputs stay sealed memory files on Linux (also when tests run as root), and read-only files on macOS.
`frozen_observations.rs` tests the masks, the row reader and `check`.

The macOS notes stay beside their cases: libc error numbers, stdio flushing on abort, and page-size-dependent container sizes.
Architecture-dependent NaN spellings and the three accepted GLib help spellings stay unchanged.
Policy-accepted cases cite their records, which state the pin's result and its source derivation.

## Pin bumps and recapture

Recapture is a separate step outside the test binaries; [the oracle pin](goldens-and-pins.md#3-the-oracle-pin) gives its conditions.
Expected values are never regenerated from oxpinyin test output.
