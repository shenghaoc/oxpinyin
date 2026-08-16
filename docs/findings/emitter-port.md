# Emitter port — `export_interpolation` → `interpolation2.text` (W9-T4a)

This documents the Rust `oxpinyin-emitter` crate, a value-level reproduction of
libpinyin's textual interpolation export. It consumes W9-T2's integer
[`Counts`] and writes `interpolation2.text` in the grammar
`crates/oxpinyin-data/src/interp.rs` already reads.

The reproduction target is `utils/storage/export_interpolation.cpp`, read from
the pinned libpinyin `2.11.91` source (authorized: it is the trainer dump
tool, not the decoder). The consumer pin is `interp.rs::parse_interpolation2`
(own code). Decode internals were not consulted.

This is the emitter only. The Wikipedia corpus pipeline (dump acquisition,
markup stripping, trad→simp, sentence-split) is W9-T4b, a separate later
task. T4b has no differential oracle: libpinyin's training corpus is
undocumented (T0).

---

## Crate placement

New crate `crates/oxpinyin-emitter`, not a module in `oxpinyin-counter` or
`oxpinyin-lambda`.

- libpinyin keeps `export_interpolation` as its own `utils/storage` binary,
  downstream of `gen_ngram` and independent of `estimate_interpolation`.
- W9 already maps each trainer stage to a crate (`oxpinyin-segment`,
  `oxpinyin-counter`, `oxpinyin-lambda`). A sibling crate keeps that seam.
- `oxpinyin-counter::Counts::dump` is a *value-only* checksum format
  (`\data pinyin-counter`, no phrase-text columns). The interpolation
  grammar is a different contract and must not overwrite that dump.
- `unsafe`: deny. Portable. `publish = false`. Never ships with the engine.

---

## Format grammar (both ends)

### What `export_interpolation` writes
(`utils/storage/export_interpolation.cpp`)

| Line | Source | Notes |
|---|---|---|
| `\data model interpolation` | `:33-35` `begin_data` | `import_interpolation.cpp:90-93` requires `model == interpolation` |
| `\1-gram` | `:76` | opens the unigram section |
| `\item %d %s count %ld` | `:98` | `token`, phrase text, raw unigram count |
| `\2-gram` | `:107` | opens the bigram section |
| `\item %d %s %d %s count %d` | `:131-132` | `prev`, `w1`, `cur`, `w2`, raw bigram count (`item->m_count`) |
| `\end` | `:38-40` `end_data` | footer |

Filters (**SHOWN**):

- Unigram `freq == 0` is skipped (`:95-96`).
- Unigram / bigram records whose `taglib_token_to_string` returns `NULL`
  are skipped (`:97-98`, `:130-132`).
- `sentence_start` (token `1`) prints as `<start>`
  (`src/storage/tag_utility.cpp:368-370`, `:391-393`).

No λ line. No probability column. The stored value is the raw integer
count (`:93`, `:128`), matching T2's count/float boundary.

`k_mixture_model_to_interpolation.cpp:78,138,174` emits the same section
markers and `\item … count …` records. It is not this PR's reproduction
target (KMM is out of W9 scope).

### What `interp.rs` reads
(`crates/oxpinyin-data/src/interp.rs`, `parse_interpolation2`)

