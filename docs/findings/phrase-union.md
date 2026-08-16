# Phrase-index union at lookup — W11 Phase 0 scope and proposed design

Date: 2026-08-17 · Status: **Phase 0 approved** · Workstream: W11 ·
Base: current main (`1c7cbd0`)

This finding is the mandatory scope-and-design gate for W11. It records what
upstream actually unions, maps oxpinyin's current single-system-index
assumptions, and proposes a design for the second PR in the stack. §9 records
the approved ground for that implementation PR. No runtime behaviour is
changed by this document.

## 1. Scope

W11 makes user, network, and addon phrases surface in candidates, makes
`pinyin_load_addon_phrase_library` real, and closes the prediction gap
(`pinyin_guess_predicted_candidates_with_punctuations`,
`pinyin_choose_predicted_candidate`). The corpus pins must not move: at zero
user data and zero loaded addons the union must be the identity, exactly as
the W6-T4 empty-store merge is.

## 2. Source set

- libpinyin pinned tag `2.11.91`, on disk at `/tmp/libpinyin-2.11.91`.
  Upstream paths below beginning `src/` are relative to that root.
- oxpinyin: this repo. Paths beginning `crates/` are repo-relative, valid
  against main `1c7cbd0`.
- Prior art that this finding leans on:
  `docs/findings/user-store.md` (W6 user store and the additive merge),
  `docs/findings/data-layer-export.md` (why the raw `pinyin_index.bin` is not
  the runtime schema),
  `docs/findings/data-formats.md` (raw addon/custom content formats),
  `docs/findings/candidate-construction.md` (the frozen window-scan
  construction).

## 3. Upstream behaviour (pinned)

### 3.1 Token layout and the per-nibble facade dispatch

`phrase_token_t` is a `u32`. The top nibble is the library index and the low
24 bits are the phrase id inside that library:

```c
#define PHRASE_INDEX_LIBRARY_MASK   0x0F000000
#define PHRASE_INDEX_LIBRARY_INDEX(token) ((token & PHRASE_INDEX_LIBRARY_MASK) >> 24)
#define PHRASE_INDEX_MAKE_TOKEN(phrase_index, token) \
    (((phrase_index << 24) & PHRASE_INDEX_LIBRARY_MASK) | (token & PHRASE_MASK))
```

(`src/include/novel_types.h:41-46`.) `PHRASE_INDEX_LIBRARY_COUNT` is `1 << 4`
(`:43`), and the documented library ids are `GB_DICTIONARY=1`,
`GBK_DICTIONARY=2`, `OPENGRAM_DICTIONARY=3`, `MERGED_DICTIONARY=4`,
`ADDON_DICTIONARY=5`, `NETWORK_DICTIONARY=6`, `USER_DICTIONARY=7`
(`novel_types.h:154-161`).

`FacadePhraseIndex` is not a merged dictionary; it is an array of at most 16
`SubPhraseIndex` pointers, dispatched by the token nibble
(`src/storage/phrase_index.h:438-442`). `add_unigram_frequency`,
`get_phrase_item`, `add_phrase_item`, and `remove_phrase_item` all extract the
nibble and delegate to `m_sub_phrase_indices[index]`
(`phrase_index.h:628-634`, `646-651`, `655-673`, `674-691`). The facade's
`m_total_freq` is the sum of the loaded sub-index totals, adjusted on every
add/remove/unigram change (`phrase_index.h:615-616`, `:631-634`, `:668-671`,
`:687-690`).

### 3.2 Init: two separate facades, not one 16-way union

`pinyin_init` creates two facade instances:

- `context->m_phrase_index` — the **default** space. It loads the default
  table list (system `SYSTEM_FILE` libraries 1..4 plus `USER_FILE` library 7)
  in a loop over `get_default_tables()`
  (`src/pinyin.cpp:374-391`; `_load_phrase_library` at `:237-323`).
  `NETWORK_DICTIONARY` (`6`) is also a `USER_FILE` entry in this same loop
  (`docs/findings/data-formats.md` §3.1), so when a non-empty `network.bin`
  exists it joins the default facade exactly like the user dictionary and its
  tokens surface as normal candidates.
