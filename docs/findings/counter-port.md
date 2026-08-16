# Counter port — `gen_ngram` n-gram counting (W9-T2)

This documents the Rust `oxpinyin-counter` crate, a value-level reproduction of
libpinyin's `gen_ngram` counting stage. It consumes T1's segmented-token
output and produces the unigram/bigram **integer counts** that W9-T3 (the
λ estimator) will interpolate.

The reproduction target is `utils/training/gen_ngram.cpp:78-125`, read from the
pinned libpinyin source (authorized: it is the trainer, not the decoder).

## Pipeline stage order (where the freq-1 floor belongs)

The pin's training pipeline runs four stages in this order:

1. `utils/storage/gen_binary_files` — builds the binary phrase/pinyin indexes
   from the `.table` sources.
2. `utils/training/gen_unigram` — **seed floor**: raises every phrase-index
   token's unigram frequency by the constant `freq = 1`
   (`gen_unigram.cpp:49`, `:67`), over both `SYSTEM_FILE` and `DICTIONARY`
   tables (`gen_unigram.cpp:45-46`).
3. `utils/training/gen_ngram` — **counting**: walks the segmented-token stream
   and adds `+1` unigram/bigram counts on top of the seeded index.
4. `utils/storage/export_interpolation` — textual dump of the resulting counts.

So the floor is a **data-build** step (`gen_unigram`), *not* part of
`gen_ngram`'s counting. `oxpinyin-counter` mirrors that split: `Counter` carries
a `floor_tokens` set (the phrase-index token set, from
`oxpinyin_segment::PhraseLexicon::tokens()`) and emits `1 + occurrences` for each
seeded token, `occurrences` for a token that appears in the stream but is
absent from the index (faithful to `add_unigram_frequency`, which creates the
item at the occurrence count).

## Input format

`gen_ngram` reads `TAGLIB_PARSE_SEGMENTED_LINE` (`gen_ngram.cpp:87`), which is
exactly T1's `Emitted::to_ngseg_line`:

- `{token} {phrase}` — one segmented token per line.
- `0 {raw}` — unknown text, emitted verbatim.
- an empty line — a `null_token` sentence separator.

`oxpinyin-counter`'s `parse_ngseg` reproduces that grammar (token + phrase
split on the first space/tab; empty line ⇒ `null_token`).

## Counting mapping (`gen_ngram.cpp:78-125` → `counter.rs`)

The C loop keeps two running tokens, both initialized to `null_token`
(`:78`), and for each parsed token does:

| `gen_ngram.cpp` | `counter.rs` |
|---|---|
| `last_token = cur_token; cur_token = token;` (`:89-90`) | `let last_token = mem::replace(&mut cur_token, token)` |
| `if (null_token == cur_token) continue;` (`:93`) | skip: a null second word contributes neither a unigram nor a bigram |
| `phrase_index.add_unigram_frequency(cur_token, 1);` (`:97`) | `occurrences[cur] += 1` |
| `if (null_token == last_token) { if (!train_pi_gram) continue; last_token = sentence_start; }` (`:100-104`) | substitute `SENTENCE_START` for `last_token`; with pi-gram disabled, `continue` drops the boundary bigram but keeps the unigram |
| `get_freq → set_freq(+1)`, else `insert_freq(1)` (`:115-118`) | `bigrams[(last, cur)] += 1` |

Notes:

- `--skip-pi-gram-training` is a `G_OPTION_FLAG_REVERSE` flag
  (`gen_ngram.cpp:38`): `train_pi_gram` defaults to `TRUE`, and the flag
  clears it. The pinned pipeline does **not** pass the flag, so pi-gram
  training is on; `Counter::new(train_pi_gram: true)` matches, and the CLI
  exposes `--skip-pi-gram-training` for parity.
- The first token of the stream follows the initial `last_token == null_token`,
  so its bigram is `(sentence_start, first_token)` — not a special case, just
  the same boundary rule applied to the implicit leading null.

## Integer counts only

The stored value is the raw count, never a probability. `SingleGramItem`
holds `guint32 m_freq` (`src/storage/ngram.cpp:33`); `retrieve_all` exposes it
as `m_count = m_freq` and computes `m_freq / (gfloat)total_freq` only at
retrieve time (`ngram.cpp:145-146`). `oxpinyin-counter` stores `u64` counts in
`Counts` (unigrams `token → u64`, bigrams `(prev, cur) → u64`) — no floats
appear anywhere in the count representation.

## Value-level differential parity

`gen_ngram` writes a binary phrase index + DBM bigram; `oxpinyin-counter` writes
a `Counts` value and a sorted text dump (`\1-gram` / `\2-gram` sections). Byte
layouts differ, so parity is asserted at the **value** level:

- The Rust counter runs over `fixtures/w9/segmenter-ngseg.txt` (T1's output)
  with the pinned phrase index as the floor seed.
- The pin side runs `gen_binary_files → gen_unigram → gen_ngram →
  export_interpolation` in a clean temp dir (`.table` + `table.conf` only, so
  `gen_ngram` does not append onto a stale `bigram.db`) and dumps
  `\item {id} {word} count {count}` lines (`export_interpolation.cpp`,
  `gen_unigram`/`gen_bigram`).
- `parse_interpolation_dump` turns that text back into a `Counts` and the two
  are compared field-by-field.

Result (pinned model20, T1's segmenter-han corpus):

- **138096 unigrams, 199 bigrams, value-identical to `gen_ngram`.**

The parity assertion lives in `crates/oxpinyin-counter/tests/differential.rs`:

- `rust_matches_committed_manifest` — skips if the migrate export is absent;
  otherwise compares against `fixtures/w9/counter-ngram.manifest` (unigram and
  bigram counts + an FNV-1a 64-bit checksum of the full dump).
- `rust_matches_live_gen_ngram` — skips unless the four pin binaries
  (`PINYIN_GEN_BINARY_FILES`, `PINYIN_GEN_UNIGRAM`, `PINYIN_GEN_NGRAM`,
  `PINYIN_EXPORT_INTERPOLATION`) and a data dir (`PINYIN_GEN_NGRAM_DATA`)
  are set; then runs the live pipeline and asserts value-identity. The
  failure path reports the *first* diverging token or pair and by how much,
  rather than tuning.

## What W9-T3 (λ estimator) consumes

`oxpinyin-counter` emits the **plain** counts: unigram `token → count` and
bigram `(prev, cur) → count`. T3's deleted-interpolation EM (the held-out /
deleted-count structure of `training-algorithm.md` §5) is *not* produced here:
T3 performs the held-out splitting over these counts, mirroring the pin's
`gen_deleted_ngram` stage, which sits downstream of `gen_ngram`. T2 stops at
the plain counts; the deleted counts are a T3 responsibility.
