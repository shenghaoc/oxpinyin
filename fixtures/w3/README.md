# W3 mini fixtures

Three families, all deterministic against the pinned oracle
(`fixtures/w3/pin-ref.txt`):

- **Content `.bin` files** (art … technology) — truncated copies of the
  oracle's custom-content tables, regenerated with
  `python3 tools/generate_w3_fixtures.py`.
- **Kyoto Cabinet tables (`.kct`)** — the default backend's committed set:
  `pinyin_index.kct`, `phrase_index.kct`, `bigram.kct`, `punct.kct`,
  `addon_4_pinyin_index.kct`, `addon_4_phrase_index.kct`. Produced by the
  same recipe as the redb set —
  `oxpinyin-datagen compile --mini --backend kyotocabinet` — and
  row-identical to it through the store API (the writer verifies every row
  on read-back). The tkrzw (`.tkt`) and LMDB (`.lmdb`) sets are committed
  too — same command, `--backend tkrzw|lmdb` — so the four-backend test
  gate is self-contained everywhere, including CI, which must never fetch
  model20. All bytes pinned by `fixtures.sha256`.
- **redb tables** (the pure-Rust portability backend,
  `--no-default-features`) — `pinyin_index.redb`, `phrase_index.redb`,
  `bigram.redb`, `punct.redb`, and the `addon_*.redb` files are **frozen**
  (committed bytes pinned by `fixtures.sha256`). They were originally
  produced by the removed `oxpinyin-migrate` exporters; the same mini
  subset is now reproducible from the canonical model20 archive alone via
  `oxpinyin-datagen compile --mini` (row-identical through the store API;
  container bytes depend on the writing redb version — see
  `docs/findings/datagen-model20.md`). Provenance is recorded below for
  reference.

  `pinyin_index.redb` / `phrase_index.redb`: the `--mini` subset of the
  public-ABI export (`docs/findings/data-layer-export.md`). `--mini` keeps
  the allowlisted pinyin keys and the phrase tokens those keys reference —
  every kept record byte-identical to the full export.

  `bigram.redb`: verbatim Tkrzw-conversion records of the pin's `bigram.db`
  per `docs/findings/data-layer-export.md` — not part of the public-ABI
  export, whose bigram iterator yields no system data — restricted like the
  other tables to the mini allowlist (entries whose previous token is one
  of those phrases).

  `punct.redb`: the Option A public-ABI export of `punct.table` (token →
  NUL-terminated UTF-8 puncts).

  The aggregate `addon_pinyin_index.redb` / `addon_phrase_index.redb` are
  verbatim conversions of the raw Tkrzw files. Those two `addon_*.redb`
  files are **superseded** for runtime use: they are the undocumented
  sectioned format, kept only because this manifest still pins them.

  `addon_4_pinyin_index.redb` / `addon_4_phrase_index.redb` are the W11
  Option A public-ABI export of a mini `art.table` subset.

`fixtures.sha256` lists the checksums of every fixture file.