- `context->m_addon_phrase_index` — the **addon** space. It is constructed
  empty and deliberately has no libraries loaded:
  `context->m_addon_phrase_index = new FacadePhraseIndex;` followed by
  `/* don't load addon phrase libraries. */` (`pinyin.cpp:432-435`).

There are also two pinyin tables and two phrase tables:

- `m_pinyin_table` = system pinyin index + user pinyin index
  (`pinyin.cpp:351-357`), feeding the default facade.
- `m_addon_pinyin_table` = `addon_pinyin_index.bin` only (`pinyin.cpp:417-422`).
- `m_phrase_table` = system phrase table + user phrase table
  (`pinyin.cpp:359-369`).
- `m_addon_phrase_table` = `addon_phrase_index.bin` only (`pinyin.cpp:424-430`).

The pinyin table and phrase table are **separate structures from the phrase
index**. Candidate guessing searches the pinyin table and then resolves token
text/frequency through the phrase index (§3.3). The phrase table is used by
longer-candidate and prediction paths, not by normal `pinyin_guess_candidates`.

### 3.3 How `pinyin_guess_candidates` enumerates candidates

`pinyin_guess_candidates` (`src/pinyin.cpp:2182-2306`) runs the same
expanding-window scan oxpinyin reproduces, but against both facades:

1. Prepare `ranges` for `m_phrase_index` and `addon_ranges` for
   `m_addon_phrase_index` (`:2218-2222`). `prepare_ranges` creates one
   `GArray` for each **loaded** sub-index only
   (`src/storage/phrase_index.h:703-727`).
2. For each `[start,end)` window, search the default pinyin table into
   `ranges` (`search_matrix(context->m_pinyin_table, …)`, `pinyin.cpp:2231`)
   and the addon pinyin table into `addon_ranges`
   (`search_matrix(context->m_addon_pinyin_table, …)`, `:2235`). The search
   result bits are OR'd.
3. Append default ranges as `NORMAL_CANDIDATE`
   (`_append_items(ranges, &template_item, candidates)`, `:2245`) and addon
   ranges as `ADDON_CANDIDATE`
   (`_append_items(addon_ranges, &addon_template_item, candidates)`, `:2251`).

`_append_items` (`pinyin.cpp:1769-1792`) walks the 16 range arrays in
ascending library-nibble order, then each contiguous token range. Therefore:

- system tokens enter as normal candidates from the default facade;
- **user tokens enter as normal candidates through the same default facade**
  (they live in `m_phrase_index` sub-index 7 and are found through
  `m_pinyin_table`, which loaded the user pinyin table);
- addon tokens enter only through the separate addon facade and are tagged
  `ADDON_CANDIDATE`, never normal.

`search_matrix` itself is the recursive key-matrix walk in
`src/storage/phonetic_key_matrix.cpp:410-438`; it calls the pinyin table's
`search`, which fills `PhraseIndexRanges` per token nibble
(`src/storage/chewing_large_table2.h:104-151`).

The phrase table does **not** feed normal guessing. It feeds
`_prepend_longer_candidates` (`search_suggestion_with_matrix`,
`pinyin.cpp:1870-1927`), prediction prefixes (§3.6), user-phrase allocation,
and masking/removal.

### 3.4 Candidate ranking by source

After collection, `_compute_phrase_length` and
`_compute_phrase_strings_of_items` resolve each candidate through the facade
that owns its type: normal/predicted tokens through `m_phrase_index`, addon
tokens through `m_addon_phrase_index` (`pinyin.cpp:1953-2039`).

`_compute_frequency_of_items` (`pinyin.cpp:1794-1870`) applies **different
denominators and bigram participation**:

- Normal and predicted-bigram candidates:
  `freq = (λ · bigram_poss · BIGRAM_FREQUENCY_DISCOUNT +
           (1 − λ) · unigram_freq / phrase_index_total_freq) · 2^24`
  (`:1854-1870`). `phrase_index_total_freq` is `m_phrase_index`'s total, which
  includes the user sub-index. A user phrase therefore participates in the
  same pool as system phrases: its `unigram_freq` is the numerator and it also
  increases the shared denominator. With `DYNAMIC_ADJUST` off (or no previous
  token), `bigram_poss = 0`, so ranking is pure unigram over the summed total.
