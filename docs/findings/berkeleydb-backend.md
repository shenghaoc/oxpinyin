# BerkeleyDB backend

Status: **reference**. The backend is implemented on libdb 5.3.28 (Debian
and Fedora; macOS through Homebrew `berkeley-db@5`). Berkeley DB is a
non-default backend with full drop-in compatibility.
This record states the backend's structure, its decisions and its key
ordering. tkrzw is the default backend and the reference
cell ([compatibility policy, "Backends and user data"](compatibility-policy.md#backends-and-user-data)).
A bare `./configure` of the pin selects Berkeley DB; oxpinyin selects it with
`--no-default-features --features bdb`. The build and the overrides are in
[the backends runbook](../runbooks/backends.md). Pin: `074a2219`.

## What the backend is

The `bdb` cargo feature selects the backend. A binary has exactly one
backend, as with the other peers.

- `crates/oxpinyin-store/src/bdb/{mod.rs,ffi.rs,wrapper.h}` hold
  `BdbStore`. Every file is one raw keyspace, opened with no subdatabase
  name. `RawReadStore` reads it, and `put_raw`/`remove_raw` write it.
  `create_hash`/`open_hash_read_only` are the `DB_HASH` form of
  `bigram.db`/`user_bigram.db`. The index trees (`pinyin_index.bin`,
  `phrase_index.bin`, `punct.bin`), the session scratch store and the
  user-bigram writer use the `put_raw`/`get_raw`/`range_raw` seam.
  On this backend the system and user bigrams are the same container
  class. The trait's default `open_user_bigram` therefore applies, and
  neither Kyoto Cabinet override applies.
- The `bdb` module of `crates/oxpinyin-store/build.rs` runs bindgen over
  the system `db.h`. The allowlist is `db_create`, `db_strerror`,
  `db_version`, `DB`/`DBT`/`DBC`/`DBTYPE` and `DB_*`. The pin finds libdb
  without pkg-config, so the build defaults `SYSTEM_DEPS_BDB_NO_PKG_CONFIG`
  and `SYSTEM_DEPS_BDB_LIB=db`. No copy of libdb is vendored.
- Every consumer crate forwards the `bdb` feature.
- Tests: the shared store read and write suites,
  `crates/oxpinyin-store/tests/bdb_libpinyin_files.rs` (files the pin's
  libdb wrote, from `fixtures/w3/db/`; their `table.conf` differs from the
  other backends' sets only in `database format:BerkeleyDB`), and the C harnesses
  `tools/bdb/hash-walk.c` and `tools/bdb/btree-order.c`.
  `just sanitize bdb` runs the Rust suite under ASan, then the C harnesses
  under ASan and UBSan. The harness walks a `user_bigram.db` that the
  backend wrote (`--example bdb_write_profile`). It checks every
  `SingleGram` invariant with no Rust in the reader. The `bdb-sanitizers`
  job in `.github/workflows/store-backends.yml` runs the same gate.
  libdb's own internals are not instrumented.

## Decisions

**No `Send`, no `Sync`, no lock.** libpinyin opens its handles without
`DB_THREAD`, and `ffi.rs` keeps the pin's open flags. The C contract
serializes the calls that reach a handle: one call at a time on a context
and all its handles. The handle's owner is neither `Send` nor `Sync`.
Cursor reads go into caller-owned (`DB_DBT_USERMEM`) buffers, so a visitor
can read a row and call the store again. A point read copies the value out
of the handle's buffer before the call returns. The module documentation
in `bdb/mod.rs` and `bdb/ffi.rs` states the full contract.

**libdb 5.3 only.** The runtime `db_version` check refuses any other
major.minor at open (`check_runtime_version` in `bdb/ffi.rs`). Debian
(`libdb-dev` → `libdb5.3-dev`) and Fedora (`libdb-devel`) ship 5.3.28, the
last Sleepycat-licensed line. Oracle relicensed 6.0 and later under the
AGPL. The default Homebrew `berkeley-db` formula (18.1) is not surveyed, and
the open refuses it. The reason: this backend writes user profiles that the
user's own libpinyin must read back.
`OXPINYIN_BDB_ALLOW_UNSURVEYED_VERSION` turns that one refusal into a
warning. No workflow or recipe sets it.

## Key ordering

libpinyin opens its B-trees (`phrase_large_table3_bdb.cpp`,
`chewing_large_table2_bdb.cpp`) without `set_bt_compare`. libdb's default
comparator is byte-wise `memcmp`, shorter key first on a shared prefix.
That is the [store key-ordering contract](store-key-ordering.md), so the
backend sets no comparator. Setting one would reorder files libpinyin
wrote. Little-endian `u32` array keys therefore walk in raw-byte order, not
integer order, and a 1-element key precedes the 2-element key that extends it.
`tools/bdb/btree-order.c` and the shared key-ordering suite hold this; the
suite asserts that this backend walks the same keys as tkrzw and Kyoto Cabinet.
