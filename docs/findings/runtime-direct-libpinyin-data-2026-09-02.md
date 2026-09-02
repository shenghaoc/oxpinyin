# P6 — the production runtime reads libpinyin's own data directly

Date: 2026-09-02 · Status: **implemented, tested, benchmarked; the
production runtime and C ABI run on the P1–P4 readers**

This closes the P1–P5 arc. P1–P4 built lazy readers for libpinyin's own
files; P5 taught `oxpinyin-datagen` to write those files. P6 replaces the
eager runtime with the readers, so `Runtime::open` — and therefore
`pinyin_init` and the Python binding — opens a system data directory the
way libpinyin does and reads it the way libpinyin does. On Kyoto Cabinet
and tkrzw that directory is an **unmodified libpinyin install's `data/`**.

## 1. The architecture, before and after

```text
BEFORE (eager)                        AFTER (P6)
Runtime::open                         Runtime::open
  SystemDictionary::open                SystemDictionary::open
    load_phrase_index  ─ 138k rows        ChewingTable   (pinyin_index DBM handle)
    load_pinyin_index  ─ 93k keys         PhraseTable    (phrase_index DBM handle)
    derive_pinyin      ─ initial keys,     PhraseLibraries(mmap gb_char.bin … merged.bin)
                         unigram map,     BigramLanguageModel
                         reverse map        BigramTable  (bigram.db handle)
                                            + the same PhraseLibraries (unigrams)
  BigramLanguageModel::open             PunctTable       (punct.bin handle)
    slurp 56k rows                      AddonDictionary  (addon DBM pair, chunks on demand)
    set_unigrams_from_interpolation2   ── nothing scanned; every reader is a
      parse 79 MiB interpolation2.text     handle plus a mmap, lookups are point reads
```

The eager `SystemDictionary`/`BigramLanguageModel` and the
`interpolation2.text` unigram load are **gone from the crate**, not kept
alongside. `oxpinyin-data`'s `dict.rs` and `lm/mod.rs` are the lazy
readers; the old `chewing_dict.rs`, `lazy_punct.rs`, `initials.rs` and
the eager modules were deleted.

The semantic seam the engine sees is unchanged: `Dictionary` and
`LanguageModel` (`oxpinyin-core`). The runtime hands the engine a
`RuntimeDict` / `RuntimeLm` over the new readers; the engine, the C ABI's
candidate/prediction/training surface, and the Python binding did not
have to learn about KC, Tkrzw, chunk files, or DBM bytes.

## 2. The unigram is upstream's item field, not the corpus count

The one scoring change P6 required. The eager path loaded
`interpolation2.text` and handed the engine the raw `\1-gram` counts and
`Σ count`; the candidate law then re-added the `+1` per item and the item
count that the pin's *data* carries. Upstream never sees the raw count:
every reader of the unigram — the trellis (`PinyinLookup2`), the
candidate law (`_compute_frequency_of_items`), training
(`train_result3`) — reads `PhraseItem::get_unigram_frequency`, the count
plus the one `gen_unigram` writes into every item "to avoid zero value
when computing unigram frequency in float format", over
`get_phrase_index_total_freq()`, the sum of those fields. The chunk files
store exactly that field, so the lazy `BigramLanguageModel` hands the
engine the item field and the facade total, and the engine's `+1` /
`+ item count` re-derivation is removed. A phrase the corpus never saw is
`1/Σ` in the trellis, not the `UNKNOWN_COST` floor. Recorded in
`docs/findings/scoring-spec.md` (Architect correction log, 2026-09-02).
`interpolation2.text` is no longer read at runtime.

## 3. The drop-in invariant, end to end

`tools/bisection/run-same-data-dir-diff.sh` drives every `<so>
<systemdir>` C-ABI differential into the pin-built `libpinyin.so` and
oxpinyin's `libpinyin_capi.so`, **both opened on one unchanged
directory** — a libpinyin install's own `data/`. No conversion, no
import, no fixture image: the directory is the test input to both.

On `/opt/libpinyin-kc/lib/libpinyin/data` (a `--with-dbm=KyotoCabinet`
install), oxpinyin's C ABI is byte-identical to the pin on: the key
surface, the dictionary surface, phrase segmentation, exact-phrase
prediction, punctuation, addon candidates, user candidates, phrase-index
import, live post-choose typing, and n-best training. Two lines diverge,
both pre-existing and both registered:

* **predicted-prefix row order** — the pin's order is its DBM's physical
  hash-bucket walk; oxpinyin reproduces the library-grouped cursor order
  `reduce_tokens` concatenates (`resolve_suggestions`), which matches the
  pin's grouping but not its intra-group bucket order. `pred-order-diff`
  is the recorded-divergence gate, not a target of zero
  (`upstream-divergences.md`, "Predicted-candidate tie order").
