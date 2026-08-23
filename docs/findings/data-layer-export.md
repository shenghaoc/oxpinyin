# Data-layer export SPEC — public-ABI derivation of the system tables

Date: 2026-08-10 · Status: **frozen for the data-layer rebuild** ·
Authorised by the maintainer's rebuild plan of 2026-08-10.

oxpinyin reads the system dictionary and bigram from portable redb tables.
This finding freezes where those tables come from, their exact schemas, and
how they are verified. It replaces the withdrawn syllable-encoder approach
(branch `feat/integration-syllable-encoder`), whose `SyllableKey → TableKey`
mapping was refuted against the pinned oracle; the refutation evidence is
summarised at the end so the replacement is legible on its own.

## Approach

Every value in the shipped tables is obtained from the pinned oracle
(`docs/findings/oracle-environment.md`) **through its public C ABI** — the
same header subset frozen in `docs/findings/oracle-ffi-seam.md`, extended by
the export/token functions listed below. No upstream translation unit is
read, no internal file format of libpinyin is interpreted, with the single
documented exception of the bigram byte format, which is frozen here from
observed data and verified by a total mechanical invariant plus
oracle-resolved spot checks.

The exporter was `oxpinyin-migrate export` (feature `oracle-ffi`, Linux-first;
the crate has since been removed — the tables are committed under
`fixtures/w3/`, frozen). It drove the additional public functions:

- `pinyin_begin_get_phrases` / `pinyin_iterator_has_next_phrase` /
  `pinyin_iterator_get_next_phrase` / `pinyin_end_get_phrases` — enumerate
  one phrase library as `(phrase, pinyin, count)` tuples.
- `pinyin_lookup_tokens` — the oracle's own `phrase text → phrase_token_t`
  mapping.
- `pinyin_token_get_phrase` — the reverse mapping, used by verification.

The four **default-loaded system libraries** are exported, identified by the
top byte of their tokens: 1 = `gb_char`, 2 = `gbk_char`, 3 = `opengram`,
4 = `merged`. Addon dictionaries (art … technology) are optional, are not
loaded under the parity capture protocol, and are out of scope. The bigram
export iterator (`pinyin_begin_get_bigram_phrases`) yields no system data in
the pinned build — measured directly, in every configuration including
trained-and-saved user state — so the system bigram is carried by the
verbatim Tkrzw conversion described below instead.

Observed export sizes for the pin (informative): `gb_char` 95,698 tuples,
`gbk_char` 21,234, `opengram` 28,255, `merged` 1,051.

## Table schemas

All tables are redb databases with the single `data` table mapping raw
`&[u8]` keys to raw `&[u8]` values (`oxpinyin-data::LookupTable`). All
multi-byte integers are little-endian. Entries are written in ascending key
order and every list below has a frozen order, so a given oracle pin
exports to byte-identical tables.

### `pinyin_index.redb` — pinyin string → phrase records

- **Key**: the pinyin spelling exactly as the oracle's export iterator
  prints it: syllables joined by `'` (U+0027), e.g. `ni`, `ni'hao`,
  `xi'an`. UTF-8 bytes, no terminator. The apostrophe is load-bearing:
  bare concatenation would merge distinct sequences (`xi'an` vs `xian`).
- **Value**: a sequence of 8-byte records `{token: u32, freq: u32}`,
  sorted by `freq` descending, then `token` ascending. `freq` is the
  export tuple's count. No header, no padding; value length is a multiple
  of 8.
- One record per `(pinyin, token)` pair. If the oracle reports the same
  pair twice the counts are summed (not observed in the pin; the exporter
  logs it if it ever happens).

A dictionary lookup for decoder keys `[k1, …, kn]` is
`LookupTable::get(join(text(k1) … text(kn), "'"))` followed by token → text
resolution through `phrase_index.redb`. Multi-syllable lookup is therefore a
single string-keyed get; there is no per-syllable binary encoder and no
compound binary key.

### `phrase_index.redb` — token → phrase text

- **Key**: `phrase_token_t` as 4 bytes little-endian.
- **Value**: the phrase's UTF-8 text.
- Tokens come from `pinyin_lookup_tokens` on the exported phrase text,
  filtered to the token whose top byte equals the library being exported.
  An export tuple whose phrase resolves to no token in its own library is
  dropped and counted; the exporter reports the count (0 for the pin).

### `bigram.redb` — previous token → successor records

Originally produced by `oxpinyin-migrate convert` as a **verbatim**
record-for-record copy of the pin's `bigram.db` (a Tkrzw HashDBM; the bridge
copied raw key and value bytes untouched). The committed
`fixtures/w3/bigram.redb` applies that same verbatim per-record copy
restricted to the mini allowlist (`fixtures/w3/README.md`); the byte format,
frozen from observed data, describes both:

- **Key**: previous `phrase_token_t` as 4 bytes little-endian.
- **Value**: `total: u32` followed by 8-byte records
  `{next_token: u32, count: u32}`.
- **Invariant**: `total == Σ count` over the value's records. Holds for
  all 56,359 entries of the pin's table; `oxpinyin-data`'s fixture tests
  enforce it on every committed entry, so a conversion or interpretation
  error cannot land silently.

This is the one schema not obtained through the public ABI. Its
verification is: (a) the total-equals-sum invariant over the whole table,
(b) every `next_token`'s top byte is a loaded system library, and
(c) resolved spot checks — for at least three common previous tokens the
top successors resolved via `pinyin_token_get_phrase` are asserted (e.g.
你 → 的 is the top successor by count in the pin).

## Verification

With the exporter removed, the generated tables survive only frozen under
`fixtures/w3/`, checksummed by `fixtures/w3/fixtures.sha256`; the live-oracle
round-trip checks retired with the exporter. Ongoing validation reduces to
the frozen tables' consumers:

- **Bigram invariant**: every committed entry of `bigram.redb` parses under
  the schema above with `total == Σ count`
  (`crates/oxpinyin-data/src/lm/tests.rs::invariant_holds_for_every_fixture_entry`),
  plus the same module's 你 → 的 ordering check
  (`observed_transition_is_cheaper_than_novel`). Successor top-byte and
  top-successor orderings are not re-run by any committed test; they were
  checked once at freeze time (§ above).
- The dictionary tables have no remaining round-trip check against the
  live oracle; their contents are frozen under `fixtures/w3/`.

## Why the previous approach was withdrawn

Measured against the pin on 2026-08-10 (evidence in the session record and
reproducible with the tools noted below):

- The real `pinyin_index.bin` holds 928 entries, all 6-byte keys — 922
  dense `00 00 00 00 [u16 BE 1..922]` and six `c0`-prefixed. There are no
  concatenated multi-syllable keys of any length.
- `ChewingKey::get_table_index()` enumerates pinyin strings roughly
  alphabetically (1..440, incomplete keys interleaved) and matches the
  frozen `idx+10` table for 3 of 405 complete syllables; neither matches
  the DBM key space. Phrase tokens observably live in two buckets each
  (你 in 449 and 874) under an underived bucket function, and the value
  layout is a sectioned multi-record format, not a token array.
- The old `phrase_index` re-keying was built from that misparse and lacks
  你 entirely; the old bigram model assumed 4-byte `(prev u16, next u16)`
  keys and `(freq, next)` record order — real keys are full u32 tokens and
  the record order is `{next, count}` — so its lookups could never match.

Deriving the real bucket function would require reading upstream C++.
The public-ABI export above sidesteps the
undocumented format entirely: the shipped tables are **ours**, their
contents are the oracle's, and the bridge between decoder keys and table
keys is the plain pinyin spelling that both sides already share.
