# W3 mini fixtures

This directory holds three data directories, one for each storage backend, and the content `.bin`
files at this level:

- **`db/`, `kct/`, `tkt/`** — a mini system data directory for each backend.
  Berkeley DB, Kyoto Cabinet and tkrzw are the DBMs that libpinyin builds
  against; `tkt/` (tkrzw) is the flavour of the default build. Each directory is
  complete, and the runtime opens it as it is. It contains:

  - `pinyin_index.bin`, `phrase_index.bin`, `bigram.db`, `punct.bin` and the `addon_*` pair
    (the file names of libpinyin on all three);
  - the five per-library chunk
    files that the subset reaches (`gb_char.bin`, `gbk_char.bin`,
    `opengram.bin`, `merged.bin`, `art.bin`), byte-identical across all
    three;
  - `table.conf`: the `data/table.conf.in` of the pin, rendered for
    the backend.

  The runtime and C-ABI test suites open
  `fixtures/w3/<DEFAULT_STORE_EXT>`.

  The three directories were written in two steps (recorded evidence).
  The generation and check commands are retired (#1764);
  `tools/dev/w3_fixtures.py` keeps the subset and sequence code for
  `tools/oracle/pin_capture.py`:

  1. The subset step writes into a work directory. It selects the phrases that the `MINI_KEYS`
     spellings (`a`, `ni`, `hao`, `ni'hao`, `ni'men`, `zhong`, `guo`,
     `zhong'guo`, `xian`, `xi'an`) name in the four system tables, and the phrases that
     the `MINI_ART_KEYS` spellings (`er'huang`, `bo'cai`, `ban'she`) name
     in `art.table`. It keeps every row of those phrases, their real
     `\1-gram` counts, and the `\2-gram` rows whose two tokens both survive. It
     copies `punct.table` whole.
  2. The build step runs
     the pin's own sequence (`data/Makefile.am:60-62` at 074a2219) from
     `oxpinyin-utils`, built for the backend of the directory:
     `gen_binary_files --gen-punct-table`, `import_interpolation` (which
     sums each surviving bigram row into its total) and `gen_unigram`,
     then `gen_deleted_ngram` for `deleted_bigram.db` (below).

  The utilities read `table.conf` from the directory and open the table and chunk of every addon
  row. The subset has rows in art only, so the build
  runs with a `table.conf` whose addon list is art. The committed
  rendering has the full list of the pin.

  The retired check regenerated each directory into a temporary location
  and compared it with the committed tree (recorded evidence):

  - the chunk files and `table.conf` byte for byte;
  - the DBM files record by record. The container
    bytes are the own layout of the writing library and change with its
    version.

  The model20 archive is not redistributable, and CI does not
  have it.
- **`*/deleted_bigram.db`** — a training input, not part of the compiled
  set. It is the output of the pin's `gen_deleted_ngram` (built for the own
  backend of the directory) over the fixed corpus
  `16777219 阿` / `16777220 啊`. It gives the `sentence_start → 阿` and
  `阿 → 啊` rows that `estimate_interpolation` and the ordinary cases of `gen_deleted_ngram`
  read. The generation above wrote it after the
  three-program sequence. The container bytes follow the DBM of the backend, as
  for the other containers.

### KMM tool fixtures

`kmm/` holds the text model that the k-mixture-model tool cases
import:

- `k_mixture_model.text`: a small valid model over the `<start>`, `中国` and `你好` tokens of the fixture.

Each cell carries three imported
containers as `k_mixture_model{,_zero,_edom}.db`. The zero container has all
counts and frequencies zero, and the edom container has a prune probability of
`NaN`; neither has a committed text source. The `validate`, `merge`, `prune`, `estimate`, `import`, `export`
and `gen` cases use them as read-only inputs:

- `db/`: the Berkeley DB `import_k_mixture_model` output of the pin, unchanged.
- `tkt/`: the same, rebuilt with
  `tkrzw_dbm_util rebuild --dbm hash` (the minimum size of the committed HashDBM file;
  records and order are kept).
- `kct/`: a fresh Kyoto Cabinet HashDB, made with
  `kchashmgr create -apow 3 -bnum 64` and one `kchashmgr set -sx` per
  record. (The default bucket count of the pin preallocates 6 MiB per file.)
  The sentence-start row of `k_mixture_model.db` keeps the text input
  order `16817937, 16802309`, unlike the sorted insertion of the pin importer.
  This is malformed stored input, kept unchanged; see
  [the unsorted-record finding](../../docs/findings/eio-flexible-bigram-unsorted-records.md).

The layout is the layout of the pin:

- the eight-byte magic key with `KMMP` and
  the 12-byte magic header;
- then one single gram keyed by a four-byte token
  (`KMMP`-less: the 8-byte array header followed by 20-byte token/item
  records in token order, except for the malformed KC row above).

These containers are
pinned inputs.

`pin-ref.txt` names the oracle pin against which the fixtures are deterministic.