* **one bigram-prediction row** in `union-diff` — a trained-count edge
  downstream of the n-best gfloat-trellis divergence
  (`upstream-divergences.md`, "N-best trellis accumulates gfloat log
  costs"): the two engines train the same phrases with counts that differ
  by the trellis's float/fixed-point residual, and at one prefix that
  tips a `count ≥ 10` prediction filter. Independent of the P6 switch (the
  user-store training code is unchanged); the standard `run-union-diff.sh`
  stays green.

The Rust seam has its own permanent test:
`crates/oxpinyin-runtime/tests/libpinyin_data_dir.rs` opens the
`OXPINYIN_LIBPINYIN_DATA_DIR` install through `Runtime::open` and asserts
the facade sizes (138,096 items, Σ item unigram 51,051,831), the
dictionary lookups, the possibility, the phrase DBM, the suggestion walk,
punctuation, the addon facade, and a decode — all through the public API.

## 4. Performance: the #260 four-cell matrix, same data per backend

`tools/bisection/run-perf-same-data.sh` — cells A/B are the pin on its
own `data/`, C/D are oxpinyin's C ABI **on those same two directories**.
Medians, CPU-pinned, 20 speed runs × 8 cycles:

| cell | init | alloc | cold cycle | steady | RSS after init | anon after init |
|---|---|---|---|---|---|---|
| libpinyin + Tkrzw | 0.79 ms | 0.00 ms | 8.94 ms | 8.12 ms | 12.8 MiB | 2.6 MiB |
| libpinyin + KC | 0.87 ms | 0.00 ms | 8.79 ms | 8.13 ms | 17.6 MiB | 7.2 MiB |
| oxpinyin + Tkrzw | 0.99 ms | 16.6 ms | 12.9 ms | 12.4 ms | 22.0 MiB | 7.6 MiB |
| oxpinyin + KC | 0.96 ms | 16.5 ms | 12.8 ms | 12.2 ms | 28.7 MiB | 12.8 MiB |

**Init: the P1–P6 goal.** The #260 baseline measured oxpinyin at ~86 ms
(KC) and ~106 ms (Tkrzw) of init, ~100× the pin. Removing the eager
reconstruction takes init to **0.96 ms (KC) and 0.99 ms (Tkrzw)** —
within ~1.1× and ~1.3× of the pin, ~90–106× faster than before.
`interpolation2.text` (79 MiB) is no longer read.

**The residual, stated honestly.** Two costs moved rather than
vanishing, and neither is hidden:

* **alloc** grew from ~0 to ~16.5 ms. `pinyin_alloc_instance` builds the
  430-syllable key-cost table (`key_cost_table`), which is now 430 DBM
  point reads plus a score each; the eager path paid the same work at
  init, resident. It is per-instance, and a natural next target (memoize
  across instances of one context, or lazily on first keystroke).
* **steady-state** is ~12.2 ms vs the pin's ~8.1 ms (~1.5×). Every
  candidate lookup is now a DBM `get` + decode rather than a resident
  vector probe. This is the space-for-time trade the direct-consumption
  architecture makes; it is the one number to drive down next, and the
  `open_profile` example (`oxpinyin-data`) is the instrument.

**Memory.** `open_profile` on the pin's KC data dir: `SystemDictionary::open`
+ LM + punct is **2.4 ms** and leaves **RssAnon ≈ 5.4 MiB**, RssFile
≈ 9.2 MiB — the file-backed chunk maps and DBM pages carry the data, not
reconstructed heap. The old eager path's dominant anon cost (the 79 MiB
interpolation table plus the reconstructed reverse/initial maps) is gone.

## 5. What changed, by crate

* `oxpinyin-store` — `DEFAULT_STORE_IS_LIBPINYIN_DBM` (KC/Tkrzw true).
* `oxpinyin-data` — `system_files.rs` (the DBM/chunk file names per
  backend); `dict.rs` rewritten as the lazy `SystemDictionary` +
  `AddonDictionary`; `lm/mod.rs` rewritten over `BigramTable` + the chunk
  unigrams; `phrase_libraries.rs` extended (load/unload, possibility);
  `punct.rs` is the lazy `PunctTable`; `chewing_table.rs` gained
  `key_exists`, `walk_extensions` and the upstream index/record semantics;
  `phrase_table.rs` gained `search_suggestion`. Eager modules deleted.
* `oxpinyin-datagen` — one backend-generic serializer set (the native
  schema and its `compile`/`compile_libpinyin` split are gone); the CLI
  writes one data directory per backend.
* `oxpinyin-runtime` — `Runtime::open` opens a system data directory
  through the readers; no fixture mode (the `fixtures/w3/<backend>` mini
  set is a real, small data directory).
* `oxpinyin-engine` — the candidate law reads the item field directly.
* `oxpinyin-capi` / `oxpinyin-zhuyin-capi` / `oxpinyin-python` — fixture
  mode removed; the token unigram surface reports the item field.
* `oxpinyin-segment` and the W9 tools — the lexicon reads the chunk files,
  the bigram is the lazy `BigramTable`.

## 6. Remaining blockers to a true drop-in replacement

None for correctness or the same-backend data contract. The open items
are performance, not architecture:

1. **steady-state ~1.5× the pin** — the DBM-read-per-lookup cost. A record
   cache over hot keys, or a per-session decode-time memo, is the lever.
2. **alloc ~16.5 ms** — the key-cost table; memoize per context.
3. **redb / LMDB** keep their own container bytes (correct — no libpinyin
   build reads them); their perf is not measured here.
