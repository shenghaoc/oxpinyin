# libpinyin user store — behaviour characterization

Date: 2026-08-15 · Status: **SHOWN-verified against libpinyin and
ibus-libpinyin 1.16.5 source** · Store: **oxpinyin reads and writes each
backend's native libpinyin user files** (§11). The user-data values and
semantics are libpinyin's, and so is the on-disk format.

This finding records, with source file and line citations, the behaviour
oxpinyin's user store reproduces. A user directory written by oxpinyin or
libpinyin on the same backend opens in the other, in both directions
([compatibility policy, "Backends and user data"](compatibility-policy.md#backends-and-user-data)).

Every claim below is tagged **SHOWN** (read directly from the cited source) or
**INFERRED** (cross-file behaviour, or a fact not directly visible at the
cited line).

## Source set

- libpinyin: the pin (`074a2219`; `tools/oracle/oracle-pin.txt` is
  authoritative). Paths below beginning `src/` are relative to its root.
  The line citations in §1–§10 were first read at 2.11.91 (`0c5e80e1`);
  §2.3 and §11 cite `074a2219` directly.
- Frontend: ibus-libpinyin pinned tag `1.16.5` (commit `2d2cdac0…`). Paths
  beginning `src/PY…` are relative to its root.
- oxpinyin: this repo. Paths beginning `crates/` are repo-relative.
- Provenance of the pinned commits: `tools/oracle/oracle-pin.txt`.

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
the pinned frontend calls and when, oxpinyin's implementation map, the
value-level comparison surface, the non-goals, and the user-file
implementation (§11). `import_interpolation` (a *training-time* system-model
tool) is out of scope for the user store — see §8.

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
  11178, 22080` (clamped), as `oxpinyin_user::seed` reproduces. `last_token`
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

`pinyin_choose_predicted_candidate` at pin `074a2219`
(`src/pinyin.cpp:2591-2640`, **SHOWN**) uses a flat seed of 69, rather than
§2.1's reselection expansion. Punctuation returns without training. Other
predicted rows first add `69 * 7 = 483` to the unigram; integer overflow
returns `false` before any bigram write. A prefix row then returns `true`
(`:2614-2616`). Only a bigram row adds 69 to `(prev_token → token)` and to
its predecessor's total (`:2618-2639`). These choices leave `m_modified`
unchanged.

`UserStore::observe_predicted_prefix(&mut self, cur: Token) ->
Result<u64, UserStoreError>` implements the prefix arm. Its returned `u64`
is the **seed 69**, not the unigram increment 483 or the resulting counter.
The actual caller, `pinyin_choose_predicted_candidate` in the C facade,
uses `.is_ok()` and discards the seed. The method preserves the guarded
unigram add and the facade-total increment on overflow, touches no bigram,
and leaves the save gate unchanged. `observe_predicted(last, cur)` remains
the bigram-row arm.


Prediction ranking at the same pin reads the live `guint32` item counter
and wrapped facade total. Prefix rows compute `(1 - lambda) * unigram /
(float)total * 256 * 256 * 256` in `gfloat`
(`pinyin.cpp:1812-1825`). The general scorer can add the bigram term only
with `DYNAMIC_ADJUST` and a non-null predecessor (`:1845-1866`), but the
prediction caller initializes its predecessor to null and never changes it
(`:2418,2438`). Thus predicted bigram rows use the same unigram law in
both option states. Lambda is parsed directly as `gfloat` from `table.conf`
(`table_info.cpp:220`), not through the decoder's rational-to-f64 conversion
or a fitted constant. A decimal just below an f32 midpoint distinguishes
these paths (`0.9999999701976776123036`).

The `guint32` assignment truncates. The pinned x86-64 build performs a
64-bit signed float-to-integer conversion followed by a low-32-bit store;
finite amplified scores beyond `guint32::MAX` therefore wrap, rather than
saturate. The prediction code reproduces that conversion safely, including
the zero low bits of the invalid-conversion sentinel. The source expression
is `pinyin.cpp:1821-1824,1862-1866`; the reference instructions are
`cvttss2si %xmm1,%rax` followed by `mov %eax,0x1c(%r15)`.

Sorting is by displayed length, then `-(lhs_frequency - rhs_frequency)`
with unsigned subtraction and a signed `gint` return (`:1678-1708`).
For high scores this comparator is not transitive. The safe index merge
uses the loaded GLib 2.90.0 half-split order and chooses the left row for
comparison <= 0 ([`glib/gqsort.c:64-183`](https://github.com/GNOME/glib/blob/2.90.0/glib/gqsort.c#L64)).
It takes O(n log n) comparisons and two usize arrays instead of a temporary
array of candidate structs; the extra comparisons on already ordered input
trade against smaller scratch space. Equal scores preserve collection order.
Dedup retains the numeric score winner in its sorted position (`:2128-2136`).
Imported phrases also create index keys for their proper prefixes, without
requiring a phrase token for that prefix (`phrase_large_table3_bdb.cpp:297-315`
and its KC/tkrzw twins).

The general scorer asserts a positive total (`pinyin.cpp:1859`); prefix
rows bypass this assert. A prediction containing a bigram row with zero
total is the existing policy row 61 class (c): both plain and punctuation
entry points answer `false`, keep the list empty, and emit exactly one
`libpinyin` warning. It is an `assert`, live in the reference build.

Source read from a clean tree at full pin
`074a2219c90feaf962d0d24f034514033ece5f99`; cited source contents were
verified against the pin's Git blobs (pinyin.cpp `f27f7cf776724ead8d55e14b4af390e790054a91`),
and matched against the bdb oracle's source. Captured 2026-10-09 UTC.

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

**Unigram-factor reconciliation.** The `unigram_factor = 3` above and
the training path's `unigram_factor = 7` (`src/lookup/phonetic_lookup.h:849`,
§2) are **two distinct constants in two distinct functions for two distinct
operations**, both **SHOWN**: `7` scales the *training* seed into the
phrase-index unigram (§2.1–2.3 — `train_result3`,
`pinyin_choose_candidate`'s special types, `pinyin_choose_predicted_candidate`),
while `3` scales the *new-phrase* seeding count at allocation (§3.2 —
`_add_phrase`, `pinyin.cpp:522`). There is no discrepancy: a phrase added by
`pinyin_remember_user_input` seeds its unigram with `count * 3`, and later
*selections* of it train with `seed * 7`. oxpinyin reproduces both as
separate constants (`oxpinyin_user::seed::UNIGRAM_FACTOR` = 7,
`oxpinyin_user::phrase::ADD_PHRASE_UNIGRAM_FACTOR` = 3).

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

**Reproduction target.** oxpinyin matches these values and writes them in
the same files: (a) user-bigram counts per `(prev_token → token)` and each
`prev_token` total (`user_bigram.db`); (b) user-token unigram frequencies in
the user sub-index (`user.bin` and the other `USER_FILE` chunks); (c) user
phrase-index entries (phrase string, pronunciations, count) and the
pinyin/phrase table indices that reach them (`user_pinyin_index.bin`,
`user_phrase_index.bin`); (d) the system-token unigram deltas, as each
`SYSTEM_FILE` library's `.dbin` diff log.

**No format version (ruling, 2026-09-08).** libpinyin's user files carry
no version. The drop-in contract is to match them, not to add guarantees
they lack; the backend's own file header stays the only stamp. A hostile or truncated
file degrades per file at the load, as upstream's does, never a panic
(`crates/oxpinyin-user/src/persistence.rs`), with one exception: a
`database format:` in `user.conf` that upstream's
`to_table_database_format_type` does not know is the pin's abort
(`table_info.cpp:122-133`, reached from `:353-354`). The load returns
`UnknownDatabaseFormat` there, before anything is judged, wiped or
written, and `pinyin_init` and `zhuyin_init` abort on it (register row
44). Every other file error degrades that file alone. Do not reintroduce
a version row without a fresh ask.

---

## 5. Merge / precedence with system data at decode time

> **Source note.** The user/system bigram merge lives inside the decode lookup
> path (`src/lookup/pinyin_lookup2.cpp`), which is decode-library internals.
> Characterizing the user store's *merge and precedence* required reading it;
> The source policy permits reading upstream C++ as the reproduction
> target. The facts below are limited to *where and how user data combines
> with system data*; the λ scoring formula is defined in
> [`scoring-spec.md`](scoring-spec.md).

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

**Storage.** The user half of the merge is the pin's own `SingleGram`
value, held once, in the pin's own in-memory container (§11). Every read
takes the bytes apart the pin's way (`oxpinyin_core::single_gram`, a port of
`ngram.cpp`): a count is `get_freq`'s first match walking from libstdc++'s
`__lower_bound`, which answers deterministically on an unsorted array, and
duplicated items are kept. An item-less user gram is still a gram:
`merge_single_gram` adds its total whatever its length
(`ngram.cpp:298-306`), so the overlay is non-zero whenever the container
holds any key.

**Seam note.** The merge sits in oxpinyin's language model
(`BigramLanguageModel::score`, count addition before the λ blend), **not** in
`UserModel::score`, which stays `Ok(0)`: a `Cost` added after the λ blend
would be a new weighting scheme — exactly what this section forbids — so the
count-level merge must live in the language model. The `UserModel` seam is
therefore observe-only.

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

**Durability is save-gated** — training lives in the session state and
reaches the pin's files only at `pinyin_save`; a crash before the save
loses the sub-timer window, exactly as upstream's does (§11).

**Persistence is a debounced timer, not focus-out** — `modified()`
(`src/PYLibPinyin.cc:216-225`) restarts a `GTimer` and (re)arms
`g_timeout_add_seconds(LIBPINYIN_SAVE_TIMEOUT = 300, timeoutCallback)`.
`timeoutCallback` (`:406-421`) fires `saveUserDB()` once ≥ 300 s have elapsed
since the last modification; `saveUserDB()` (`:423-431`) calls
`pinyin_save(m_pinyin_context)` (and the chewing context). `pinyin_save` is
also called immediately after a phrase-list import (`src/PYLibPinyin.cc:275`).
`focusOut` (`src/PYPPinyinEngine.cc:496`) does **not** save (**SHOWN**: no save
call in the override), and the backend destructor removes the timer without a
final flush (**SHOWN**: `LibPinyinBackEnd::~LibPinyinBackEnd`
(`src/PYLibPinyin.cc:43-50`) destroys the timer, removes the timeout source,
and calls only `pinyin_fini`; `saveUserDB`'s only callers are the timer
callback and `importPinyinDictionary`). Upstream therefore **has no
flush-on-shutdown**: modifications younger than the last timer tick are lost
on abrupt exit.

**oxpinyin reproduces the call pattern and the loss window.**
oxpinyin's `pinyin_fini` does not save, matching upstream, so changes
younger than the last `pinyin_save` are lost on exit. `pinyin_save`
reproduces the §4 gate (`false` when unmodified or without a user dir) and
writes the pin's file set (§11).

**Import** — `LibPinyinBackEnd::importPinyinDictionary`
(`src/PYLibPinyin.cc:230`) uses `pinyin_begin_add_phrases(context, …)` /
`pinyin_iterator_add_phrase` / `pinyin_end_add_phrases`
(`:237,267`), then `pinyin_save` (`:275`). (The same trio at `:540,570` sits in
`importRestNetworkDictionary`, a network-dictionary path that is a §10
non-goal.)

**Clear** — `clearPinyinUserData` (`src/PYLibPinyin.cc:356-381`) uses
`pinyin_mask_out` for `all` / `user` / addon targets (`:363-370`).

**Consequence:** for the real consumer, the user store backs, at minimum:
`pinyin_choose_candidate` (constraint + special-type unigram), `pinyin_train`, `pinyin_remember_user_input`,
`pinyin_save`, `pinyin_mask_out`, `pinyin_remove_user_candidate`, the
`pinyin_*_add_phrase*` import trio, and the `pinyin_*_get_(bigram_)phrases`
export iterators (§9).

---

## 7. oxpinyin implementation map

`oxpinyin-core` defines the `UserModel` seam (`crates/oxpinyin-core/src/lib.rs`).
`oxpinyin_user::UserStore` implements it (`crates/oxpinyin-user/src/model.rs`)
and exposes the stored counts as `UserCountDelta` for the §5 merge.

| Behaviour | oxpinyin |
|---|---|
| §2.1 training seed arithmetic | `crates/oxpinyin-user/src/seed.rs` |
| §2.2 per-candidate side effects, §2.3 prediction | `crates/oxpinyin-engine/src/session/selection.rs`, `crates/oxpinyin-facade` |
| §3 user phrase index and token allocation | `crates/oxpinyin-user/src/phrase.rs`, `store.rs` |
| §4 save cycle and `m_modified` gate | `crates/oxpinyin-user/src/persistence.rs`, `UserStore::save_reporting` |
| §5 additive merge | `BigramLanguageModel` over `UserCountDelta` (`crates/oxpinyin-data/src/lm/mod.rs`) |
| §9 export iterators | `crates/oxpinyin-capi/src/iterators.rs`, `crates/oxpinyin-facade/src/export_rows.rs` |
| §11 user files | `crates/oxpinyin-data/src/user_files.rs`, `crates/oxpinyin-user/src/store_libpinyin.rs` |

The C entry points `pinyin_save`, `pinyin_train`, `pinyin_choose_candidate`,
`pinyin_choose_predicted_candidate`, `pinyin_remember_user_input`,
`pinyin_mask_out`, `pinyin_remove_user_candidate`, the import trio and the
export iterators all reach the store.

---

## 8. Training-time tools

`import_interpolation` /
`export_interpolation` live in `utils/storage/` (build/training tools) and
touch the **system** interpolation model, not the user store — confirmed by
grep: no reference in `src/pinyin.cpp` or the runtime library. They are
characterized under [`training-algorithm.md`](training-algorithm.md) and
are not part of the user store.

---

## 9. Value comparison surface

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

**Comparison.** Drive the pin and oxpinyin through an identical C-ABI
sequence — `pinyin_init` → parse → `pinyin_choose_candidate` /
`pinyin_train` / `pinyin_remember_user_input` → `pinyin_save` — then compare
the two exported `(phrase, pinyin, count)` **sets**. Because the §2
arithmetic is exact integer counting, this is an **exact-equality**
comparison, not a tolerance. Unit golden vectors derived by hand from §2
cover the update arithmetic. Because oxpinyin writes the pin's own files,
the files themselves are a second comparison surface (§11).

---

## 10. Dependencies and non-goals

**Dependencies.**

- The `UserModel` seam (`crates/oxpinyin-core/src/lib.rs`) and the engine
  `Session` (`crates/oxpinyin-engine/src/session/`).
- The backend store (`oxpinyin-store`), which reads and writes the user
  files in the backend's own containers.
- The export symbols (§9).
- The training pipeline is **independent** — it builds the *system* model
  offline; the user store shares the bigram/λ math but not its code.

**Non-goals (explicit).**

1. **Not** a private user-data format. oxpinyin writes libpinyin's own user
   files on every backend (§11); there is no conversion, import or version row.
2. **Not** the K-mixture-model path (out of scope for the user store).
3. **Not** reproducing the frontend's 5-minute debounce timer inside the
   library — that is frontend policy (§6); the library persists when
   `pinyin_save` is called.
4. **Not** cloud/network-dictionary import (`rememberCloudInput`,
   `readNetworkDictionary`) — frontend features outside the core user store.

---

## Appendix — settled INFERRED items

- System-token unigram increments persist as each `SYSTEM_FILE` library's
  `.dbin` diff log, as upstream's do (§4, §11).
- Upstream has no flush-on-shutdown (§6); oxpinyin reproduces the call
  pattern and the loss window.
- The exact numeric value of `lambda` is intentionally not restated here; it is
  defined in [`scoring-spec.md`](scoring-spec.md).
  §5 depends only on the *structure* (λ blends bigram-vs-unigram; user/system
  is an additive pre-merge), which is SHOWN.

---

## 11. Implementation record — the store persists in libpinyin's own files (2026-09-09)

The user directory holds the pin's own file set on every backend. A
same-backend pair (oxpinyin and libpinyin on Berkeley DB, Kyoto Cabinet or
tkrzw) reads and writes the same profile in both directions. A profile
from another backend family is non-conforming: `user.conf`'s
`database format` line fails the conformance check, and the profile is
wiped, exactly as upstream's `check_format` does.

Architecture — the session state behind the value engine:

* `oxpinyin_data::user_files` — the inventory (`UserDbm`: libpinyin's
  file names on every backend), the `user.conf` codec (`UserTableInfo`,
  `OPEN_COUNTER_LIMIT` = 6, `get_open_counter`, conformance), and the
  `PhraseIndexLogger` record codec (add/remove/modify/header;
  MODIFY_HEADER carries the payload length once, then two runs).
* `oxpinyin_user::persistence` — load, save and fini.
  - Load runs `check_format` first and wipes a non-conforming profile.
    It then reads the user bigram's raw rows verbatim, the `USER_FILE`
    chunk stores, and the `.dbin` logs replayed onto the original system
    chunks with merge's stop-at-first-mismatch.
  - Save is the pin's `_write_files` + `_rename_files`: every file whole
    to a `.tmp` sibling, then all renames.
  - Load, save and fini write `user.conf` under the facade's law (`UserConfLaw`, below).
* `user_phrase_index.bin` is saved as the session's stored records, as
  they are, never rebuilt from the `USER_FILE` items. At load it is read
  back only as a set of exact `(token, text)` membership pairs
  (`PHRASE_TABLE`) for `remove_user_phrase`'s phrase-table check
  (`pinyin.cpp:3750`; policy row 81). A token may sit under several texts,
  and the pin matches the exact pair; the file is never a text source.
  A derived `PHRASE_TABLE_BY_TEXT` index (`text ++ 0x00 ++ token_be`)
  gives one text's pairs a contiguous range for the membership read the
  zhuyin import makes (`phrase_table_tokens_for_text`). Every
  `PHRASE_TABLE` writer maintains it, and an empty one is rebuilt from
  `PHRASE_TABLE`. A profile without the table is not backfilled: a
  missing row is the pin's `:3750` state. The value engine's text lookups
  derive from `user.bin`.
* `user_pinyin_index.bin` keeps the per-reading `indexed` flag: a reading
  merged into an existing phrase is never indexed (`pinyin.cpp:569-582`),
  so its records are loaded and saved back as they are.
* **The session keeps the pin's own user bigram container, and it is the
  only copy.** A libpinyin session holds `Bigram::m_db` as the pin builds
  it — tkrzw `TinyDBM`, Kyoto Cabinet `StashDB` via `load_snapshot`,
  Berkeley DB an in-memory `DB_HASH` by cursor copy
  (`oxpinyin_store::UserBigramDb`). The session reads `user_bigram.db`
  once, by `load_db`, after the profile check. Every bigram operation
  then runs the pin's own sequence on the raw grams:
  - training (`phonetic_lookup.h:876-909`);
  - predicted training (`pinyin.cpp:2622-2638`);
  - prediction (`pinyin.cpp:2321-2341`);
  - the export's lazy per-key load (`pinyin.cpp:775-787`, `:885-890`);
  - `Bigram::mask_out` for a mask or a removal (`ngram_bdb.cpp:217-254`
    and its twins), which leaves a gram it removed nothing from untouched.

  Its walk order is therefore the pin's. The bigram export iterates it,
  and `pinyin_save` writes `user_bigram.db` from it by the pin's
  `save_db`. The fault checks read the live container: non-token keys
  from its keys, short and partial values from the bytes, and a mask's
  residual total by running the ported mask on a copy. A writer takes the
  database guard, then the container, and moves the write generation
  before it releases the database guard. `UserState.bigram` holds the raw
  `(key, value)` rows in file order. `persistence::load` fills it and
  `persistence::save_state_reporting`, which has no container, writes it.
* **The user bigram's container is backend-specific, and is not the
  system bigram's container.** On Kyoto Cabinet the *system* `bigram.db`
  is a `HashDB` file (`attach`, `ngram_kyotodb.cpp:110`), but the *user*
  `user_bigram.db` is an in-memory `StashDB` dumped as a snapshot stream,
  magic `KCSS` (`load_db`/`save_db`'s `load_snapshot`/`dump_snapshot`,
  `ngram_kyotodb.cpp:54-101`). tkrzw uses a `HashDBM` file for both
  (`ngram_tkrzwdb.cpp:48-84`), and Berkeley DB a `DB_HASH` file for both.
  The store therefore has its own seam —
  `RawReadStore::open_user_bigram` / `WriteStore::write_user_bigram`.
  Its default is the hash container (correct for tkrzw and Berkeley DB),
  and Kyoto Cabinet overrides it with the stash-plus-snapshot form.
  Opening the user bigram through the *system* bigram's hash path answers
  `kcdbopen: missing magic data of the file` on Kyoto Cabinet.
* `oxpinyin_user::store_libpinyin` — `UserStore::open_libpinyin` reads the
  profile into the **session's state in process memory**: the user
  library arenas, the pinyin index and phrase table, the pin's user
  bigram container, the live system items and the libraries'
  stored-total offsets. Every value operation runs against that state,
  and `save()` exports the session back into the file set. The pin reads
  no temp variable and opens no temp file anywhere under `src` at
  074a2219; its context holds the loaded profile in memory
  (`pinyin.cpp:326-444`). Process memory is private, so a `fork`ed
  child's save cannot disturb the parent. The value mapping is
  one-for-one:
  - the bigram is the pin's own container (above);
  - a user item is the `PHRASE` text, the `PRONUNCIATION` rows (packed
    keys — one wire form) and the token's full `UNIGRAM` accumulation
    (`count·3` base, `seed·7` training);
  - system imports retain full packed item payloads in a sparse
    session-memory map, and original items are read lazily from shared
    immutable mappings;
  - appended system items also have user lookup rows for their creation reading;
  - save emits full-item `.dbin` ADD/MODIFY records using local slots
    (`074a2219`, `phrase_index.cpp:394–442`), including pronunciation changes.

Semantics this preserves:

* **The §4 gate is the pin's** — only training and the import trio's
  `mark_modified` arm `m_modified` (the pin sets it in `pinyin_train`
  and `pinyin_end_add_phrases` alone), so `pinyin_save` before either is
  a no-op.
* **Nothing is durable between saves** — a crash loses the sub-timer
  window exactly as upstream's does. The user directory holds only the
  pin's files.
* System import pronunciation changes are preserved, including
  pin-created payloads read and saved back through oxpinyin. The separate
  training path still lacks per-pronunciation updates, and replayed
  REMOVE records remain outside the runtime value model.
* **The open counter's lifecycle is the facade's, and fini lowers it.**
  The two pins differ here, and `UserConfLaw` names which one a profile
  follows.
  - libpinyin raises the counter at init and writes the marker
    (`pinyin.cpp:185-187`), writes the same value back at save
    (`mark_version`, `:1143`), and lowers it at fini —
    `get_open_counter` then `counter > 1 ? counter - 1 : 0` — writing
    again whether or not a save came first (`:1194-1200`).
  - libzhuyin never moves it: init only reads the marker
    (`zhuyin.cpp:126-162`), save writes a fresh `UserTableInfo` with
    counter 0 (`:164-176`, `:695`), and fini writes nothing (`:741-757`).

  Neither wipe removes `user.conf`. The fini write belongs to the store
  handle whose open raised the counter: dropping it is the context's
  `*_fini`. A process that dies before its fini leaves the counter
  raised, so seven killed launches in a row wipe the profile at the
  eighth, where the pin wipes it too.
* **Every open is a session of its own.** Upstream holds nothing per
  directory: every init reads the profile into its own context
  (`pinyin.cpp:326-444`) and every save writes that context's whole state
  (`:1132-1147`). `open_libpinyin` reserves a fresh session per open and
  runs `check_format` every time. Two contexts on one directory are
  therefore independent, and when both save, the later save's files are
  the profile.
* **The whole marker is the pin's `fscanf` sequence, and its abort point
  is a refusal.** `UserTableInfo::load` is four `fscanf` calls over one
  stream (`table_info.cpp:338-359`), and `UserTableInfo::parse` is that
  sequence, byte for byte:
  - both version directives are `%d` (`:338`, `:344`): a sign, every
    digit, `strtol`'s saturation at `long`, and the low 32 bits kept by
    the store through `int *`. `+7` and `4294967303` read 7 and conform;
  - `database format:` is `%255s` (`:352`): white-space prefixed, at
    most 255 bytes, stopped by white space, so a token can cross a
    newline and a trailing word is left for the next directive;
  - the counter directive reads from where the third stopped — one
    attempt at its literal, then `%d` into an `int`
    (`table_info.cpp:356-359`, `table_info.h:102`). It takes a sign, a
    negative value, the digits before trailing garbage, a value on the
    next line, and the low 32 bits of a value past `int`. A junk line
    between the two reads 0, and a failed conversion reads 0;
  - `parse` takes bytes, and bytes upstream never reads decide nothing.
    `UserTableInfo::open_counter` is an `i32`.

  The one outcome that is not a value is
  `to_table_database_format_type`'s `abort()` (`:122-133`, reached at
  `:353-354` whenever the third call does not return `EOF` — including
  the matching failure that leaves its `str` unwritten). That is a
  class-(c) site: the parse answers
  `Err(UserConfError::UnknownDatabaseFormat)`, the store does not open
  (`OpenError::UnknownDatabaseFormat`), `pinyin_init`/`zhuyin_init`
  answer NULL, and nothing is cleaned or written. One GLib warning per
  attempt: `check_format: unknown database format in user.conf`.
* **An empty user directory is the working directory, and only NULL is
  no user directory.** Upstream keeps `g_strdup(userdir)`
  (`pinyin.cpp:332`, `zhuyin.cpp:276`), tests only the pointer
  (`pinyin.cpp:1133`, `:2671`; `zhuyin.cpp:548`, `:1697`) and builds each
  path with `g_build_filename`, which drops an empty element. A `""` user
  directory is a profile of bare file names, resolved against the
  directory current at each file operation, so a consumer that changes
  directory after init saves into the new one. The two stay apart from
  `pinyin_init`/`zhuyin_init` down (`ContextCore::try_open` takes
  `Option<&str>`). `Runtime::open` opens the store on an empty path and
  keeps it relative, and the DBM sidecar cleanup unlinks its two names by
  path instead of walking the directory, which an empty path cannot open.

Verification: unit goldens and round-trips at every layer (codecs,
persistence, bridge, end-to-end), and the handoff tests over the
pin-written profiles in each C crate's `testdata/pin-profiles.txt`
([contract references](../runbooks/contract-references.md)). Recorded
evidence (2026-09-13) for the round-trip claim: the pin trained a profile
on the inputs `nihao nisha`, oxpinyin opened it through the production
`Runtime` and saved it back in place, and the pin then rendered the
original and the rewritten profile.

| pair | Phase C — the pin re-renders | Phase D — the pin reads |
|---|---|---|
| oxpinyin ↔ libpinyin, **tkrzw** | **IDENTICAL**, 3/3 export rows byte-identical | **READABLE**, 2 phrase rows, all expected |
| oxpinyin ↔ libpinyin, **Kyoto Cabinet** | **IDENTICAL**, 3/3 export rows byte-identical | **READABLE**, 2 phrase rows, all expected |
| oxpinyin ↔ libpinyin, **Berkeley DB** | **IDENTICAL**, 3/3 export rows byte-identical | **READABLE**, 2 phrase rows, all expected |

The three rows the pin renders are `P 你好 ni'hao 5`, `P 你沙 ni'sha 5`
and `B 你好 ni'hao 138`. The driver ran ibus's `remember-every-input`
path (`pinyin_get_sentence` → `pinyin_remember_user_input`) after each
train, so `user.bin` and the two index trees carried real content.

Rule for harnesses on this surface: diff one renderer over one dataset,
or diff the raw containers. Never assert render-against-render across two
trainers or two renderers that carry registered divergences (the bigram
export's stale pinyin buffer, policy row 1; n-best training, row 11).
Never verify a per-backend claim on one backend.

### NULL directory sessions (2026-10-08 UTC, #642)

NULL retains an independent in-memory user index and native bigram store.
It has no directory, unlike the non-NULL empty path (cwd at each operation).
Import, lookup, mask, remember and frequency changes use session state;
train and save refuse before mutation or persistence. Pinyin emits the
pin's failed empty-filename marker diagnostics at init/fini without writing
files (`UserStore::open_transient`).