| Accepts | How | Source |
|---|---|---|
| `\1-gram` | section open; everything before it is ignored | `:34`, `:187-189` |
| `\item <id> <text…> count <count>` | `fields[1]` → `u32` token; last field → `u64` count; `fields[len-2] == "count"` | `:200-228` |
| phrase text | **ignored** (the token already identifies the phrase) | `:200-202` |
| `\2-gram` / `\end` / any other `\` line that is not `\item` | ends the `\1-gram` payload | `:191-195` |
| empty lines inside the section | skipped | `:196-198` |

Rejects: malformed `\item` lines, a zero count (`:223-228`), duplicate
tokens (`:237-242`), a file with no `\1-gram` (`:232-234`).

### Fields one side emits that the other ignores

| Field | `export_interpolation` | `parse_interpolation2` |
|---|---|---|
| `\data model interpolation` | writes | ignores (seeks `\1-gram`) |
| phrase-text columns | writes | ignores |
| `\2-gram` records | writes | skips the whole section |
| `\end` | writes | treated as a section terminator |
| λ | **does not write** | does not look for it |

The two sides agree on the load-bearing unigram payload: `(token: u32,
count: u64)` with `count > 0`. The emitter writes the *full* export
grammar (header, both n-gram sections, footer) so a pin-built
`import_interpolation` / `export_interpolation` differential can compare
the 2-gram half too.

---

## λ is not written (PR #55)

PR #55 established that the shipped interpolation weight lives in
`table.conf` (`lambda parameter:0.312699`) and is read at decode time via
`get_lambda()`. The model20 archive ships `interpolation2.text` (counts) +
`.table` files, no `table.conf`.

`export_interpolation.cpp` was re-read for this PR: `begin_data` /
`gen_unigram` / `gen_bigram` / `end_data` emit n-gram records only. A
search of the pinned model20 `interpolation2.text` finds no `lambda` line.
The strong prior from PR #55 holds; there is no discrepancy to report.

T3 already produces the λ value (`oxpinyin-lambda`,
`Lambda::table_conf_value()`). Wiring that into a `table.conf` emitter is
a separable follow-up, out of scope here.

---

## Count / float boundary

The format is **pure integer counts**. `export_interpolation` prints
`item.get_unigram_frequency()` (`size_t`) and `item->m_count` (`guint32`)
as decimal integers. `parse_interpolation2` parses the last field as
`u64`. No probability column exists, so no float tolerance is required.
The T2/T3 boundary is unchanged: counts stay `u64`; λ (T3) never enters
this file.

---

## Mapping (`export_interpolation.cpp` → `emit.rs`)

| `export_interpolation` | `emit.rs` |
|---|---|
| `fprintf("\\data model interpolation\n")` (`:34`) | `DATA_HEADER` |
| `fprintf("\\1-gram\n")` (`:76`) | `\1-gram` section |
| skip `freq == 0` (`:95-96`) | `if count == 0 { continue; }` |
| `taglib_token_to_string` / skip `NULL` (`:97-98`) | `phrase_text` / skip `None` |
| `fprintf("\\item %d %s count %ld\n", token, phrase, freq)` (`:98`) | `\item {token} {text} count {count}` |
| `fprintf("\\2-gram\n")` (`:107`) | `\2-gram` section |
| skip unless `word1 && word2` (`:130-132`) | skip unless both `phrase_text` hits |
| `fprintf("\\item %d %s %d %s count %d\n", …)` (`:131-132`) | `\item {prev} {w1} {cur} {w2} count {count}` |
| `fprintf("\\end\n")` (`:39`) | `\end` |
| `sentence_start` → `"<start>"` (`tag_utility.cpp:368-370`) | `SENTENCE_START_TEXT` |

Unigram walk order in the pin is library-then-token
(`PHRASE_INDEX_LIBRARY_COUNT` ranges, `:77-86`), which is ascending
token (library index occupies the high bits). Bigram walk order is
tkrzw `get_all_items` hash order (`:112`) then `retrieve_all` (ascending
`cur`). The Rust emitter walks the already-sorted `BTreeMap`s, so the
*file* is not byte-identical to a live export. Comparison is at the
**value** level — `(token, count)` / `((prev, cur), count)` — matching
T0 §9 and T2.

The emitter does **not** reproduce libpinyin's binary phrase-index /
DBM storage. It writes the textual format directly from T2's counts.

---

## Verification

### Round-trip through `parse_interpolation2` (CI-unconditional)

`crates/oxpinyin-emitter/tests/roundtrip.rs` emits a synthetic 2-unigram /
2-bigram model, writes it to a temp file, and calls
`oxpinyin_data::parse_interpolation2(&path)`
(`Result<UnigramTable, InterpolationError>`). The parsed unigram
`(token, count)` pairs equal the input bit-exact. The two bigrams are
checked through `parse_interpolation_dump` (the 2-gram half is outside
`parse_interpolation2`'s contract).

Pinned:

- Synthetic (CI-unconditional): **2 unigram records** survive emit → parse
  bit-exact (`tests/roundtrip.rs`).
- Fixture chain (`segmenter-ngseg.txt` → T2 counter → emit → parse,
  skipped without the migrate export): **138096 unigram records** survive
  emit → `parse_interpolation2` bit-exact; **199 bigrams** value-identical
  via `parse_interpolation_dump`. Pinned by
  `fixtures/w9/interpolation2.manifest`.

### Differential against `export_interpolation` (env-gated)

`crates/oxpinyin-emitter/tests/differential.rs::rust_matches_live_export_interpolation`
feeds the same fixture through
`gen_binary_files → gen_unigram → gen_ngram → export_interpolation` when
`PINYIN_GEN_BINARY_FILES`, `PINYIN_GEN_UNIGRAM`, `PINYIN_GEN_NGRAM`,
`PINYIN_EXPORT_INTERPOLATION`, and `PINYIN_GEN_NGRAM_DATA` are set. The
two texts are compared at the value level via `parse_interpolation_dump`.
A live export that contained a `lambda` line would fail the gate (it
does not).

Pinned (pin-built `2.11.91` pipeline, T1 fixture): **138096 unigrams,
199 bigrams, value-identical to `export_interpolation`.** No probability
fields; no float tolerance.

---

## W9-T4b (explicitly out of scope)

T4b is the corpus pipeline that would feed this emitter a real training
stream:

1. dump acquisition (Wikipedia or otherwise),
2. markup stripping,
3. traditional → simplified conversion,
4. sentence splitting.

It has **no differential oracle**: libpinyin's training corpus is
undocumented (T0). This PR reuses the existing synthetic T1/T2 fixture
chain and vendors no corpus, no model bytes, and no `table.conf`.

---

## Source index

| Claim | Source | Lines | Tag |
|---|---|---|---|
| header `\data model interpolation` | `utils/storage/export_interpolation.cpp` | 33-35 | SHOWN |
| footer `\end` | `export_interpolation.cpp` | 38-40 | SHOWN |
| `\1-gram` + `\item %d %s count %ld`, skip freq 0 / NULL text | `export_interpolation.cpp` | 75-104 | SHOWN |
| `\2-gram` + `\item %d %s %d %s count %d`, skip NULL text | `export_interpolation.cpp` | 106-143 | SHOWN |
| `sentence_start` → `"<start>"` | `src/storage/tag_utility.cpp` | 368-370, 391-393 | SHOWN |
| `import_interpolation` requires `model interpolation` | `utils/storage/import_interpolation.cpp` | 77-94, 134, 169 | SHOWN |
| KMM converter emits the same textual sections | `utils/training/k_mixture_model_to_interpolation.cpp` | 78, 138, 174 | SHOWN |
| `parse_interpolation2` reads `\1-gram` `token u32` + `count u64` | `crates/oxpinyin-data/src/interp.rs` | 149-252 | SHOWN |
| phrase text ignored; `\2-gram` skipped | `interp.rs` | 191-202 | SHOWN |
| λ ∈ `table.conf`, not `interpolation2.text` | PR #55; `export_interpolation.cpp` (no λ fprintf) | — | SHOWN |