- Addon candidates:
  `freq = (1 − λ) · unigram_freq / addon_phrase_index_total_freq · 2^24`
  (`:1823-1842`), using **the addon facade's own total** and no bigram term.
- Predicted-prefix candidates:
  `freq = (1 − λ) · unigram_freq / phrase_index_total_freq · 2^24`
  (`:1814-1822`).

Sort keys are `SORT_BY_PHRASE_LENGTH | SORT_BY_PINYIN_LENGTH |
SORT_BY_FREQUENCY`, all descending, applied by
`compare_item_with_sort_option` (`pinyin.cpp:1678-1709`).

### 3.5 Addon library loading

`pinyin_load_addon_phrase_library(context, index)`:

```c
const pinyin_table_info_t *phrase_files = context->m_system_table_info.get_addon_tables();
FacadePhraseIndex *phrase_index = context->m_addon_phrase_index;
const pinyin_table_info_t *table_info = phrase_files + index;
if (NOT_USED == table_info->m_file_type) return false;
assert(DICTIONARY == table_info->m_file_type);
return _load_phrase_library(system_dir, user_dir, phrase_index, table_info);
```

(`src/pinyin.cpp:477-501`.) `_load_phrase_library` for `DICTIONARY`
(`pinyin.cpp:280-301`) loads the addon's **content file** (for example
`art.bin`) into the addon facade's sub-index `index`. It does not load a
separate pinyin table per call: the aggregate `m_addon_pinyin_table` and
`m_addon_phrase_table` were already loaded once at init. Loading the same
index twice returns `false` because `get_range` already sees it
(`_load_phrase_library` `:243-246`).

The addon index space is `4..15` in the *addon* facade, named by numeric
config entries `addon 4 art.table art.bin NULL DICTIONARY`, … `addon 15
technology…` (`docs/findings/data-formats.md` §3.2). Index 4 is therefore
overloaded: `MERGED_DICTIONARY` in the default space and `art` in the addon
space; the two facades keep them distinct.

`pinyin_unload_addon_phrase_library` unloads the corresponding sub-index
(`pinyin.cpp:503-508`).

### 3.6 Prediction enumeration and choose

Prediction has three independent inputs:

1. `_compute_prefixes(instance, prefix)` (`pinyin.cpp:1389-1424`) converts the
   prefix to UCS-4, then for each suffix length `i = 1..min(prefix_len,
   MAX_PHRASE_LENGTH)` calls `m_phrase_table->search(i, last_i_chars, tokens)`
   and appends the resulting tokens to `instance->m_prefixes`. This is a
   phrase-table lookup, not a pinyin-table lookup.
2. `_compute_predicted_bigram_candidates(instance, &merged_gram)`
   (`pinyin.cpp:2309-2368`) walks `m_prefixes` backwards, loads **only the
   user bigram** for each prefix token into `merged_gram`, and stops at the
   first prefix that has one. It retrieves all successor `(token,count)` rows,
   keeps those with `m_count >= filter` where `const guint32 filter = 10`
   (`pinyin.cpp:2311`, skip at `:2349-2350`), and appends successors whose
   phrase length is 2 then 1 as `PREDICTED_BIGRAM_CANDIDATE`, resolving
   phrase length through `m_phrase_index`.
3. `_compute_predicted_prefix_candidates(instance)` (`pinyin.cpp:2370-2409`)
   calls `m_phrase_table->search_suggestion(prefix_len, prefix_ucs4, tokens)`,
   then appends each token as `PREDICTED_PREFIX_CANDIDATE`, skipping phrases
   longer than `prefix_len * 2 + 1`.

`pinyin_guess_predicted_candidates` (`pinyin.cpp:2411-2451`) clears candidates,
recomputes prefixes, runs both, computes phrase lengths and frequencies with a
null previous token and the (empty after prefix-walk) merged gram, sorts by
`SORT_BY_PHRASE_LENGTH | SORT_BY_FREQUENCY`, computes strings, and dedups.
`pinyin_guess_predicted_candidates_with_punctuations` (`:2454-2498`) delegates
to it and prepends punctuation candidates from `m_system_punct_table` for each
prefix token.

