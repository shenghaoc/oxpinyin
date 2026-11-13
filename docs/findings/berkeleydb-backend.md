# BerkeleyDB backend

Status: **implemented** on libdb 5.3.28 (Debian and Fedora; macOS through
Homebrew `berkeley-db@5`). Berkeley DB is a non-default backend with full
drop-in compatibility.
This record states the backend's structure, its decisions and the evidence
for its key ordering and real files. tkrzw is the default backend and the reference
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
  libdb wrote, from `fixtures/w3/db/`), and the C harnesses
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
wrote.

`tools/bdb/btree-order.c` confirms this. It opens a `DB_BTREE` as
libpinyin does (no environment, no transaction, no comparator). It loads
little-endian `u32` array keys that cross 256 in the first element and in
a later element. The walk is in raw-byte order, not integer order:

- The 1-element key `00000001` precedes the 2-element key
  `0000000102010000` that extends it.
- The decoded values run `0x01000000, 0x00010000, 0x00ff0000,
  0x00000100, …`.

The shared key-ordering suite asserts that this backend
walks the same keys as tkrzw and Kyoto Cabinet.

## Real-file evidence

Against a pin-built `--with-dbm=BerkeleyDB` data directory:

- The 56,359-record system `bigram.db` walks whole, ordered and free of
  duplicates. Every key is 4 bytes and every value is `4 + 8n` bytes.
- The index B-trees walk in strict byte order.
- A user bigram that the backend wrote reads back byte for byte through
  both seams and refuses a tree open.
- The mini fixtures in `fixtures/w3/db/` hold the same chunk files as the
  other backends' sets. Their `table.conf` differs only in
  `database format:BerkeleyDB`.

## Pin facts for a profile round trip

A driver that trains a profile on the pin must respect these facts:

- Candidate 0 is an `NBEST_MATCH_CANDIDATE` only once `m_nbest_results` is
  non-empty, and only `pinyin_guess_sentence` (`pinyin.cpp:1372`) fills it
  through `_prepend_sentence_candidates` (`:1934`). A `LONGER_CANDIDATE`
  trains unigram only and installs no constraint (`pinyin.cpp:2523`). An
  n-best selection at index 0 installs no `CONSTRAINT_ONESTEP`, so
  `train_result3` stores no user-bigram gram.
- `pinyin_parse_more_full_pinyins` (`pinyin.cpp:1497-1525`) clears neither
  the matrix state nor `m_nbest_results` nor `m_constraints`. Only
  `pinyin_reset` (`pinyin.cpp:2693`) does.
- A driver follows ibus's order (`PYPPhoneticEditor.cc:341,355,494-511`):
  parse, guess the sentence, list candidates, select `NORMAL_CANDIDATE`s
  at the lookup cursor, train, remember and reset. ibus does not train an
  n-best selection at index 0
  (`PYPLibPinyinCandidates.cc:116`). ibus sorts with
  `SORT_BY_PHRASE_LENGTH | SORT_BY_PINYIN_LENGTH | SORT_BY_FREQUENCY`
  (`PYPConfig.cc:151`).
- With sort option `0`, the first `NORMAL_CANDIDATE` is an arbitrary rare
  character (`疒` for `nihao`). Its grams crash the pin's bigram-export
  iterator on Berkeley DB. This is the class (b) defect of
  compatibility-policy row 1. The `phrases` export of the same profile
  walks, and tkrzw and Kyoto Cabinet export the same rows. With ibus's sort
  option the rows export on all three backends.

With such a driver, the round trip gives the same result on all three
backends. When the pin trains and oxpinyin loads and saves in place, the
pin's re-render is `IDENTICAL`. When oxpinyin trains, the pin reads the
profile (`READABLE`). [user-store.md](user-store.md) §11 holds the user-store
record.

## Not verified

- **libdb other than 5.3.** The open refuses it, so nothing is measured
  against 4.8 or 18.x.
- **A stock distro `libpinyin-data` install.** A pin-built data directory
  stands in for it (same generator, same pin).
