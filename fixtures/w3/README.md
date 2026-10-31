# W3 mini fixtures

Three data directories, one per storage backend, plus the content `.bin`
files at this level:

- **`kct/`, `tkt/`, `db/`** — the `--mini` compile of the
  pinned model20 (`oxpinyin-datagen compile --mini --backend <peer>
  --out-dir fixtures/w3/<ext>`), one directory per compiled-in backend,
  each a complete system data directory the runtime opens as is:
  `pinyin_index`, `phrase_index`, `bigram`, `punct`, the `addon_*` pair
  (libpinyin's own file names on all three — Kyoto Cabinet, tkrzw and
  Berkeley DB are the DBMs libpinyin builds against; `db/` is the
  default build's flavour since 2026-09-20),
  the five per-library chunk files the `--mini` subset reaches
  (`gb_char.bin`, `gbk_char.bin`, `opengram.bin`, `merged.bin`, `art.bin`,
  byte-identical across all three), `table.conf`, and the
  producer's `datagen-manifest.txt`. The subset is `system::MINI_KEYS`
  and `addon::MINI_ART_KEYS`: the phrases those spellings reference, with
  their real `\1-gram` counts and bigram rows restricted to the
  subset's phrases on both the previous-token and successor side
  (group totals recomputed over the surviving records — no dangling
  successor references). The runtime and C-ABI test
  suites open `fixtures/w3/<DEFAULT_STORE_EXT>`; `oxpinyin-datagen`'s
  `fixtures_identity` test reproduces the compiled backend's directory
  from the model cache (records and chunk bytes; DBM container bytes
  depend on the writing library's version). Since the 1-gram-only
  emission change, `oxpinyin-datagen compile` also writes an
  `interpolation2.text` into the out-dir; the committed sets do not
  carry it (nothing at runtime reads it from a data dir — it is
  gitignored under `fixtures/w3/*/`).
- **`*/deleted_bigram.db`** — a training input, not part of the compiled
  set: the pin's `gen_deleted_ngram` (built for the directory's own
  backend) run over the fixed corpus
  `16777219 阿` / `16777220 啊`, giving the `sentence_start → 阿` and
  `阿 → 啊` rows `estimate_interpolation` and `gen_deleted_ngram`'s
  ordinary cases read. Container bytes follow the backend's DBM, as the
  other containers do.
- **Content `.bin` files** (art … technology, at this level) — truncated
  copies of the oracle's custom-content tables for the custom-content
  loader tests, regenerated with `python3 tools/generate_w3_fixtures.py`.

### KMM tool fixtures (2026-10-31 UTC)

`kmm/` holds the three text models the k-mixture-model `tool-diff` cases
import: `k_mixture_model.text` (a small valid model over the fixture's
`<start>`, `中国` and `你好` tokens), `k_mixture_model_zero.text` (all
counts and frequencies zero) and `k_mixture_model_edom.text` (a model
whose prune probability is `NaN`). Each cell carries the three imported
containers as `k_mixture_model{,_zero,_edom}.db`, used as read-only
inputs by the `validate`, `merge`, `prune`, `estimate`, `import`, `export`
and `gen` cases:

- `db/`: the pin's Berkeley DB `import_k_mixture_model` output, unchanged.
- `tkt/`: the same, rebuilt with
  `tkrzw_dbm_util rebuild --dbm hash` (the committed HashDBM file's
  minimum size; records and order are preserved).
- `kct/`: a fresh Kyoto Cabinet HashDB with
  `kchashmgr create -apow 3 -bnum 64` and one `kchashmgr set -sx` per
  record, writing the same host-native record bytes the pin's import
  writes (the pin's default bucket count preallocates 6 MiB per file).

The record bytes are the pin's: the eight-byte magic key with `KMMP` and
the 12-byte magic header, then one four-byte-token-keyed single gram
(`KMMP`-less: the 8-byte array header followed by 20-byte token/item
records in token order). `fixtures.sha256` covers all of them.

`pin-ref.txt` names the oracle pin the fixtures are deterministic
against. `fixtures.sha256` lists the checksums of every fixture file;
regenerate it with:

```bash
cd fixtures/w3 && find . -type f ! -name fixtures.sha256 | LC_ALL=C sort | sed 's|^\./||' | xargs sha256sum | sed 's|  |  fixtures/w3/|' > fixtures.sha256
```
