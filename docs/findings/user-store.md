# libpinyin user store — behaviour characterization (W6-T0)

Date: 2026-08-15 · Status: **SHOWN-verified against libpinyin 2.11.91 and
ibus-libpinyin 1.16.5 source** · Decision: **W6 reproduces libpinyin's
user-data *values and semantics*; the on-disk binary format
(MemoryChunk/DBM) is a NON-GOAL (redb is the store).** See §10.

This finding records, with source file + line citations, the exact behaviour a
later Rust implementation must reproduce to make pinyin-rs's user store
*value-identical* to libpinyin's on identical C-ABI call sequences. It is a
characterization, not an implementation. No code is added by this task.

Every claim below is tagged **SHOWN** (read directly from the cited source) or
**INFERRED** (cross-file behaviour, a design proposal, or a fact not directly
visible at the cited line). Proposed W6 sub-tasks in §8 are INFERRED by
construction (they are design, not upstream fact).

## Source set

- libpinyin: pinned tag `2.11.91` (sha `0c5e80e1…`), on disk at
  `/tmp/libpinyin-2.11.91`. Paths below beginning `src/` are relative to that
  root. (A byte-identical second copy sits under
  `/tmp/pinyin-rs-oracle/src/libpinyin-2.11.91`; verified identical for every
  file cited here.)
- Frontend: ibus-libpinyin pinned tag `1.16.5` (sha `2d2cdac0…`), on disk at
  `/tmp/pinyin-rs-oracle/src/ibus-libpinyin-1.16.5`. Paths beginning `src/PY…`
  are relative to that root.
- pinyin-rs: this repo. Paths beginning `crates/` are repo-relative, valid
  against `main` (`4a9a6f0`, post-W9). Reading these used rust-analyzer via the
  LSP (available — see §7).
- Provenance of the pinned tags: `docs/findings/oracle-environment.md:15-16`.

Constants used throughout (all **SHOWN**):

| Symbol | Value | Source |
|---|---|---|
| `initial_seed` | `23 * 3` = 69 | `src/lookup/phonetic_lookup.h:847`; also `src/pinyin.cpp:2502,2595` |
| `expand_factor` | `2` | `src/lookup/phonetic_lookup.h:848` |
| `unigram_factor` (training) | `7` | `src/lookup/phonetic_lookup.h:849`; `src/pinyin.cpp:2503,2596` |
| `pinyin_factor` | `1` | `src/lookup/phonetic_lookup.h:850` |
| `ceiling_seed` | `23 * 15 * 64` = 22080 | `src/lookup/phonetic_lookup.h:851` |
| `default_count` (add phrase) | `5` | `src/pinyin.cpp:521` |
| `unigram_factor` (add phrase) | `3` | `src/pinyin.cpp:522` |
| `USER_DICTIONARY` | `7` | `src/include/novel_types.h:161` |
| `ADDON_DICTIONARY` | `5` | `src/include/novel_types.h:159` |
| `PHRASE_MASK` | `0x00FFFFFF` | `src/include/novel_types.h:41` |
| `PHRASE_INDEX_LIBRARY_MASK` | `0x0F000000` | `src/include/novel_types.h:42` |
| `PHRASE_INDEX_LIBRARY_INDEX(t)` | `(t & 0x0F000000) >> 24` | `src/include/novel_types.h:44` |
| `sentence_start` | `1` | `docs/findings/training-algorithm.md` (`novel_types.h:122`) |
| `LIBPINYIN_SAVE_TIMEOUT` | `5 * 60` = 300 s | `src/PYLibPinyin.cc:29` |
| `USER_BIGRAM` | `"user_bigram.db"` | `src/pinyin_internal.h:58` |
| `USER_PINYIN_INDEX` | `"user_pinyin_index.bin"` | `src/pinyin_internal.h:61` |
| `USER_PHRASE_INDEX` | `"user_phrase_index.bin"` | `src/pinyin_internal.h:63` |
| `SYSTEM_BIGRAM` | `"bigram.db"` | `src/pinyin_internal.h:57` |

---

## 1. Purpose and scope

