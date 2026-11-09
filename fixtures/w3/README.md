# W3 mini fixtures

Three data directories, one per storage backend, plus the content `.bin`
files at this level:

- **`db/`, `kct/`, `tkt/`** — a mini system data directory per backend
  (Berkeley DB, Kyoto Cabinet and tkrzw are the DBMs libpinyin builds
  against; `db/` is the default build's flavour since 2026-09-20), each
  a complete directory the runtime opens as is: `pinyin_index.bin`,
  `phrase_index.bin`, `bigram.db`, `punct.bin`, the `addon_*` pair
  (libpinyin's own file names on all three), the five per-library chunk
  files the subset reaches (`gb_char.bin`, `gbk_char.bin`,
  `opengram.bin`, `merged.bin`, `art.bin`, byte-identical across all
  three) and `table.conf`, the pin's `data/table.conf.in` rendered for
  the backend. The runtime and C-ABI test suites open
  `fixtures/w3/<DEFAULT_STORE_EXT>`.

  The retired generator wrote
  the three directories (`tools/dev/w3_fixtures.py`, standard library
  only). The subset step selects the phrases that the `MINI_KEYS`
  spellings (`a`, `ni`, `hao`, `ni'hao`, `ni'men`, `zhong`, `guo`,
  `zhong'guo`, `xian`, `xi'an`) name in the four system tables and that
  the `MINI_ART_KEYS` spellings (`er'huang`, `bo'cai`, `ban'she`) name
  in `art.table`, keeps every row of those phrases, their real
  `\1-gram` counts and the `\2-gram` rows whose both tokens survive, and
  copies `punct.table` whole, into a work directory. The build step runs
  the pin's own sequence (`data/Makefile.am:60-62` at 074a2219) from
  `oxpinyin-utils`, built for the directory's backend:
  `gen_binary_files --gen-punct-table`, `import_interpolation` (which
  sums each surviving bigram row into its total) and `gen_unigram`,
  then `gen_deleted_ngram` for `deleted_bigram.db` (below). The
  utilities read `table.conf` from the directory and open every addon
  row's table and chunk; the subset has rows in art alone, so the build
  runs with a `table.conf` whose addon list is art, and the committed
  rendering with the pin's full list is written afterwards.

  The retired checker regenerated each
  directory into a temporary location and compared it with the
  committed tree: the chunk files and `table.conf` byte for byte, and
  the DBM files record by record, read through
  [archived raw records source](https://github.com/shenghaoc/oxpinyin/blob/71092202e489262fd3ccbdf116fa19fccab8f928/crates/oxpinyin-store/examples/raw_records.rs), because the container
  bytes are the writing library's own layout and change with its
  version. The model20 archive is not redistributable and CI does not
  have it. Both retired commands used the verified model extraction and refused an
  extraction whose cache lacks the fetch's verified marker.
- **`*/deleted_bigram.db`** — a training input, not part of the compiled
  set: the pin's `gen_deleted_ngram` (built for the directory's own
  backend) run over the fixed corpus
  `16777219 阿` / `16777220 啊`, giving the `sentence_start → 阿` and
  `阿 → 啊` rows `estimate_interpolation` and `gen_deleted_ngram`'s
  ordinary cases read. The recipe above writes and checks it after the
  three-program sequence. Container bytes follow the backend's DBM, as
  the other containers do.
- **Content `.bin` files** (art … technology, at this level) — truncated
  copies of the oracle's custom-content tables for the custom-content
  loader tests, captured from the oracle data directory by the retired content generator.

### KMM tool fixtures (2026-10-31 UTC)

`kmm/` holds the three text models the k-mixture-model tool cases
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
  record (the pin's default bucket count preallocates 6 MiB per file).
  The sentence-start row of `k_mixture_model.db` keeps the text input
  order `16817937, 16802309`, unlike the pin importer's sorted insertion.
  This is malformed stored input, retained unchanged; see
  [the unsorted-record finding](../../docs/findings/eio-flexible-bigram-unsorted-records.md).

The layout is the pin's: the eight-byte magic key with `KMMP` and
the 12-byte magic header, then one four-byte-token-keyed single gram
(`KMMP`-less: the 8-byte array header followed by 20-byte token/item
records in token order, except for the malformed KC row above). They are
pinned inputs: the retired W3 generator did not write or check them. `fixtures.sha256` covers all of them.

`pin-ref.txt` names the oracle pin the fixtures are deterministic
against. `fixtures.sha256` lists the checksums of every fixture file;
To update the checksum list after a deliberate fixture change:

```bash
cd fixtures/w3 && find . -type f ! -name fixtures.sha256 | LC_ALL=C sort | sed 's|^\./||' | xargs sha256sum | sed 's|  |  fixtures/w3/|' > fixtures.sha256
```