`pinyin_choose_predicted_candidate` (`pinyin.cpp:2589-2639`) applies a **flat**
`initial_seed = 69` to the chosen token's unigram (`+69 * 7 = 483`) and to the
user bigram `(last → token)` plus `last`'s total (`+69`). `last` is
`_get_previous_token(instance, 0)`, defaulting to `sentence_start`. W6 already
stores this arithmetic (`oxpinyin_user::observe_predicted`), but nothing
currently produces a predicted candidate for the call to consume.

## 4. oxpinyin current single-index assumptions — the gap map

### 4.1 Data layer

| Site | Current assumption | Upstream equivalent |
|---|---|---|
| `crates/oxpinyin-data/src/dict.rs` `SystemDictionary::lookup` | One `pinyin_index.redb` / `phrase_index.redb` pair; every token resolved through that one phrase index | Default facade + addon facade, two pinyin tables and two phrase indexes |
| `SystemDictionary::phrase_prefix_exists` | Prefix probe only over the system pinyin key lists | Prefix probe over system + user pinyin tables, and separately addon pinyin table |
| `SystemDictionary` unigram map | Aggregated system export frequencies only | Default facade total (system + user deltas) and addon facade total |
| `crates/oxpinyin-data/src/content.rs` `ContentTable` | Parses addon `.bin` records, but has no runtime loader and no text/pinyin resolution | `SubPhraseIndex::load` for a `DICTIONARY` addon sub-index |
| `crates/oxpinyin-data/src/lm.rs` `BigramLanguageModel` | `interpolation2.text` real unigrams are the system phrase-index counts only | System phrase-index counts plus user deltas for the default pool; addon pool is separate |

### 4.2 Engine

| Site | Current assumption | Gap |
|---|---|---|
| `crates/oxpinyin-engine/src/session.rs` `refresh` / `collect_window_scan` | Calls only `dictionary.lookup` and `dictionary.phrase_prefix_exists` | No second addon pass, no user-dictionary union in `D`, no source tagging |
| `Session::candidate_frequencies` | Reads only `LanguageModel::unigram_freq` | No addon-frequency path or addon-total denominator |
| `crates/oxpinyin-engine/src/candidate.rs` `CandidateKind` | `Phrase`, `Sentence`, `Fallback` only | Upstream additionally distinguishes `ADDON_CANDIDATE`, `PREDICTED_*`, `LONGER_CANDIDATE`, `ZOMBIE_CANDIDATE` |

### 4.3 C ABI

| Site | Current assumption | Gap |
|---|---|---|
| `crates/oxpinyin-capi/src/state.rs` `SharedDict` | Thin `Arc<SystemDictionary>` | No user lookup, no addon set |
| `SharedLm` | User-count overlay on the system LM only | No addon total/ranking path |
| `CapiCandidate` / `pinyin_get_candidate_type` | Maps `Phrase → NORMAL_CANDIDATE`, `Sentence → NBEST_MATCH_CANDIDATE` | Cannot report `ADDON_CANDIDATE` or `PREDICTED_*` |
| `crates/oxpinyin-capi/src/config.rs` `pinyin_load_addon_phrase_library` | Always `false` | Needs real addon sub-index loading |
| `crates/oxpinyin-capi/src/sentence.rs` predicted entry points | Always `false` | Needs prediction enumeration |
| `crates/oxpinyin-capi/src/candidates.rs` `pinyin_choose_predicted_candidate` | Correct store write, but no predicted candidate is ever generated | Needs predicted candidates to surface first |

### 4.4 User store

| Site | Current assumption | Gap |
|---|---|---|
| `crates/oxpinyin-user/src/store.rs` | Stores user phrase text, token, pronunciation, counts | No exact `SyllableKey[] → phrases` lookup, no incomplete/prefix probe, no phrase-text prefix/suffix search |
| `crates/oxpinyin-user/src/phrase.rs` | Has nibble helpers for `USER_DICTIONARY` only | Does not model addon tokens; addon ambiguity (§3.5) must live outside the token nibble |
| `crates/oxpinyin-capi/src/iterators.rs` export | Only `USER_DICTIONARY` exports; system/addon exports are empty | Acceptable for the user export ABI; not a candidate-surface blocker |

There is also no source for `NETWORK_DICTIONARY` (`6`) at all. Upstream treats
it as a second `USER_FILE` sub-index in the default facade; oxpinyin has no
network-file store, so restoring a non-empty `network.txt`/`network.bin` has
nothing to load. See §9.

