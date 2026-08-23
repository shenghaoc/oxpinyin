# W3 mini fixtures

Two families, both deterministic against the pinned oracle
(`fixtures/w3/pin-ref.txt`):

- **Content `.bin` files** (art … technology) — truncated copies of the
  oracle's custom-content tables, regenerated with
  `python3 tools/generate_w3_fixtures.py`.
- **redb tables** — `pinyin_index.redb`, `phrase_index.redb`,
  `bigram.redb`, `punct.redb`, and the `addon_*.redb` files are **frozen**
  and no longer regenerated in-tree (the `oxpinyin-migrate` crate that
  produced them has been removed). Their provenance is recorded below for
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
