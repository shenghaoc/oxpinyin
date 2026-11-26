# Contract references

This page describes the frozen expectations of the C-boundary tests and the local cross-open check.

The frozen expectations live beside the modules that implement them, in
the root crate. The C crates retain
conversion, dispatch and publication checks in their sibling `tests.rs` files
and `tests/it` targets.

The expected values are inline in each test. The tests never run the pin.
`just fetch-contract-data` supplies the pinned system data for the tkrzw tests.
`just test --locked` runs the workspace tests through nextest. The read-fault
and locale tests require its separate process for each test.
The two C crates' `testdata/pin-profiles.txt` files hold the pin-written profiles
that `oxpinyin/src/user/persistence/tests/pinned_profiles.rs` reads.

An expectation changes at a pin bump under [the oracle pin](goldens-and-pins.md#3-the-oracle-pin).

## Reverse cross-open (local only)

The handoff tests cover a pin-written file that oxpinyin reads. The other direction
needs the pin, so it is a local command. `PREFIX` is the oracle prefix of the
cell that `just provision-oracle` built. Build the two subject libraries with
the same backend as the cell.

For the tkrzw cell, pass the pinned package data:

```sh
just reverse-cross-open --cell tkrzw --pin-prefix PREFIX \
  --data "$(just fetch-contract-data)" \
  target/debug/liboxpinyin_capi.so target/debug/liboxzhuyin_capi.so
```

For the bdb and kc cells, pass the verified data of the cell's prefix:

```sh
just reverse-cross-open --cell bdb --pin-prefix PREFIX \
  --data PREFIX/lib/libpinyin/data \
  target/debug/liboxpinyin_capi.so target/debug/liboxzhuyin_capi.so
```

Every cell needs `--data`. Without it, the command stops at argument parsing.

On the tkrzw cell, the command runs four handoff cases: `issue1114-collision-load-save` and
`save-929-logical-cross-open`, each for pinyin and for zhuyin. On the bdb and kc
cells, it runs the two `save-929-logical-cross-open` cases. Each case does these steps:

1. oxpinyin writes a profile in a fresh worker.
2. The case copies that profile.
3. The pin reads the copy and saves it again.

The case asserts the stored rows. For `issue1114`, it also asserts the saved
bytes outside the timestamp.

The issue1114 handoff cases also read tkrzw HashDBM bytes, so they run only on
the tkrzw cell. The save-929 cases run on every supported cell.

The handoff check runs after a change to a persistence writer, to serialisation or
to a database format. The `just reverse-cross-open` recipe is local-only. No workflow runs the pin.
The command stops at the first failed case.

The Berkeley DB fixture writer uses `db5.3_load` from `PATH`. The shared
reader uses `db5.3_dump` from `PATH`. Both names must be available on a
Berkeley DB host. The writer loads dump-format byte values into a fresh,
unnamed hash database. It clears `DB_HOME` and runs in an empty temporary
directory. It does not rewrite the profile handed from oxpinyin to the pin.
