# Contract references

The C-boundary tests are ordinary Rust tests in the `src` test modules of
`crates/oxpinyin-capi` and `crates/oxpinyin-zhuyin-capi`: `boundary_tests.rs` on the
w3 mini fixture and `pinned_data_tests.rs` on the pinned package. Their expected values
are the pin's frozen observations, inline in each test, and they never run the pin.
`just fetch-contract-data` supplies the pinned system data for the tkrzw tests.
`testdata/pin-profiles.txt` in each crate holds the two pin-written profiles that its
handoff tests read.

At a pin bump, a changed contract expectation is derived from the pin's source
diff. Use a `pin_capture.py` observation only where the source does not settle
the result. `pin_capture.py` is the only capture tool; its procedure is
[pin-captures.md](pin-captures.md).

## Reverse cross-open (local only)

The handoff tests cover the pin's file read by oxpinyin. The other direction
needs the pin, so it is a local command:

```sh
uv run --no-project --managed-python python -X lazy_imports=normal \
  tools/bisection/reverse_cross_open.py --cell tkrzw --pin-prefix PREFIX \
  --data target/contract-data/VERSION/libpinyin/data \
  target/debug/libpinyin_capi.so target/debug/libzhuyin_capi.so
```

It runs four handoff cases: `issue1114-collision-load-save` and
`save-929-logical-cross-open`, for pinyin and zhuyin. Each case lets oxpinyin
write a profile in a fresh worker, copies that profile, and lets the pin read and
re-save it. The case asserts the stored rows and, for `issue1114`, the saved
bytes outside the timestamp.

The S2 witness prints one oxpinyin observation and has no frozen expectation. It
needs the system data of the cell it names. The pinned package data is a tkrzw
directory: `table.conf` with a HashDBM `bigram.db`:

```sh
data=$(just fetch-contract-data | tail -n 1)
uv run --no-project --managed-python python -X lazy_imports=normal \
  tools/bisection/reverse_cross_open.py --cell tkrzw --data "$data" --s2 \
  target/debug/libpinyin_capi.so
```

The issue1114 handoff cases also read tkrzw HashDBM bytes, so they run only on
the tkrzw cell. The save-929 cases run on every supported cell.

Run the handoff check after a change to a persistence writer, to serialisation or
to a database format. No workflow and no just recipe calls either command.