## 5. Addon data availability — a load-bearing decision

The prompt says to make `pinyin_load_addon_phrase_library` real "against the
existing `addon_*.redb` (W3 already converts them)". Those W3 files are
`oxpinyin-migrate convert` passthroughs of the raw Tkrzw files, i.e. the
same **undocumented sectioned binary format** that
`docs/findings/data-layer-export.md` deliberately refused to parse for the
system pinyin index. Concretely:

- `fixtures/w3/addon_pinyin_index.redb` has 117 records, all with 6-byte keys
  of the `00 00 00 00 [u16 BE]` family, and sectioned values — not the
  string-keyed `pinyin_index.redb` schema `SystemDictionary` reads.
- `fixtures/w3/addon_phrase_index.redb` has 68 records, also 6-byte keys and
  sectioned values — not the token→UTF-8 schema `SystemDictionary` reads.

Therefore "use the existing addon_*.redb" can mean one of two very different
implementations:

**Option A — regenerate the addon tables in the established public-ABI schema.**
Add a data-preparation step (in `oxpinyin-migrate`, analogous to
`docs/findings/data-layer-export.md`) that reads the model archive's addon
`.table` text files and writes string-keyed pinyin indexes plus token→text
phrase indexes, one per addon library. Runtime `pinyin_load_addon_phrase_library`
then opens those files exactly like `SystemDictionary` opens the system pair.
This is the low-risk path, reuses the existing loader, and keeps the raw
sectioned format out of the hot path. It changes the provenance of the
`addon_*.redb` fixtures from raw Tkrzw copies to schema exports (flagged
below). W3's `ContentTable` remains useful as an independent parser/validator
for the `.bin` records.

**Option B — port a raw binary decoder.** Read upstream C++ to reconstruct the
sectioned `pinyin_index.bin`/`phrase_index.bin` format and implement it for the
raw W3 addon tables. This is exactly the path `data-layer-export.md` rejected
for system tables after measuring that the raw key space does not match
`ChewingKey::get_table_index()` and the value layout is a sectioned
multi-record format. It is much larger and is not required by any runtime
contract.

**Recommendation: Option A.** It follows the project's own precedent for system
tables, avoids reading a legacy format the public-ABI export already
side-stepped, and keeps runtime complexity additive rather than replacing a
known-good loader. It does require a maintainer decision because the W11
prompt names the W3 files; that decision is the first stop point of this
Phase 0.

## 6. Proposed design (Option A)

### 6.1 Union point

Mirror upstream's two-facade split without changing the frozen `Session<D, L>`
signature:

- **Default union** = `SystemDictionary` + `UserLookup`, exposed as one
  `Dictionary` implementation. Its `lookup` returns system entries first, then
  user entries, matching upstream's ascending library-nibble append order
  (system sub-indexes 1..4 before user sub-index 7). Its
  `phrase_prefix_exists` ORs the system probe and the user probe. At zero user
  data the user half is empty, so this is bit-identical to today.
- **Addon set** = one `AddonDictionary` per loaded library, kept separate and
  not folded into `lookup`, so an addon token never needs to be distinguished
  from system `MERGED_DICTIONARY` by nibble alone.

### 6.2 Data structures

`UserLookup` (in `oxpinyin-user` or a small capi-side wrapper):

- exact map: `BTreeMap<String, Vec<(u32 token, String text)>>` keyed by
  `'`-joined `SyllableKey` spellings, rebuilt lazily from the user phrase
  pronunciation table when the store changes;
- incomplete/prefix set: the same keys projected through
  `SystemDictionary::initial_key` logic (or a shared helper), so
  `phrase_prefix_exists` matches the system incomplete-index probe;
- phrase-text suffix/prefix maps for prediction: `BTreeMap<String, Vec<u32>>`
  over user phrase text.

`AddonDictionary`:

- generated from Option A as a pinyin-keyed table and a token→text table, with
  a per-library `unigrams: BTreeMap<u32,u64>` and `unigram_total: u64`;
- one instance per loaded addon index; `lookup` and `phrase_prefix_exists`
  reuse the `SystemDictionary` implementation shape;
