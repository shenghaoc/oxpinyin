# Pin captures — the apostrophe inputs

The six inputs `'`, `''`, `'''`, `ni'`, `ni'!` and `ni'i` are tested natively in the crates that
own the behaviour.

`crates/oxpinyin-capi/src/parse.rs` and `crates/oxpinyin-zhuyin-capi/src/parse.rs` hold the
parse and parsed length of `'`, `''` and `'''`:
- the values are Rust literals;
- each case cites its record in `fixtures/contract-refs/tkrzw/contract_zero_key_guess.jsonl`
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

   Each run, `tools/oracle/pin_capture.py`, first records `capture.json`:
   - the pin commit in the prefix's build record;
   - the SHA-256 of the `libpinyin.so` it loads;
   - the identity of the system data.

   It refuses the run unless these are the repository's pins:
   - `libpinyin_commit` in `tools/oracle/oracle-pin.txt`;
   - this platform's `libpinyin_sha256_tkrzw` in `tools/oracle/oracle-pin.txt`;
   - `content_sha256` in `tools/oracle/libpinyin-data.json`.

   Only then does it make the C API calls of each observation in `OBSERVATIONS`. Each runs on
   the image's tkrzw cell, in a fresh process with a fresh user directory. An observation that
   ends in the pin's abort keeps the results of the calls before it.

   The run writes `target/pin-observations/<run>/capture.json` and `observations.json`, and
   nothing else. A run directory that already exists is refused.

2. The two runs' `observations.json` must be equal:

   ```bash
   diff target/pin-observations/1/observations.json target/pin-observations/2/observations.json
   ```

3. These are proposed observations. Show them, next to the pin source lines, as a STOP before
   any test uses them ([goldens and pins](goldens-and-pins.md)). The approved values then go
   into the Rust tests as literals, each citing its run and source line. No test reads the
   staging directory.

`tools/oracle/pin_capture.test.py` holds the two refusals: a capture of another pin, another
library or other data, and a capture that would write outside its staging directory.

A new observation goes into `OBSERVATIONS` in `pin_capture.py`.
