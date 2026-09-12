# BerkeleyDB backend — drop-in task 10

Date: 2026-09-12 · Status: **implemented and verified on Debian testing
and Fedora 44, both libdb 5.3.28**; the peer is non-default (tkrzw stays
the workspace default) and Linux-only in practice.

Phase 1's survey is `berkeleydb-compat-phase1.md` and its checklist is
`berkeleydb-compat-open-items.md`; the shelved first implementation on
`feat/bdb-backend` (2026-08-28) supplied the FFI layer's shape and the
byte-layout evidence. Everything below was re-derived against the pin at
`074a2219` (the storage files are byte-identical at the default-branch
tip `55e9051`, which the host checkout holds).

## What was built

The fifth store peer, behind the `bdb` cargo feature, exactly one per
binary like the other four:

- `crates/oxpinyin-store/src/bdb/{mod.rs,ffi.rs,wrapper.h}` —
  `BdbStore` over the full current trait surface: the framed
  `DB_BTREE` container (`ReadStore`/`WriteStore`, the session scratch,
  datagen output, the benches), the `DB_HASH` raw seam
  (`create_hash`/`open_hash_read_only` = `bigram.db`/`user_bigram.db`),
  and the bare-keyspace ops (`put_raw`/`get_raw`/`range_raw`) that
  `stage_dbm`'s index trees and the user-bigram writer go through. The
  trait defaults do the rest: the default `write_user_bigram`
  (create_hash + rows + compact + rename) is exactly the BDB shape, and
  the default `open_user_bigram` (hash open) is correct because on this
  backend the system and user bigrams are the same container class —
  neither Kyoto Cabinet override applies.
- `build.rs`'s `bdb` module — bindgen over the system `db.h` (allowlist:
  `db_create`, `db_strerror`, `db_version`, `DB`/`DBT`/`DBC`/`DBTYPE`,
  `DB_*`, `DB_VERSION_*`), header probe over pkg-config and the fixed
  directories (`/usr/include/db.h`, Fedora's `/usr/include/libdb/db.h`,
  historical `/usr/include/db5/db.h`), `OXPINYIN_BDB_{INCLUDE,LIB}_DIR`
  overrides, link `db`. No vendored copy, same arrangement as LMDB.
- The `bdb` feature forwarder in every consumer crate (mirroring each
  `kyotocabinet` line), the datagen `Backend::BerkeleyDb` variant
  (`--backend bdb`, token `BerkeleyDB`, `is_libpinyin_dbm` = true,
  extension `db`), `DEFAULT_STORE_DB_FORMAT = "BerkeleyDB"` — a token
  `user_files.rs`'s upstream-format set already carried — and
  `DEFAULT_STORE_EXT = "db"` for the scratch and datagen-native names.
- Tests: the shared `store_read_tests!`/`store_write_tests!` arms, the
  `user_store_tests!(bdb, …)` arm, `tests/bdb_libpinyin_files.rs`
  (below), `tools/bdb/{hash-walk.c,btree-order.c,run-sanitizers.sh}`
  (from the shelved branch), the `backend_matrix_bdb` bench, and a
  `bdb-sanitizers` CI lane that gates the merge like the tkrzw one.

## The two decisions worth recording

**`Send` + `Sync` via `DB_THREAD` + `DB_DBT_USERMEM`.** The shelved
implementation was `!Send` by choice — libpinyin opens without
`DB_THREAD`, which permits borrowed zero-copy reads. Today's tree
cannot ship that: `oxpinyin-user`'s registry holds `DefaultStore` in a
`static` (`Send`), and `oxpinyin-data`'s readers share one store
(`Sync`). Both claims are sound under libdb's own documented contract
for multi-threaded handles — `DB_THREAD` at open and every
libdb-written `DBT` over caller-owned memory — which is exactly the
configuration `ffi.rs` builds: every read lands in caller buffers with
a `DB_BUFFER_SMALL` grow-and-retry, cursors never escape one call, and
nothing frees library-owned memory. The cost is one copy per record
read, which the `ReadStore` trait (`Vec<u8>` returns) charges on every
backend anyway.

