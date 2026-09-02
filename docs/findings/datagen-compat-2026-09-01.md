# P5 datagen — libpinyin-schema producers for the drop-in backends

Date: 2026-09-01 (rewritten 2026-09-02) · Status: **implemented, tested
against the pin-built data, wired into the normal `compile` command;
consumed by the production runtime since P6 (2026-09-02)**

> The file name is historical. There is **no compatibility layer** in
> this design and this document describes none: the earlier `compat.rs`
> (an oxpinyin-entries → libpinyin-rows converter) was removed as an
> architectural defect, not renamed.

## 1. Architecture

```text
model20 text (canonical source)
        │
        ▼
system::read_semantic / addon::read_libraries / punct::read_rows
        │        in-memory semantic records: phrases, parsed keys
        │        (tones kept), unigram counts, bigram groups
        │
        ├──► KC producer      ─┐  libpinyin's own schema, libpinyin's own
        ├──► Tkrzw producer   ─┘  file names: the drop-in set
        │
        ├──► redb producer    ─┐  the same records in the backend's own
        └──► LMDB producer    ─┘  container, `<stem>.<ext>` (since P6)
```

The read pass produces one semantic model. One set of serializers
consumes it — `system::compile`, `addon::compile`, `punct::compile`, the
byte-level output of upstream's build-time chain `gen_binary_files` →
`import_interpolation` → `gen_unigram` (`data/Makefile.am:43-49`) — and
every backend writes the rows it produces. (Until P6 the redb and LMDB
producers wrote a separate native schema for the old eager runtime; that
schema and its serializers are gone.)

Nothing translates one persistent representation into another. The
semantic model is in memory and belongs to neither implementation; each
writer implements its schema directly.

## 2. What the drop-in producers emit, and who defines each format

| File | Defined by | Writer | Comparison against the pin |
|---|---|---|---|
| `gb_char.bin`, `gbk_char.bin`, `opengram.bin`, `merged.bin`, `art.bin` … `technology.bin` | `SubPhraseIndex::store` (`phrase_index.cpp`), `MemoryChunk` | `chunks::build_chunk` | **byte-exact** (all 16) |
| `pinyin_index.bin`, `addon_pinyin_index.bin` | `ChewingLargeTable2` + `PinyinIndexItem2<L>` (`pinyin_phrase3.h`) | `libpinyin::pinyin_index_entries` | field-exact rows; struct tail padding excluded (§6) |
| `phrase_index.bin`, `addon_phrase_index.bin` | `PhraseLargeTable3` | `libpinyin::phrase_index_entries` | row-exact |
| `bigram.db` | `Bigram` / `SingleGram` (`ngram.cpp`) — a KC HashDB / Tkrzw HashDBM | `system::compile_libpinyin` | row-exact by point read |
| `punct.bin` | `PunctTable` (`punct_table.cpp`) | `punct::rows_to_entries_ucs4` | row-exact |
| `table.conf` | `SystemTableInfo2` | the CLI, `database format:` token per backend | text |

The container bytes of a DBM are the writing library's own layout and
are not compared; the runtime readers (P2–P4) and libpinyin itself read
records, not pages.

## 3. Tone information

Upstream's `ChewingLargeTable2::load_text` parses each `.table` row with
`PinyinDirectParser2::parse(USE_TONE, …)`: a trailing `1`–`5` on a
syllable is its tone (`parse_one_key`, `pinyin_parser2.cpp`), and the
parsed `ChewingKey` carries it. Three places store it:

* the chunk files — every pronunciation's packed key run
  (`PhraseItem::add_pronunciation`);
* the pinyin index **values** — `PinyinIndexItem2<L>::m_keys` are the
  parsed keys, tones included (`ChewingTableEntry::add_index`);
* nowhere in the pinyin index **keys** — `compute_chewing_index` zeroes
  the tone of every syllable, so all tone variants of one spelling share
  one key and are told apart by the value records; the incomplete key
  space (`compute_incomplete_chewing_index`) keeps only initials.

`system::parse_pinyin_keys` performs the same parse (`from_pinyin` on the
spelling without its digit, then `with_tone`), `ParsedRow.keys` carries
the toned keys into `libpinyin::encode_item`, and `PhraseModel.prons`
carries them into `chunks::build_chunk`. Nothing between the parse and
the bytes discards a tone.

