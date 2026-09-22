# redb removal audit — 2026-09-20 UTC

Branch `refactor/drop-redb-backend`, stacked on `ci/portable-lanes-on-a-c-backend`
(PR #500), `fix/capi-database-format-bdb` (PR #498) and `refactor/drop-redb-lmdb-backends`
(PR #497). One commit.

## Why

redb had no upstream counterpart: libpinyin has only ever built against
Berkeley DB, Kyoto Cabinet and tkrzw, so no redb behaviour was
oracle-verifiable, and a redb store file was oxpinyin's own artifact from
a configuration that was never released — there is no arriving user to
carry over (maintainer ruling 2026-09-20; the empty-affected-set instance
of the backend-family data-loss ruling). No migration, import, export or
reader for redb data exists, was proposed, or is kept. Its last CI role —
the pure-Rust peer the portable lanes ran because their runners were
unprovisioned — ended in the member below (#500 moved test-macos to
Homebrew tkrzw and test-windows to vcpkg Berkeley DB 4.8.30; the
python-portable job that sentence used to name was retired on main
in `5e7bfd98`), so the removal has **no platform consequence**.

## What was removed

- The inline `RedbStore` implementation, its read/write helpers, write
  transaction and error mapping in `crates/oxpinyin-store/src/lib.rs`;
  the `DefaultStore`/`DEFAULT_STORE_EXT`/`DEFAULT_STORE_DB_FORMAT` redb
  arms; the redb pairs of the exactly-one-backend `compile_error!`
  guards; the per-peer test groups and the write-side emptiness-probe
  test (compatibility-policy row 31's subject).
- `DEFAULT_STORE_IS_LIBPINYIN_DBM` is **kept**, now unconditionally
  `true`, with a dated comment — removing a pub item is an API change,
  and the DBM-vs-native distinction it documents still keys the drop-in
  surface (§5 ruling).
- The `redb` workspace dependency, the store's optional dep, the
  `oxpinyin-user` direct dev-dependency (fed only by the codec
  order-cross-check, deleted with the backend), and the feature forward
  in every member manifest. The workspace lock loses exactly `redb`
  (172 → 171 packages); `fuzz/Cargo.lock` was already clean.
- `Backend::Redb` in `oxpinyin-datagen` (variant, parse arm, method
  arms); its `DEFAULT` const needs no change — it was already cfg-gated
  per backend, and the redb arm went with the feature.
- The two redb benches (`redb_is_empty`, `backend_matrix_redb`), the
  fixtures tier `fixtures/w3/redb/` (six sparse `.redb` files: 6.3 MB
  apparent, 504 KB on disk) with its 13 `fixtures.sha256` lines and the
  README count; `user_store.redb` from `.gitignore`; the store-backends
  matrix child, case arm and gate display name; `backend-matrix.sh`'s
  valid/invalid combos; the dead case arms in the packaging and
  bisection scripts.
- Local drivers were **ported, not deleted**: `run-differentials.sh`
  and `nightly-fixture-differentials.sh` (tkrzw export; the w3 tier now
  consumes the committed `w3/tkt` fixtures), `run-w8-cycle.sh` and
  `run-train-diff-dynamic-off.sh` (`*.tkt` export names),
  `probe-debuginfo-neutrality.sh` (default backend tkrzw; its comment
  was stale independently — the default has been tkrzw since
  2026-09-05). The `backend_bench` example lost its dead two-column
  comparison mode — the exactly-one-backend guard had refused both
  features in one build since 2026-08-2x, so the mode was already
  unrunnable — and now benches the compiled-in backend.

## Disclosures

- `tools/bisection/system-dir.sh`'s extension enumerators gained the
  `db` entries their own case arm and usage line already promised — the
  pre-existing BDB gap flagged in Phase 1, completed under the standing
  peer-count-class gate. The equivalent enumerators in
  `run-bisect`/`run-cpp-smoke`/`check-alloc-pairing` scan fixture
  directories and stay without `db` on this member; that gap is an
  **open item**, not an impossibility — `fixtures/w3/db` does exist (it
  has since this stack's base commit), so the omission was an unreached
  case. The member above closes it: its enumerators accept `db` and
  prefer it, because the default build opens that flavour.
- **Open item, pre-existing, outside this stack's scope:** the two
  `.kct`-gated surface differentials — `tools/bisection/run-key-surface-diff.sh`
  and `tools/bisection/run-phrase-surface-diff.sh` — accept only
  `.kct`-named system tables (`has_all_kct_tables` checks
  `$SYSTEM/{pinyin_index,phrase_index,bigram}.kct`), and no producer has
  written `<stem>.kct` system tables since P6 (`DEFAULT_STORE_EXT`'s
  `kct` names session scratch and datagen-native containers; datagen's
  libpinyin-format writer emits libpinyin's own names). Both gates can
  therefore only ever SKIP. Recorded per the review ruling; they get
  their own PR after this stack merges.
- `.kiro/steering/rust-conventions.md`'s Dependencies ruling keeps
  its force. The parenthetical that named redb as the worked example
  of "a Rust library is used as Rust" now says that example was removed
  2026-09-22 and the tree has no pure-Rust database in its place.
- `.kiro/steering/compatibility-policy.md:42` needed no amendment: it
  states the per-KV-backend-family ruling without naming any removed
  backend's builds.

## Verification

Recorded in the PR body, verbatim: tkrzw default build; KC and BDB
container builds; the compile_error! guards firing on zero and on two
backends; `cargo metadata --locked` and `cargo deny` on both lockfiles;
fmt / clippy -D warnings / the full workspace test; the store-backends
matrix green on the remaining peers with the case count recounted; the
drop-in differential byte-identical to the base on tkrzw and KC with a
base control; the completeness grep with its residual table; and the
§9 cargo-cinstall artifact measurement (expected bit-identical: redb was
never in the default graph).