- loading/unloading idempotence follows `_load_phrase_library`
  (`get_range` already loaded → `false`).

### 6.3 Candidate collection

Add defaulted methods to the `Dictionary` and `LanguageModel` seams
(`docs/findings/core-trait-seam.md`: the seam grows by defaulted methods only):

```rust
trait Dictionary {
    // existing methods ...

    /// Addon lookup pass, empty for every existing implementor.
    fn lookup_addon(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        Ok(Vec::new())
    }

    /// Addon continued-search probe, false for every existing implementor.
    fn phrase_prefix_exists_addon(&self, syllables: &[Self::Syllable]) -> Result<bool, Self::Error> {
        Ok(false)
    }
}
```

In the real-frequency window scan, after the existing default lookup the engine
also calls `lookup_addon` on each complete key-path and tags those entries
`CandidateKind::Addon`; the widening probe ORs `phrase_prefix_exists_addon`.
Existing implementors return empty/false, so the no-addon path is unchanged.

### 6.4 Scoring/ranking

- Default candidates keep the current frozen ranking; the W6-T4
  `UserCountDelta` overlay already adds user unigram deltas into the system LM
  before the λ blend.
- Addon candidates need a separate denominator. Add defaulted
  `LanguageModel` accessors:
  ```rust
  fn addon_unigram_freq(&self, token: &Self::Token) -> Result<Option<u64>, Self::Error> { Ok(None) }
  fn addon_unigram_total(&self) -> Result<Option<u64>, Self::Error> { Ok(None) }
  ```
  `SharedLm` supplies these from the loaded addon set. When a candidate is
  `Addon`, `Session::candidate_frequencies` uses the addon path (no bigram
  term, addon total denominator) rather than the system path.
- `CandidateKind` grows `Addon`, `PredictedBigram`, `PredictedPrefix`,
  `PredictedPunctuation` (and, if needed for the C ABI mapping, `Longer`).
  The engine's existing sort/dedup is source-aware only where frequency comes
  from.

### 6.5 Prediction

Prediction is implemented at the data/capi boundary rather than inside the
generic `Session`, because it needs concrete phrase-table and user-bigram
surfaces that `Session` deliberately does not model:

- Add reverse phrase lookup methods to `SystemDictionary` and `UserLookup`:
  suffix search (for `_compute_prefixes`) and prefix suggestion (for
  `_compute_predicted_prefix_candidates`).
- Add a `UserBigram` successor iterator to `UserStore` (or expose the existing
  bigram table read) with the copied `filter = 10` skip
  (`pinyin.cpp:2311`, `:2349-2350`).
- `SharedLm`/`SharedDict` expose a `predict(prefix)` method that reproduces
  `_compute_prefixes` → `_compute_predicted_bigram_candidates` →
  `_compute_predicted_prefix_candidates` → sort/dedup.
- `pinyin_guess_predicted_candidates` snapshots those results into
  `inst.candidates` with the correct `lookup_candidate_type_t`.
- `pinyin_guess_predicted_candidates_with_punctuations` prepends punctuation
  candidates from `punct.redb` (a W11 sub-task if punctuation is in scope for
  the fork call surface; otherwise the punctuation list is empty and the
  function still returns the non-punctuation prediction list).
- `pinyin_choose_predicted_candidate` is already correct on the store side and
  becomes reachable.

### 6.6 C ABI candidate typing

Rather than infer candidate type from `CandidateKind` alone, add a
`candidate_type: lookup_candidate_type_t` field to `CapiCandidate`, populated
when the candidate is built:

- default window-scan phrase → `NORMAL_CANDIDATE`;
- addon window-scan phrase → `ADDON_CANDIDATE`;
- predicted bigram/prefix/punctuation → the corresponding enum value;
- sentence/fallback → current mapping.

`pinyin_get_candidate_type` returns that field directly. This keeps the
token-nibble ambiguity for addon index 4 inside the collection code and out of
the C ABI.

### 6.7 Addon data preparation (Option A implementation shape)

- Add an addon-export command to `oxpinyin-migrate` that reads
  `data/*.table` from the model archive and writes one string-keyed
  `pinyin_index` plus one `token→text` phrase index per addon library, with
  counts taken from the `.table` count column.
