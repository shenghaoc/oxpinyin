# Contract references

This page describes the frozen expectations of the C-boundary tests and the local cross-open check.

The frozen expectations live beside the modules that implement them, in
`crates/oxpinyin-core`, `oxpinyin-engine`, `oxpinyin-data`, `oxpinyin-input`,
`oxpinyin-user`, `oxpinyin-runtime` and `oxpinyin-facade`. The C crates retain
conversion, dispatch and publication checks in their sibling `tests.rs` files
and `tests/it` targets.

The expected values are inline in each test. The tests never run the pin.
`just fetch-contract-data` supplies the pinned system data for the tkrzw tests.
`just test --locked` runs the workspace tests through nextest. The read-fault
and locale tests require its separate process for each test.
The two C crates' `testdata/pin-profiles.txt` files hold the pin-written profiles
that `oxpinyin-user/src/persistence/tests/pinned_profiles.rs` reads.

An expectation changes at a pin bump under [the oracle pin](goldens-and-pins.md#3-the-oracle-pin).

## Reverse cross-open (local only)

The handoff tests cover a pin-written file that oxpinyin reads. The other direction
needs the pin, so it is a local command:

```sh
uv run --managed-python python -X utf8 \
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
