# W3 mini fixtures

Two families, both deterministic against the pinned oracle
(`fixtures/w3/pin-ref.txt`):

- **Content `.bin` files** (art … technology) — truncated copies of the
  oracle's custom-content tables, regenerated with
  `python3 tools/generate_w3_fixtures.py`.
- **redb tables** — `pinyin_index.redb`, `phrase_index.redb` and
  `bigram.redb` are the `--mini` subset of the public-ABI export
  (`docs/findings/data-layer-export.md`), regenerated with

  ```sh
  cargo run -p oxpinyin-migrate --features oracle-ffi -- export --out-dir fixtures/w3 --mini
  ```

  `--mini` keeps the allowlisted pinyin keys (`MINI_KEYS` in
  `crates/oxpinyin-migrate/src/export.rs`), the phrase tokens those keys
  reference, and the bigram entries whose previous token is one of those
  phrases — every kept record byte-identical to the full export.
  `punct.redb` and the aggregate `addon_pinyin_index.redb` /
  `addon_phrase_index.redb` are verbatim `oxpinyin-migrate convert` output
  of the raw Tkrzw files. Those two `addon_*.redb` files are **superseded**
  for runtime use: they are the undocumented sectioned format, kept only
  because this manifest still pins them.
  `addon_4_pinyin_index.redb` / `addon_4_phrase_index.redb` are the W11
  Option A public-ABI export of a mini `art.table` subset:

  ```sh
  cargo run -p oxpinyin-migrate -- export-addon --table-dir <model-data> --out-dir fixtures/w3 --mini
  ```

`fixtures.sha256` lists the checksums of every fixture file.