- Regenerate `fixtures/w3/addon_*.redb` accordingly, keeping the system
  fixtures and all corpus pins untouched.
- Runtime `CapiContext` discovers addon table paths from a small manifest next
  to the tables (or a fixed naming convention), never by scanning.

This is the only part of the design that touches `oxpinyin-migrate`, which is
outside W11's stated primary ground; it is flagged below.

## 7. Complexity comparison vs upstream

| Concern | Upstream | Proposed oxpinyin |
|---|---|---|
| Lookup | Two facades × 16 possible sub-indexes, DBM pinyin-table lookups | Two logical unions; default `lookup` = system map + user map, addon pass = one map per loaded library |
| User phrase surface | User pinyin table + user sub-index inside default facade | In-memory exact + initial-key maps over user pronunciations |
| Addon surface | Aggregate addon pinyin/phrase tables + per-library content-file sub-index | One generated pinyin/phrase table per loaded addon library |
| Candidate collection | Two `search_matrix` passes per window, range arrays per nibble | One existing default pass + one defaulted addon pass per window |
| User ranking | Shared unigram total over default facade | Existing W6-T4 additive count merge; total already includes user delta |
| Addon ranking | Separate addon total, no bigram | Defaulted `addon_unigram_*` path, separate total |
| Prediction | Phrase-table suffix/prefix search + user bigram walk + punctuation table | In-memory reverse maps + user bigram iterator + optional punctuation list |

No step worsens both time and space. The addon and prediction paths are
additive; at zero user data and zero addons they are empty/false, which is the
identity property the pins require. The main new memory is the user reverse
index and per-loaded-addon tables, both proportional to loaded data rather
than to the system index.

## 8. Verification plan

### 8.1 Pin identity

Before any feature is active, the empty-store profile must remain bit-identical:

```text
top-1      10177
top-5-set  10189
prefix-10  94871 of 98930
absent     1
tie-swaps  1036
```

Movement means a leak into existing behaviour and is a STOP.

### 8.2 New differentials

Unique names under `tools/bisection/`, not edits to `run-import-diff.sh` or
`run-train-diff.sh`:

- import a user phrase through the add/iterator/end trio in both engines,
  re-parse, and assert the phrase appears at the same candidate position;
- load the same addon library in both engines and compare candidate lists;
- train then predict in both engines and compare predicted lists. This
  un-blinds the training path the corpus profile skips; the profile used must
  be recorded in the report.

### 8.3 Existing gates

- import/train diffs remain IDENTICAL;
- bisect and valgrind remain 0/0;
- C++ smoke, `fmt`, `clippy`, `test`.

## 9. Approved decisions (Phase 0)

1. **Addon data: Option A.** Regenerate from the `.table` text via
   `oxpinyin-migrate`. Conditions: (a) differential-verify the regenerated
   tables — pinned libpinyin loads the same `.table` through its own addon
   path, ADDON candidates compared exactly; (b) remove the raw W3
   `addon_*.redb` in the implementation PR unless a manifest/test still pins
   them — if pinned, keep and mark superseded instead of deleting.
2. **Network index 6 is in scope** for the implementation PR — same
   default-facade mechanism as user. Confirm from the pinned frontend which
   index its network import targets and reproduce exactly (W7-T1 made only
   index 7 writable; if extending writability to 6 is structurally more than
   a second nibble in the same path, report and it splits out).
3. **Punctuation prediction is conditional.** Check `punct.redb`
   consumability first. Consumable → include punctuation in the first
   prediction PR. Not → stub empty, run the prediction differential in
   non-punctuation mode, register the gap with a named follow-up. Phrase
   prediction is differential-exact in the first PR either way.
4. **Trait additions are within W11 ground** (defaulted methods; other
   implementors compile unchanged). W10 owns `config.rs` / parser masking;
   W13 owns new parsers. Flag the seam additions in the implementation
   report.

The ROADMAP W11 "unions up to 16 libraries by token nibble" claim is
superseded by §3.2: two facades (`m_phrase_index` vs
`m_addon_phrase_index`), not one 16-way union (`pinyin.cpp:432-435`).

Expected fork-side consequence: the fork's parity gate can run with stock
`network.txt` only after the default facade has a `USER_FILE` source for index
6 as well as 7. The fork itself is not run here.