**libdb 5.3 only.** The runtime `db_version` gate refuses any other
major.minor at open. Both target distros pin 5.3.28 (Debian `libdb-dev`
→ `libdb5.3-dev` `5.3.28+dfsg2-11`, Fedora `libdb-devel`
`5.3.28-67.fc44`) — the last Sleepycat-licensed line, which is why the
distros froze there; Oracle relicensed 6.0+ to AGPL. The Homebrew
formula's 18.1 is unsurveyed and refused rather than guessed at: this
backend writes user profiles that the user's own libpinyin must read
back.

## Verification (Docker, Debian testing + Fedora 44, arm64)

The macOS host has no libdb; everything ran in containers
(`debian:testing`-based image with `libdb-dev libclang-dev`, and stock
`fedora:latest`), rustc 1.97.1 per `rust-toolchain.toml`:

- **Workspace sweep**:
  `cargo test --locked --workspace --no-default-features --features bdb`
  with CI's seven excludes — 103 test suites green, zero failures;
  `clippy --all-targets -- -D warnings` and `fmt --check` clean.
- **Exactly-one-backend gate**: `tools/store/backend-matrix.sh` —
  18/18 (six valid selections, ten pairs + a four-way + zero refused).
- **Sanitizers**: `tools/bdb/run-sanitizers.sh` — Rust suite under
  ASan/LSan clean; the C harnesses under ASan+UBSan clean; the harness
  half also walks a `bigram.db` the backend itself wrote
  (`--example bdb_write_profile`), checking every `SingleGram`
  invariant with no Rust in the reader. libdb's own internals are not
  instrumented (a rebuilt 5.3 would be a separate job).
- **Fixtures**: `oxpinyin-datagen compile --mini --backend bdb` →
  `fixtures/w3/db/` — 182/736/65/272 rows over the six DBMs, chunk
  files byte-identical to the other sets, `table.conf` differing only
  in `database format:BerkeleyDB`.
- **Real-file tests** (`--include-ignored`, against a pin-built
  `--with-dbm=BerkeleyDB` oracle's installed data): the 56,359-record
  system `bigram.db` walks whole, ordered and duplicate-free with all
  keys 4 bytes and all values `4 + 8n`; the index B-trees walk in
  strict byte order; a written user bigram reads back byte-for-byte
  through both seams and refuses a tree open.

## The drop-in differential, and a harness regression found on the way

The oracle is the pin at `074a2219` built `--with-dbm=BerkeleyDB`
(`tools/oracle/build-oracle.sh --dbm bdb`, model from the local cache);
a tkrzw twin was built for comparison. With
`OX_CARGO_FEATURES="--no-default-features --features bdb"`:

- **Phase D (oxpinyin trains, the pin reads)**: green — the pin opens
  the profile oxpinyin wrote through the BDB backend and renders the
  expected phrases.
- **Phases A–C (the pin trains, oxpinyin loads and saves in place, the
  pin re-renders)**: **blocked by a pre-existing harness regression,
  not by this backend.** The committed `user_driver.c` chooses
  candidate 0 — an n-best candidate whose `diff_result(best, best)`
  installs no `CONSTRAINT_ONESTEP` — so the pin's `train_result3`
  stores no user-bigram grams on *any* backend (traced against
  instrumented oracles: zero `Bigram::store` calls;
  `ForwardPhoneticConstraints::diff_result` skips equal tokens). The
  `a_pin_profile…` test's `!bigram.is_empty()` assertion therefore
  fails against a freshly built tkrzw oracle exactly as it does
  against the BDB one — the differential has been broken for every
  backend since `939400c6` (2026-09-11) created it, and CI never runs
  it (local-only, `--include-ignored`).
  With grams restored through an uncommitted probe driver (the same
  driver with the candidate index changed to 1 — the shape of a real
  non-default selection), **the full A–C round trip passes on BDB and
  on tkrzw identically**: oxpinyin's load→save of the pin's trained
  profile preserves it byte-for-byte (7 dump rows, 5 gram rows, the
  pin's own re-render `IDENTICAL` both ways).
- Fixing `user_driver.c` (and re-validating the differential's claim)
  is left to its own change: the driver defines what the differential
  measures, and that is a maintainer decision, not a backend PR's
  errand.

## Not verified

- **The frozen candidate and sentence pins were not re-measured** —
  this change adds a backend behind an off-by-default feature and
  touches no decode path, so it cannot move them; that is an argument,
  not a measurement.
- **libdb 18.x** is refused at open, so nothing was measured against
  it.
- **A stock distro `libpinyin-data` install** was not used; the
  oracle-built data directory stands in (same generator, same pin).