The user store is the read-write half of the model: the **user bigram**
(`m_user_bigram`, `src/pinyin.cpp:52`) and the **user phrase library**
(sub-index `USER_DICTIONARY = 7` inside the shared `FacadePhraseIndex`), plus
the user copies of the pinyin/phrase index tables. It records what the person
actually types and adapts rankings over time. libpinyin builds it from the
same `SingleGram`/`Bigram`/`PhraseIndex` machinery as the system model, but
mutable — updated at runtime on candidate selection and persisted
periodically.

Scope of this document: the update path, the user phrase index and token
allocation, persistence/save, the merge with system data at decode time, what
the pinned frontend calls and when, pinyin-rs's current state, a proposed W6
task breakdown, a value-level differential-parity plan, and the explicit
non-goals. `import_interpolation` (a *training-time* system-model tool) is out
of scope for the user store — see §8's note.

---

## 2. The user-bigram (and user-unigram) update algorithm

Two entry points feed the store on selection: `pinyin_train` (the main
sentence-training path) and `pinyin_choose_candidate` /
`pinyin_choose_predicted_candidate` (per-candidate side effects). All are
**SHOWN**.

### 2.1 Main path — `pinyin_train` → `train_result3`

`pinyin_train` (`src/pinyin.cpp:2668-2689`) refuses when there is no user dir
(`:2669`), sets `m_modified = true` (`:2679`), and delegates to
`m_pinyin_lookup->train_result3(&matrix, m_constraints, result)` (`:2685`).
`m_pinyin_lookup` is a `PhoneticLookup<2,3>` (`src/pinyin.cpp:55,406`), so the
implementation is the template method `train_result3` in
`src/lookup/phonetic_lookup.h:844-936`.

The algorithm (**SHOWN**, `phonetic_lookup.h:844-936`), per accepted sentence:

```
last_token = sentence_start               // = 1
for i in 0 .. constraints.length():
    token = result[i]
    if token == null_token: continue
    constraint = constraints.get_constraint(i)
    if train_next OR constraint.m_type == CONSTRAINT_ONESTEP:
        set train_next per constraint type            // :866-872
        seed = initial_seed                            // = 69
        // ---- bi-gram (into the USER bigram) ----
        if last_token != 0:                            // :876
            user = m_user_bigram.load(last_token) or new SingleGram
            total_freq = user.get_total_freq()
            if not user.get_freq(token, freq):         // token unseen after last_token
                user.insert_freq(token, 0); seed = initial_seed      // :889-890
            else:
                seed = max(freq, initial_seed)
                seed *= expand_factor                  // ×2   :893
                seed = min(seed, ceiling_seed)         // cap 22080  :894
            if overflow guard trips: goto next         // :898-899
            user.set_total_freq(total_freq + seed)     // :901
            user.set_freq(token, freq + seed)          // :903
            m_user_bigram.store(last_token, user)      // :904
        // ---- uni-gram (into the phrase index sub-index owning token) ----
        increase_pronunciation_possibility(..., seed * pinyin_factor)  // :925-927
        m_phrase_index.add_unigram_frequency(token, seed * unigram_factor)  // :928-929
    last_token = token
```

What gets incremented, by how much, against which tokens (**SHOWN**):

- **User bigram** `(last_token → token)`: the per-token count and
  `last_token`'s total both rise by `seed`. On the *first* time `token` is seen
  after `last_token`, `seed = 69`; on repeats, `seed = min(max(prev_freq, 69) *
  2, 22080)`. The seed grows **geometrically** — and because it is added to the
  prior count (`new = prev + 2 * prev`), the stored count **triples** per
  reselection, not doubles: the seed sequence is `69, 138, 414, 1242, 3726,
  11178, 22080` (clamped), confirmed by W6-T1's implementation. `last_token`
  starts at `sentence_start` (1), so the first phrase of a sentence trains the
  `sentence_start → token` bigram.
- **Phrase-index unigram** of `token`: raised by `seed * 7`
  (`add_unigram_frequency`, `:928`). `FacadePhraseIndex::add_unigram_frequency`
  (`src/storage/phrase_index.h:628-634`) routes the delta by
  `PHRASE_INDEX_LIBRARY_INDEX(token)` to the sub-index that *owns* the token —
  system tokens land in the in-memory system sub-index (persisted as a diff,
  §4); user tokens land in the user sub-index.
