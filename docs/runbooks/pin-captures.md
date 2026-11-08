# Pin captures — the apostrophe inputs

The six inputs `'`, `''`, `'''`, `ni'`, `ni'!` and `ni'i` are tested natively in the crates that
own the behaviour.

`crates/oxpinyin-capi/src/parse.rs` and `crates/oxpinyin-zhuyin-capi/src/parse.rs` hold the
parse and parsed length of `'`, `''` and `'''`:
- the values are Rust literals;
- each case cites its frozen observation, held by the `zero_key_guess` contract tests,
  and the pin source line behind it;
- the tests need no pin, no container, no Python and no environment variable, and read no file;
- they read no database, so they run on every cell.

Two groups of tests wait for the captures below:
- the abort of a key query on an apostrophe-only parse;
- the key walk of `ni'`, `ni'!` and `ni'i`.

## Capture

The capture runs on the pin only. It is a STOP under the
[approval conditions](../../CONTRIBUTING.md#approval-conditions): do it only when the maintainer
asks for it.

1. On a native host, capture two runs in the capture image:

   ```bash
   bash tools/oracle/capture-pin.sh 1
   ```

   ```bash
   bash tools/oracle/capture-pin.sh 2
   ```

   Each run, `tools/oracle/pin_capture.py`, first refuses the capture unless:
   - `tools.oracle.image.prefix_ok` accepts the tkrzw prefix;
   - the SHA-256 of the `libpinyin.so` it loads equals the prefix's recorded
     `shared_object_sha256`;
   - the data's digest equals `content_sha256` in `tools/oracle/libpinyin-data.json`.

   Only then does it make the C API calls of each observation in `OBSERVATIONS`. Each runs on
   the image's tkrzw cell, in a fresh process with a fresh user directory. An observation that
   ends in the pin's abort keeps the results of the calls before it.

   The run writes `target/pin-observations/<run>/capture.json` and `observations.json`, and
   nothing else. A run directory that already exists is refused.

   `capture.json` is provenance, not frozen values: the source commit, the recipe hash, the
   loaded library's SHA-256, `versions_sha256`, the capture image and the data identity.

2. The two runs' `observations.json` must be equal:

   ```bash
   diff target/pin-observations/1/observations.json target/pin-observations/2/observations.json
   ```

3. These are proposed observations. Show them, next to the pin source lines, as a STOP before
   any test uses them ([goldens and pins](goldens-and-pins.md)). The approved values then go
   into the Rust tests as literals, each citing its run and source line. No test reads the
   staging directory.

`tools/oracle/pin_capture.test.py` holds the two refusals: a capture of a prefix `prefix_ok`
rejects, a loaded library that differs from the prefix's record or other data, and a capture
that would write outside its staging directory.

A new observation goes into `OBSERVATIONS` in `pin_capture.py`.
