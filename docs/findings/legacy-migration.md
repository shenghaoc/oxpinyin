# Legacy user-data migration (W7-T2) — SHELVED

Date: 2026-08-16 · Status: **SHELVED — cancelled per libpinyin succession precedent** ·
Scope: the binary legacy-dir migration (`pinyin-dictool migrate --legacy-dir
<path> --user-dir <path>`) into the pinyin-rs user store.

This work is parked, not merged. The findings below are the real, verified
results worth keeping; the implementation (the `storage_dump` shim, the
`apply_migration` bulk-write, and the `migrate` subcommand) lives on the
parked branch `feat/w7-t2-legacy-migrate`.

## 1. Why shelved

The Architect cancelled W7-T2 because libpinyin itself set the succession
precedent: it shipped **no** migration from the pinyin engine or from
novel-pinyin (its own ancestor — `novel_types.h` is the residue), and
ibus-libpinyin shipped none from ibus-pinyin. The supported interchange has
always been the line-oriented text format the frontend's Import/Export
buttons drive (which W7-T1 already imports). A binary-level DBM migrator
recreates a story the original never had.

## 2. Approach chosen: (B)-refined — read via the storage classes

Two candidate approaches were weighed:

- **(A)** Parse the on-disk binary layouts in Rust (`user.bin` MemoryChunk +
  the DBM-backed `user_bigram.db`). Rejected: three DBM backends (BDB /
  Kyoto / tkrzw) would each need a reader, plus a MemoryChunk checksum and
  length reader — three reimplementations of layouts the project otherwise
  deliberately bypasses.
- **(B)** Drive the pinned libpinyin's public C ABI export iterators
  (`pinyin_begin_get_phrases` / `pinyin_begin_get_bigram_phrases`) and write
  the triples into redb. Rejected as insufficient: the export surface is
  lossy (see §3) — it cannot round-trip the store.

The chosen route is **(B)-refined**: link libpinyin's internal
`libstorage.a` through a C++ dump shim (`storage_dump.cpp`) and read
`user.bin` + `user_bigram.db` through the very classes that wrote them
(`FacadePhraseIndex` / `PhraseItem` over `MemoryChunk`; `Bigram` /
`SingleGram` over the DBM). No binary layout is reimplemented; the DBM
backend is whatever libpinyin's own code selects; the shim emits a flat,
deterministic, little-endian buffer the Rust side parses bounds-checked.

Why `libstorage.a` and not `libpinyin.so`: the `.so` is built with a version
script that exports only the public ABI (`pinyin_*`, `g_*`) and hides the
internal `pinyin::` storage classes. The storage symbols are only reachable
by linking the `libstorage` convenience archive, which `make install` skips
(everything is `noinst_*`). The shim therefore depends on the pinned
build-oracle install manifest for the internal headers + archive.

Dependency stated explicitly: migration requires the pinned libpinyin at
migration time (Linux-first, like oracle/capi/migrate). Acceptable for a
one-time tool; it is not on any runtime path.

## 3. The export surface does not cover migration

This is the finding that forces (B)-refined. The public iterators are a
*rendering* surface, not a value surface:

- **`sentence_start` predecessors are dropped** (`pinyin.cpp:804`): the
  bigram iterator skips `m_index_token == sentence_start`, so the
  sentence-start bigram rows (which the store keeps verbatim) are
  unreachable through `pinyin_begin_get_bigram_phrases`.
- **Counts are re-scaled ×2** (`pinyin.cpp:899`,
  `*count = iter->m_count * unigram_factor`, `unigram_factor = 2`): the
  stored `m_count` is doubled on export, so the export value is not the
  stored value.
- **Below-threshold counts are hidden**: `pinyin_bigram_iterator_has_next_phrase`
  skips any `item->m_count <= threshold` where `threshold = 23*3 - 1 = 68`,
  so low-count bigram rows never surface.
- **Cartesian pronunciation rendering**: the bigram iterator emits one row
  per (first-pronunciation × second-pronunciation) pair, so a predecessor
  pair with `a` and `b` pronunciations renders `a×b` rows, not one.
- **Per-predecessor totals are unreachable**: `SingleGram::get_total_freq()`
  is never exposed by the bigram iterator (only per-(prev,cur) counts, ×2).
  The store maintains `BIGRAM_TOTAL` as the exact sum of successor counts,
  so it must be *recomputed* from the merged successors — which
  `apply_migration` does — rather than read from the export.
- **Pronunciation `m_freq` is internal**: `get_nth_pronunciation(…, freq)`
  reads it into a local that the export discards; only the phrase iterator's
  `count` carries it, merged across pronunciations.

Net: migration that is value-exact (including totals and sentence-start
rows) needs the storage classes, hence the shim.

## 4. Read-only guarantee

`MemoryChunk::load` opens `O_RDONLY` and verifies the stored length and
checksum before exposing the mapping; `Bigram::load_db` opens the DBM with
`OPEN_NO_CREATE` and copies it into an in-memory DB. Nothing in the shim
opens a path for writing. The contract "never write the legacy dir" is
enforced by these storage-class semantics (confirmed from source, not from a
read-only mount), and is the reason the shim reads the two files directly
instead of driving `pinyin_init` (which would open the dir for the normal
read-write lifecycle).

## 5. Tone-strip and key-mapping decision

libpinyin stores pinyin with a trailing tone digit (`ni3`, `hao3`; zero-tone
is plain `ni`). pinyin-rs stores tone-stripped `SyllableKey` ids. The
migration strips exactly one trailing `1`..`5` (pinyin spellings never end
in an ASCII digit, so the strip is unambiguous) and maps each syllable
through the frozen pinyin-core inventory. A pronunciation with any
unmappable syllable is dropped and counted (`keys_unmapped`) while the phrase
is kept; the migration report prints the tone-strip and null-slot arithmetic
so it is auditable.

## 6. Caveats

- **Header fragility**: the internal headers are `noinst_*` and the
  generated `config.h` carries DBM-backend guards
  (`HAVE_TKRZW`/`HAVE_BERKELEY_DB`/`HAVE_KYOTO_CABINET`). The shim depends on
  an explicit, pinned install manifest; any libpinyin version bump re-rolls
  the header set and the `storage_manifest_sha256`.
- **tkrzw backend**: the pinned oracle uses Tkrzw. The storage-class route
  insulates the shim from backend variation, but the manifest headers expose
  all three backends, so the fragility is real even if the reading logic is
  not.
- **`ERROR_NO_ITEM` null slots**: iterating the user token range hits
  deallocated slots (tokens whose phrases were removed); these are skipped
  silently and counted, so a legacy store with holes reports "N tokens
  iterated, M null slots skipped".
- **`pub(crate)` bulk-write bumps `write_generation`**: `apply_migration`
  calls the same `mark_committed_write` the ABI write sites use, so an
  already-open decode session invalidates its read cache after a migration.