- **Pronunciation possibility** of `token` for the matched keys: raised by
  `seed * pinyin_factor` = `seed` (`increase_pronunciation_possibility`,
  `:925`).

Training touches the user bigram only for `CONSTRAINT_ONESTEP` positions (and
positions the previous step marked `train_next`) — i.e. the phrases the user
actually pinned, not every Viterbi edge (**SHOWN**, `:866`).

### 2.2 Per-candidate side effects — `pinyin_choose_candidate`

`pinyin_choose_candidate` (`src/pinyin.cpp:2499-2587`) does *not* train the
user bigram on the normal path. For a `NORMAL_CANDIDATE` it only records a
constraint (`add_constraint`, `:2580`) and returns the new cursor; the bigram
update is deferred to a later `pinyin_train` call (**SHOWN**). Special types
train a unigram directly:

- `LONGER_CANDIDATE` (`:2521-2530`) and `SORT_WITHOUT_SENTENCE_CANDIDATE`
  (`:2563-2574`): `add_unigram_frequency(token, initial_seed * unigram_factor)`
  = `69 * 7 = 483` only.
- `ADDON_CANDIDATE` (`:2532-2561`): promotes an addon-dictionary phrase into
  the live phrase index (new token from the addon range, pinyin/phrase index
  entries added), then falls through to normal handling.
- `NBEST_MATCH_CANDIDATE` (`:2513-2519`): rewrites constraints only.

### 2.3 Prediction path — `pinyin_choose_predicted_candidate`

`pinyin_choose_predicted_candidate` (`src/pinyin.cpp:2589-2639`, **SHOWN**)
uses a *flat* increment, not the doubling of §2.1: it adds `initial_seed *
unigram_factor` to the unigram (`:2607`), and for the user bigram inserts or
adds a flat `initial_seed` (69) to `(prev_token → token)` and to
`prev_token`'s total (`:2630-2636`). This is the simpler learning applied to
accepted *predictions*.

---

## 3. The user phrase index and token allocation

New words the user types enter the shared `FacadePhraseIndex` under the
`USER_DICTIONARY` (=7) sub-index. All **SHOWN**.

### 3.1 Adding a remembered phrase — `pinyin_remember_user_input`

`pinyin_remember_user_input` (`src/pinyin.cpp:3667-3708`): validates length
(`0 < len < MAX_PHRASE_LENGTH`), pre-computes the phrase's tokens
(`_pre_compute_tokens`, `:3686`), then recurses through the pinyin matrix with
`_remember_phrase_recur` (`:3700`).

`_remember_phrase_recur` (`src/pinyin.cpp:3576-3665`) fixes the target
sub-index to `const guint8 index = USER_DICTIONARY;` (`:3585`), walks the key
matrix to find a valid pronunciation whose length matches the phrase, and calls
`_add_phrase(context, index, cached_keys, phrase, phrase_length, count)`
(`:3603`).

### 3.2 Token allocation — `_add_phrase`

`_add_phrase` (`src/pinyin.cpp:514-612`), the single allocation choke point:

- `count == -1` → `default_count = 5` (`:520-524`); `unigram_factor = 3`
  (`:522`).
- Searches the phrase table for the phrase (`:536-558`). **If it already exists
  in the same sub-index** (`PHRASE_INDEX_LIBRARY_INDEX(token) == index`,
  `:563`): removes the item, `add_pronunciation(keys, count)`, re-adds
  (`:574-583`) — i.e. merges a new reading into the existing user phrase.
- **Otherwise (new phrase in this sub-index)** (`:584-608`): `get_range(index,
  range)`; `token = range.m_range_end`; if the low 24 bits are zero, `token++`
  (skip the reserved id) (`:592-594`). Then it wires the phrase into all three
  tables — `phrase_table->add_index`, `pinyin_table->add_index`,
  `phrase_index->add_phrase_item` (`:597-603`) — and seeds the unigram with
  `add_unigram_frequency(token, count * unigram_factor)` = `count * 3`
  (`:604-605`).

