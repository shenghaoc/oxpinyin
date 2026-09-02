# W3 mini fixtures

Four data directories, one per storage backend, plus the content `.bin`
files at this level:

- **`kct/`, `tkt/`, `redb/`, `lmdb/`** — the `--mini` compile of the
  pinned model20 (`oxpinyin-datagen compile --mini --backend <peer>
  --out-dir fixtures/w3/<ext>`), one directory per compiled-in backend,
  each a complete system data directory the runtime opens as is:
  `pinyin_index`, `phrase_index`, `bigram`, `punct`, the `addon_*` pair
  (libpinyin's own file names on `kct/` and `tkt/` — Kyoto Cabinet and
  tkrzw are the DBMs libpinyin builds against; `<stem>.<ext>` on `redb/`
  and `lmdb/`), the sixteen per-library chunk files (`gb_char.bin` …
  `technology.bin`, byte-identical across the four), `table.conf`, and the
  producer's `datagen-manifest.txt`. The subset is `system::MINI_KEYS`
  and `addon::MINI_ART_KEYS`: the phrases those spellings reference, with
  their real `\1-gram` counts and bigram rows. The runtime and C-ABI test
  suites open `fixtures/w3/<DEFAULT_STORE_EXT>`; `oxpinyin-datagen`'s
  `fixtures_identity` test reproduces the compiled backend's directory
  from the model cache (records and chunk bytes; DBM container bytes
  depend on the writing library's version).
- **Content `.bin` files** (art … technology, at this level) — truncated
  copies of the oracle's custom-content tables for the custom-content
  loader tests, regenerated with `python3 tools/generate_w3_fixtures.py`.

`pin-ref.txt` names the oracle pin the fixtures are deterministic
against. `fixtures.sha256` lists the checksums of every fixture file;
regenerate it with:

```bash
cd fixtures/w3 && find . -type f ! -name fixtures.sha256 | LC_ALL=C sort | sed 's|^\./||' | xargs shasum -a 256 | sed 's| \./| fixtures/w3/|; s|  | fixtures/w3/|' > fixtures.sha256
```
