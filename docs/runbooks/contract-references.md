# Contract references

This page describes the frozen expectations of the C-boundary tests and the local cross-open check.

The C-boundary tests are ordinary Rust tests in the `src` test modules of
`crates/oxpinyin-capi` and `crates/oxpinyin-zhuyin-capi`:

- `boundary_tests.rs` runs on the w3 mini fixture;
- `pinned_data_tests.rs` runs on the pinned package.

Their expected values are the frozen observations of the pin, inline in each test. The tests never run the pin.
`just fetch-contract-data` supplies the pinned system data for the tkrzw tests.
`cargo test -p oxpinyin-capi --lib` and `cargo test -p oxpinyin-zhuyin-capi --lib` run them.
In each crate, `testdata/pin-profiles.txt` holds the two pin-written profiles that the
handoff tests of that crate read.

An expectation changes at a pin bump under [the oracle pin](goldens-and-pins.md#3-the-oracle-pin).

## Reverse cross-open (local only)

The handoff tests cover a pin-written file that oxpinyin reads. The other direction
needs the pin, so it is a local command:

```sh
uv run --no-project --managed-python python -X utf8 \
  tools/bisection/reverse_cross_open.py --cell tkrzw --pin-prefix PREFIX \
  --data "$(just fetch-contract-data)" \
  target/debug/libpinyin_capi.so target/debug/libzhuyin_capi.so
```

The command runs four handoff cases: `issue1114-collision-load-save` and
`save-929-logical-cross-open`, each for pinyin and for zhuyin. Each case does these steps:

1. oxpinyin writes a profile in a fresh worker.
2. The case copies that profile.
3. The pin reads the copy and saves it again.

The case asserts the stored rows. For `issue1114`, it also asserts the saved
bytes outside the timestamp.

The issue1114 handoff cases also read tkrzw HashDBM bytes, so they run only on
the tkrzw cell. The save-929 cases run on every supported cell.

The handoff check runs after a change to a persistence writer, to serialisation or
to a database format. No workflow and no just recipe calls the command.
