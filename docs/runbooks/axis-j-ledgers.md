# Axis J ledgers — records pinned to their measurement tree

The three axis-J ledgers are records, not live gates:
[`axis-j-bdb-ledger.md`](../findings/axis-j-bdb-ledger.md),
[`axis-j-kc-ledger.md`](../findings/axis-j-kc-ledger.md) and
[`axis-j-tkrzw-ledger.md`](../findings/axis-j-tkrzw-ledger.md). Each is
pinned to the crates tree it measured, named in the ledger's own
"Pin … subject crates tree …" line:

| ledger | subject crates tree |
| --- | --- |
| bdb | `ecead320030bade4e7f72dacbe32ed5bc1edde3d` |
| kc | `8804b6629941e85ccc3d98b14d279c261fc84d02` |
| tkrzw | `8804b6629941e85ccc3d98b14d279c261fc84d02` |

What that means at a later `main`:

- **bdb.** The full `--check` fails. `render_ledger.render` raises
  "manual audit Rust tree changed" when `HEAD:crates` is not
  `finish_ledger.MANUAL_CRATES_TREE` (the bdb tree above), so it refuses
  before it measures or compares anything.
  `tools/bisection/internal-ledger.py --inventory-check` is the only ledger-check mode
  the tooling does not refuse at a later tree, but it is not verified to
  pass there: it still reads the repository (the anchor table at the top of
  `internal-ledger.py`, which text-searches `crates/` files; the capi
  `lib*.ver` export lists; the Rust items `coverage_tiers.cited_items`
  reads; and the recorded oracle prefix, via `gcov_measure.verify_prefix`),
  so a rename or an export-list change can make it fail. It was last
  observed passing in #789, using locally rebuilt pin-prefix digests because
  the recorded prefix was not reproducible on that host.
- **kc and tkrzw.** No check is expected to pass at a later `main`.
  `internal-ledger.py` refuses `--inventory-check` for any cell other than
  bdb, and `delta_ledger.render_delta` has no tree guard: it writes the
  current `HEAD:crates` and the SHA-256 of the built capi shared objects
  into the text, so the byte-for-byte compare of the full `--check` differs
  when the later tree changes generated ledger content, such as the recorded
  `crates` tree hash or a measured shared-object hash. A change outside
  `crates/` can trigger this when it alters the built facades, such as the
  toolchain or `Cargo.lock`.
- A failing full `--check` at a later `main` is expected, not a regression.
- Line numbers cited in a ledger belong to the tree it names (`AGENTS.md`:
  dated records keep the line numbers of the tree they name).
- Regenerate a ledger only when its manual sample is re-reviewed, never to
  turn `--check` green.

The ledgers, `render_ledger.py` and `delta_ledger.py` generate and own
their own headers; this page is the hand-written place for the rule. The
regeneration commands are in each ledger's header.