So **token allocation = "max token in the sub-index + 1"** (`range.m_range_end`,
bumped past the reserved zero id). User tokens are distinguished from system
tokens purely by the library nibble: `PHRASE_INDEX_LIBRARY_INDEX(token) == 7`
(see `pinyin_is_user_candidate`, `src/pinyin.cpp:3710-3722`).

**Unigram-factor reconciliation (W6-T3).** The `unigram_factor = 3` above and
the training path's `unigram_factor = 7` (`src/lookup/phonetic_lookup.h:849`,
§2) are **two distinct constants in two distinct functions for two distinct
operations**, both **SHOWN**: `7` scales the *training* seed into the
phrase-index unigram (§2.1–2.3 — `train_result3`,
`pinyin_choose_candidate`'s special types, `pinyin_choose_predicted_candidate`),
while `3` scales the *new-phrase* seeding count at allocation (§3.2 —
`_add_phrase`, `pinyin.cpp:522`). There is no discrepancy: a phrase added by
`pinyin_remember_user_input` seeds its unigram with `count * 3`, and later
*selections* of it train with `seed * 7`. pinyin-rs reproduces both as
separate constants (`pinyin_user::seed::UNIGRAM_FACTOR` = 7,
`pinyin_user::phrase::ADD_PHRASE_UNIGRAM_FACTOR` = 3).

### 3.3 Importing a phrase list

`pinyin_begin_add_phrases(context, index)` (`src/pinyin.cpp:506-512`) opens an
iterator targeting sub-index `index` (the caller passes `USER_DICTIONARY` for
user imports — §6). `pinyin_iterator_add_phrase(iter, phrase, pinyin, count)`
(`src/pinyin.cpp:614-653`) parses the pinyin with `FullPinyinParser2` under
`PINYIN_CORRECT_ALL | USE_TONE` (`:630-638`) and calls the same `_add_phrase`
allocator (`:646`). `pinyin_end_add_phrases` (`src/pinyin.cpp:655-…`) compacts
the phrase index and sets `m_modified = true` (`:657-658`).

### 3.4 Removing a user phrase — `pinyin_remove_user_candidate`

`pinyin_remove_user_candidate` (`src/pinyin.cpp:3724-3767`) asserts
`USER_DICTIONARY` ownership (`:3736`) and removes the phrase from the phrase
index, phrase table, and every pronunciation in the pinyin table
(`:3738-3760`), then masks it out of the user bigram: `user_bigram->mask_out(
PHRASE_INDEX_LIBRARY_MASK | PHRASE_MASK, token)` (`:3763-3764`).

---

## 4. Persistence and the save cycle

`pinyin_save` (`src/pinyin.cpp:1132-1147`, **SHOWN**):

1. Returns `false` if `m_user_dir` is unset (`:1133`) or `m_modified` is false
   (`:1136`) — **an unmodified context is a deliberate no-op**.
2. `m_phrase_index->compact()` (`:1139`).
3. `_write_files(context) && _rename_files(context)` (`:1141`).
4. `mark_version(context)` (`:1143`); `m_modified = false` (`:1145`).

`_write_files` (`src/pinyin.cpp:922-1023`) writes into `m_user_dir`, each to a
`.tmp` sibling first:

- **SYSTEM_FILE / DICTIONARY sub-index** (`:944-976`): writes a **delta**, not
  the whole library. It loads the original system chunk, computes
  `m_phrase_index->diff(i, chunk, log)` (`:963`), and `log->save(<user
  filename>.tmp)` (`:971`). This is how the system-token *unigram increments*
  from §2 are persisted — as a logger diff against the shipped system file.
- **USER_FILE sub-index** (`:978-993`): `m_phrase_index->store(i, chunk);
  chunk->save(…)` (`:981-988`) — the whole user library, wholesale.
- **User pinyin table** → `USER_PINYIN_INDEX.tmp` (`m_pinyin_table->store`,
  `:996-1003`).
- **User phrase table** → `USER_PHRASE_INDEX.tmp` (`m_phrase_table->store`,
  `:1005-1012`).
- **User bigram** → `USER_BIGRAM.tmp` (`m_user_bigram->save_db`, `:1014-1018`).

`_rename_files` (`src/pinyin.cpp:1025-1130`) atomically `rename(2)`s each
`.tmp` to its final name (`:1059,1078,1094,1108,1121`). Final on-disk names in
`m_user_dir`: `user_bigram.db`, `user_pinyin_index.bin`,
`user_phrase_index.bin`, and one file per non-user library carrying its diff
log.

**Value-level reproduction target** (the semantics W6 must match, *not* the
byte layout): (a) user-bigram counts per `(prev_token → token)` and each
`prev_token` total; (b) user-token unigram frequencies in the user sub-index;
(c) user phrase-index entries (phrase string, pronunciations, count) and the
pinyin/phrase table indices that reach them; (d) the system-token unigram
*deltas* (libpinyin's diff log; in pinyin-rs a redb delta table — INFERRED
mapping).

---

## 5. Merge / precedence with system data at decode time

> **Tension flag (per task instruction).** The user/system bigram merge lives
> inside the decode lookup path (`src/lookup/pinyin_lookup2.cpp`), which is
> decode-library internals. Characterizing the user store's *merge and
> precedence* required reading it. AGENTS.md's spec discipline forbids reading
> upstream C++ to derive the **decode algorithm**; this task authorizes reading
> the **user-data implementation** as the reproduction target. The two meet
> here. I did **not** re-derive the Viterbi/k-best decode or the λ scoring
> formula — those are already frozen in `docs/findings/scoring-spec.md`,
> `docs/findings/kbest-search.md`, and `docs/findings/decode-differential.md`.
> The facts below are limited to *where and how user data combines with system
> data*, and I surface the boundary rather than resolving it unilaterally.

**SHOWN** (`src/lookup/pinyin_lookup2.cpp`, `src/storage/ngram.cpp`):

- In `search_bigram2` (`pinyin_lookup2.cpp:350-413`), for each prior step the
  lookup loads both grams for the same key: `m_system_bigram->load(index_token,
  system)` and `m_user_bigram->load(index_token, user)` (`:367-369`), then
  `merge_single_gram(&m_merged_single_gram, system, user)` (`:371`). All bigram
  probabilities are taken from the **merged** gram (`:378-400`).
- `merge_single_gram` (`src/storage/ngram.cpp:277-…`) is **additive**:
  `merged_total = system_total + user_total` (`:305`), and for a token present
  in both, `merged.m_freq = system.m_freq + user.m_freq` (`:333`); tokens in
  only one side are carried through unchanged (`:318-347`). The bigram
  probability is then `freq / total` over the merged counts (`:381`).
- The score blends **bigram vs unigram**, not user vs system:
  `log((bigram_lambda * bigram_poss + unigram_lambda * unigram_poss) *
  pinyin_poss)` (`pinyin_lookup2.cpp:466`), with `bigram_lambda = lambda` and
  `unigram_lambda = 1 - lambda` (`pinyin_lookup2.cpp:30-31`).

**Precedence answer (the part that affects decode):** the user bigram is
consulted **in addition to** the system bigram, combined by **summing counts**
before the probability is computed. There is **no separate user-vs-system
weight** and λ does **not** govern that blend — λ interpolates the merged
bigram against the unigram. A user selection therefore raises the merged
`(prev → token)` count (and total), which raises `bigram_poss`, which raises
that continuation's decode score. The magnitude of the nudge is exactly the
`seed` arithmetic of §2 flowing through this additive merge.

---

## 6. What the pinned frontend calls, and when

From ibus-libpinyin 1.16.5 (**SHOWN**). The library is otherwise policy-free;
the frontend drives the lifecycle.

**On candidate selection** — `LibPinyinCandidates::selectCandidate`
(`src/PYPLibPinyinCandidates.cc:~95-178`):

- `pinyin_choose_candidate(instance, offset, candidate)` — always
  (`:111,132,140,147`).
- On sentence completion (cursor reaches end, or an n-best index ≠ 0):
  `pinyin_train(instance, index)` (`:117,155`) — the §2.1 update.
- If the `remember-every-input` option is on:
  `LibPinyinBackEnd::rememberUserInput(instance, str)` (`:121,158`) →
  `pinyin_remember_user_input(instance, phrase, -1)`
  (`src/PYLibPinyin.cc:389`) — adds the committed sentence as a user phrase.
- `LibPinyinBackEnd::instance().modified()` (`:122,134,142,159`) — arms the
  save timer.

**On candidate removal** — `removeCandidate`
(`src/PYPLibPinyinCandidates.cc:180-196`): `pinyin_remove_user_candidate`
(`:193`), only for `CANDIDATE_USER` / `CANDIDATE_LONGER_USER`.

**Persistence is a debounced timer, not focus-out** — `modified()`
(`src/PYLibPinyin.cc:216-225`) restarts a `GTimer` and (re)arms
`g_timeout_add_seconds(LIBPINYIN_SAVE_TIMEOUT = 300, timeoutCallback)`.
`timeoutCallback` (`:406-421`) fires `saveUserDB()` once ≥ 300 s have elapsed
since the last modification; `saveUserDB()` (`:423-431`) calls
`pinyin_save(m_pinyin_context)` (and the chewing context). `pinyin_save` is
also called immediately after a phrase-list import (`src/PYLibPinyin.cc:275`).
`focusOut` (`src/PYPPinyinEngine.cc:496`) does **not** save (**SHOWN**: no save
call in the override), and the backend destructor removes the timer without a
final flush (`src/PYLibPinyin.cc:45-48`) — so modifications newer than the last
timer tick can be lost on abrupt shutdown (**INFERRED** from the absence of a
flush path; a `pinyin_save` on the disconnect/exit path elsewhere would settle
it).

**Import** — `LibPinyinBackEnd::importPinyinDictionary`
(`src/PYLibPinyin.cc:230`) uses `pinyin_begin_add_phrases(context, …)` /
`pinyin_iterator_add_phrase` / `pinyin_end_add_phrases`
(`:237,267`), then `pinyin_save` (`:275`). (The same trio at `:540,570` sits in
`importRestNetworkDictionary`, a network-dictionary path that is a §10
non-goal.)

**Clear** — `clearPinyinUserData` (`src/PYLibPinyin.cc:356-381`) uses
`pinyin_mask_out` for `all` / `user` / addon targets (`:363-370`).

**Consequence for W6:** to make the C-ABI non-stub for the real consumer, the
user store must back, at minimum: `pinyin_choose_candidate` (constraint +
special-type unigram), `pinyin_train`, `pinyin_remember_user_input`,
`pinyin_save`, `pinyin_mask_out`, `pinyin_remove_user_candidate`, the
`pinyin_*_add_phrase*` import trio, and the `pinyin_*_get_(bigram_)phrases`
export iterators (§9).

---

## 7. pinyin-rs current state and the gap

The LSP (rust-analyzer) **was available** and used for this section
(`goToImplementation` / `findReferences` / `documentSymbol`).

**The seam exists but is unwired.** `pinyin-core` defines
`pub trait UserModel` (`crates/pinyin-core/src/lib.rs:87-101`) with
`score(history, token) -> Cost` (`:95`) and `observe(history, token)`
(`:100`) — exactly the read/observe shape the user store needs. LSP
`goToImplementation` on the trait returns **no implementors**, and
`findReferences` returns a **single** hit (its own declaration): nothing
implements it and no engine code consults it.

**`pinyin-user` is an empty shell.** `crates/pinyin-user/src/lib.rs:1-4` is a
doc comment only ("redb ACID store for user data … The redb major version is
pinned"); `crates/pinyin-user/Cargo.toml` has an **empty `[dependencies]`** —
redb is not yet a dependency.

**The C-ABI user-data entry points are stubs** (all in `crates/pinyin-capi`):

| Symbol | File:line | Current behaviour |
|---|---|---|
| `pinyin_save` | `context.rs:64-73` | Validated **no-op**; returns `true`, writes nothing (comment: "pinyin-user has no persistence implementation yet"). |
| `pinyin_remember_user_input` | `user_data.rs:18-28` | **STUB**, returns `false` ("T4 will implement"). |
| `pinyin_train` | `candidates.rs:299-304` | Validated **no-op**; returns `true` ("no training with StubLm"). |
| `pinyin_choose_candidate` | `candidates.rs:240-268` | Resolves the candidate by pointer identity and calls `Session::select(index)`; **no** user-store write. |
| `pinyin_choose_predicted_candidate` | `candidates.rs:280-288` | Returns `false`. |
| `pinyin_mask_out` | `config.rs:114-119` | Returns `false` (no masking). |
| `pinyin_is_user_candidate` / `pinyin_remove_user_candidate` | `candidates.rs:195,215` | Provisional; "no user dictionary yet". |

**The gap.** There is no user bigram, no user unigram delta store, no user
phrase index, no persistence, and no decode-time merge. `Session::select`
records the selection for decoding but never `observe()`s it into any user
model (**SHOWN** by the empty `findReferences` on `UserModel`). W6 must (a)
implement the `UserModel` seam in `pinyin-user` over redb, (b) wire it into
`Session` and the capi entry points above, and (c) implement the additive
decode-time merge of §5.

---

## 8. Proposed W6 task breakdown (INFERRED — design, not upstream fact)

Each sub-task names what it implements and how it is verified. Ordering lets
storage + arithmetic land and be unit-tested before touching decode/parity.

- **W6-T1 — user store schema + update arithmetic.** redb tables in
  `pinyin-user`: user bigram (`(prev_token, token) → count`, `prev_token →
  total`) and user-unigram delta (`token → freq`). Implement `UserModel` for
  it. Encapsulate the §2.1 seed rule (first-seen 69; repeats
  `min(max(prev,69)*2, 22080)`) and the §2.3 flat rule.
  *Verify:* unit tests with hand-computed golden increments from §2; a
  determinism property test (same input+state → same counts).
- **W6-T2 — user phrase index + token allocation.** New-phrase insertion into a
  `USER_DICTIONARY` (=7) sub-index with "max token + 1" allocation (§3.2), and
  the user pinyin/phrase index tables.
  *Verify:* an added phrase yields a token with library nibble 7; round-trip
  lookup; the "merge new reading into existing phrase" branch (§3.2).
- **W6-T3 — wire training through the engine + capi.** `Session` records the
  chosen sentence and calls `observe`; make `pinyin_train`,
  `pinyin_choose_candidate` side effects, and `pinyin_remember_user_input`
  non-stub (§2, §3.1, §7).
  *Verify:* after a scripted selection sequence, the store's exported counts
  equal the §2 arithmetic.
- **W6-T4 — decode-time additive merge.** Consult the user bigram additively
  with the system bigram at scoring, per §5 (sum counts, then existing λ
  blend). This is the decode-touching step; treat §5's tension flag as a gate —
  reuse the already-frozen scoring formula, do not re-derive it.
  *Verify:* the release + export-gated parity harness
  (`real_tables_session_reports_parity`, per the parity finding) shows a
  trained phrase's rank rise, matching libpinyin.
- **W6-T5 — persistence.** `pinyin_save` writes the redb store atomically;
  load on init; dirty/`m_modified` tracking so an unmodified save is a no-op
  (§4).
  *Verify:* save → reopen → counts and phrases survive; unmodified save writes
  nothing.
- **W6-T6 — masking / removal.** `pinyin_mask_out` and
  `pinyin_remove_user_candidate` (§3.4, §6).
  *Verify:* masked tokens vanish from bigram and phrase index; the mask-value
  semantics (`LIBRARY_MASK | PHRASE_MASK`) match §3.4.
- **W6-T7 — import / export iterators.** `pinyin_*_add_phrase*` (import) and
  `pinyin_*_get_(bigram_)phrases` (export). The export iterators double as the
  §9 differential surface.
  *Verify:* import a list → export returns the same `(phrase, pinyin, count)`
  set.

*Note on `import_interpolation`.* `import_interpolation` /
`export_interpolation` live in `utils/storage/` (build/training tools) and
touch the **system** interpolation model, not the user store — confirmed by
grep: no reference in `src/pinyin.cpp` or the runtime library. They are
characterized under `docs/findings/training-algorithm.md` /
`docs/findings/lambda-port.md` and are **not** a W6 task.

---

## 9. Differential-parity plan (value-level, not byte layout)

libpinyin exposes a **format-independent** value dump that is the natural
comparison surface (**SHOWN**, `src/pinyin.h`):

- User phrase index: `pinyin_begin_get_phrases(context, index)` (`:173`) /
  `pinyin_iterator_has_next_phrase` (`:184`) /
  `pinyin_iterator_get_next_phrase(iter, &phrase, &pinyin, &count)` (`:197`) /
  `pinyin_end_get_phrases` (`:209`).
- User bigram: `pinyin_begin_get_bigram_phrases(context)` (`:219`) /
  `pinyin_bigram_iterator_has_next_phrase` (`:229`) /
  `pinyin_bigram_iterator_get_next_phrase(iter, &phrase, &pinyin, &count)`
  (`:242`) / `pinyin_end_get_bigram_phrases` (`:254`).

Each yields `(phrase, pinyin, count)` triples. The frontend's
`exportUserPhrase` / `exportBigramPhrase` (`src/PYLibPinyin.cc:280-321`)
demonstrate the intended use.

**Plan.** Drive the pinned oracle (the existing `pinyin-oracle` harness) and
pinyin-rs through an *identical* scripted C-ABI sequence — `pinyin_init` →
parse → `pinyin_choose_candidate` / `pinyin_train` /
`pinyin_remember_user_input` → `pinyin_save` — then compare the two exported
`(phrase, pinyin, count)` **sets**. Because the §2 arithmetic is exact integer
counting, this is an **exact-equality** comparison, not a tolerance. This
compares *values*, so redb ≠ MemoryChunk/DBM is irrelevant (§10).

Two complementary layers back it up: (a) unit golden vectors derived by hand
from §2 for the update arithmetic; (b) the release + export-gated decode parity
harness (`real_tables_session_reports_parity`) for the §5 rank effects — noting
that harness is release-only and export-gated (`docs/findings/` parity notes;
project memory).

---

## 10. Dependencies and non-goals

**Dependencies.**

- `redb` as the `pinyin-user` backend — *intended* (per the crate doc) but
  **not yet a dependency** (`crates/pinyin-user/Cargo.toml`). Adding it is a
  `Cargo.toml` change and therefore a maintainer-ask per AGENTS.md "Hard
  forbids"; W6-T1 must raise it, not slip it in.
- The `UserModel` seam (`crates/pinyin-core/src/lib.rs:87`) and the engine
  `Session` (`crates/pinyin-engine/src/session.rs`).
- The oracle export symbols (§9) and the parity harness.
- W9 (training pipeline) is **independent** — it builds the *system* model
  offline; the user store shares the bigram/λ math but does not depend on W9
  code.

**Non-goals (explicit).**

1. **Not** reproducing libpinyin's binary user-data format — `user_bigram.db`
   (a DBM/BerkeleyDB store), the `.bin` MemoryChunk dumps, or the phrase-index
   diff-logger byte layout. redb is the store; only the **values and
   semantics** are the target. This is the headline decision of this finding.
2. **Not** the K-mixture-model path (out of scope, as in W9).
3. **Not** reproducing the frontend's 5-minute debounce timer inside the
   library — that is frontend policy (§6); the library persists when
   `pinyin_save` is called.
4. **Not** cloud/network-dictionary import (`rememberCloudInput`,
   `readNetworkDictionary`) — frontend features outside the core user store.

---

## Appendix — open items marked INFERRED

- The mapping of libpinyin's system-token unigram **diff-logger** (§4) onto a
  redb representation is a design choice, not an upstream fact; W6-T5 settles
  it.
- Whether any shutdown/disconnect path flushes the store before the 5-minute
  timer (§6) is not visible in the files read; a `pinyin_save` on engine
  teardown elsewhere in the frontend would settle it.
- The exact numeric value of `lambda` is intentionally not restated here; it is
  frozen in `docs/findings/scoring-spec.md` / `docs/findings/lambda-port.md`.
  §5 depends only on the *structure* (λ blends bigram-vs-unigram; user/system
  is an additive pre-merge), which is SHOWN.