The pinned model20 tables carry no tone digits, so the model20 parity
run cannot distinguish "tones preserved" from "tones always zero". The
toned mini model under `fixtures/datagen-toned/` exists for exactly that:
`tools/datagen/libpinyin-drop-in-differential.sh` compiles it with
libpinyin's own tools on one side and `oxpinyin-datagen` on the other,
and the parity test compares every chunk byte and every index record —
four readings of `ba` under one key (`ba`, `ba1`, `ba3`, `ba4`), two
pronunciations of one item differing only by tone (`ni3'hao3` /
`ni'hao`), toned addon rows. Both backends pass.

## 4. The pinyin index keyspace

Both key spaces live in one DBM. Every parsed row is inserted twice
(`ChewingLargeTable2::add_index`): once under its initial-only key, once
under its tone-zeroed key. `add_index_internal` then walks the shorter
prefixes of the just-inserted key from length `L-1` down and inserts an
empty-value marker until it meets a key that already exists, at which
point it stops.

That early stop does not make the final marker set order-dependent: by
induction every key in the DBM already has all of its proper prefixes
present when it is inserted (either it created them, or it stopped at
one whose own insertion had created the rest). The final keyspace is
therefore the prefix closure of the stored keys, which is what
`pinyin_index_entries` computes directly; the 201,658-row model20
comparison confirms it. The two spaces never share a physical key —
`compute_chewing_index` keeps middle and final, and the parser rejects a
syllable with neither — so no cross-space collision exists to resolve.

Record order inside a value is `pinyin_exact_compare2` (all initials,
then middle/final per syllable, then tones), token ascending among
equal keys; a duplicate `(keys, token)` is `ERROR_INSERT_ITEM_EXISTS`
and ignored.

## 5. The KC empty-value bug this work exposed

The first full KC compile failed its own write-time verification:
201,658 rows in, 138,753 rows back, the first missing row an empty
marker. Kyoto Cabinet's `Visitor::REMOVE` sentinel is the pointer value
`1` (`kcdb.h:46`), and an empty `Vec<u8>` reports `as_ptr() == 1` — so
every zero-length value handed to `kcdbset` deleted its key and
reported success. The fix (`kyotocabinet::ffi::c_ptr`) lives in the P2
layer with a conformance test on all four backends
(`empty_value_is_a_record`); see the P2 PR. Tkrzw, redb and LMDB never
had the hazard.

## 6. The one divergence: uninitialized struct padding

`PinyinIndexItem2<L>` for odd `L` carries two bytes of tail padding to
the `u32` token's alignment. Upstream inserts the whole `sizeof` of a
stack struct into the value, padding included; the bytes are whatever
sat on the stack (observed `17, 236` and `90, 237` in the pin's files).
This compiler writes zeros; the readers on both sides never touch the
padding. Register entry 18, class (b) — memory safety
(`docs/findings/upstream-divergences.md`, `compatibility-policy.md`).

## 7. What is, and is not, done

| Claim | State |
|---|---|
| Normal `oxpinyin-datagen compile` writes the drop-in set for KC and Tkrzw | done — `--backend kyotocabinet` (default) and `--backend tkrzw` |
| Output field-exact / byte-exact against the pin-built data dir | done — model20 on KC and Tkrzw; toned mini model on both |
| Tones preserved | done — proven by the toned differential |
| redb / LMDB | the same records in the backend's own container under `<stem>.<ext>` (P6); the earlier native schema is gone |
| Production runtime reads this output | done (P6, 2026-09-02) — `Runtime::open` opens this directory, or a libpinyin install's own, through the P1–P4 readers (`docs/findings/runtime-direct-libpinyin-data-2026-09-02.md`) |
| `tools/bisection/Dockerfile.perf-matrix` | its oxpinyin cells run `compile` into `/opt/oxpinyin-*/data`; `run-perf-same-data.sh` measures oxpinyin on the libpinyin cells' own directories instead |

## 8. Files

* `crates/oxpinyin-datagen/src/system.rs` — read pass, both serializers
* `crates/oxpinyin-datagen/src/libpinyin.rs` — the two index DBM row builders
* `crates/oxpinyin-datagen/src/chunks.rs` — `MemoryChunk` + `SubPhraseIndex` emitter
* `crates/oxpinyin-datagen/src/addon.rs`, `punct.rs` — addon and punctuation halves
* `crates/oxpinyin-datagen/src/write.rs` — raw-keyspace and hash-container writers with read-back verification
* `crates/oxpinyin-datagen/tests/libpinyin_parity.rs` — the differential
* `tools/datagen/libpinyin-drop-in-differential.sh`, `fixtures/datagen-toned/` — the toned run
