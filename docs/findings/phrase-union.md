# Phrase-index union at lookup

Date: 2026-08-17 · Status: **implemented** (W11)

This finding records what upstream unions at lookup (§3) and how oxpinyin
reproduces it (§4–§6). User, network and addon phrases surface in
candidates; `pinyin_load_addon_phrase_library` loads addon libraries; and
prediction (`pinyin_guess_predicted_candidates_with_punctuations`,
`pinyin_choose_predicted_candidate`) produces candidates.

## 1. Scope

The union covers the default facade (system, user, network and
choose-promoted addon phrases), the addon facade, and prediction. At zero
user data and zero loaded addons the union is the identity: the corpus
pins do not move.

## 2. Source set

- libpinyin: the pin (`074a2219`; `tools/oracle/oracle-pin.txt` is
  authoritative). Upstream paths below beginning `src/` are relative to its
  root. The line citations in §3 were first read at 2.11.91.
- oxpinyin: this repo. Paths beginning `crates/` are repo-relative.
- Related records: [`user-store.md`](user-store.md) (the user store and the
  additive merge), [`data-formats.md`](data-formats.md) (the system and
  addon data files), [`candidate-construction.md`](candidate-construction.md)
  (the frozen window-scan construction) and
  [`addon-choose-promotion.md`](addon-choose-promotion.md).

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

- `context->m_phrase_index` — the **default** space. It loads every default
  table that is not `NOT_USED` (`src/pinyin.cpp:374-391`;
  `_load_phrase_library` at `:237-323`): system `SYSTEM_FILE` libraries
  1..4 plus three `USER_FILE` sub-indexes from `table.conf` —
  `ADDON_DICTIONARY` (5, `addon.bin`), `NETWORK_DICTIONARY` (6,
  `network.bin`), and `USER_DICTIONARY` (7, `user.bin`)
  (`docs/findings/data-formats.md` §3.1). A missing/empty `*.bin` still
  creates the sub-index. Choosing an `ADDON_CANDIDATE` later *promotes*
  that phrase into default nibble 5 (`pinyin.cpp:2532-2561`), after which
  it is a `NORMAL_CANDIDATE` found through `m_pinyin_table`.
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
prediction prefixes, user-phrase allocation, addon promotion, and
mask/remove — not by normal `pinyin_guess_candidates` and not by
`_prepend_longer_candidates`.

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

The phrase table does **not** feed normal guessing. Longer candidates come
from a pinyin-table `search_suggestion_with_matrix`
(`pinyin.cpp:1870-1927`, `:1883-1884`); the phrase table is used by
prediction prefixes (§3.6), user-phrase allocation, addon choose-promotion
into nibble 5 (`:2555`), and mask/remove.

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
`_get_previous_token(instance, 0)`, defaulting to `sentence_start`. oxpinyin
stores this arithmetic in `UserStore::observe_predicted` (§6.5).

## 4. oxpinyin implementation map

### 4.1 Data layer

| Upstream | oxpinyin |
|---|---|
| Default facade: system pinyin and phrase DBMs, system chunk files | `SystemDictionary` (`crates/oxpinyin-data/src/dict.rs`) over `pinyin_index.bin`, `phrase_index.bin` and the system chunks |
| Addon facade: `addon_pinyin_index.bin`, `addon_phrase_index.bin`, addon chunks loaded per call | `AddonDictionary` (`crates/oxpinyin-data/src/dict.rs`), held by the runtime's `AddonSet` |
| Default facade total (system + user) and addon facade total | `BigramLanguageModel` unigram total; `addon_unigram_total` |

### 4.2 Engine

| Upstream | oxpinyin |
|---|---|
| Two `search_matrix` passes per window | `Dictionary::lookup` then `lookup_addon` per window (`crates/oxpinyin-engine/src/session/lookup.rs`) |
| `ADDON_CANDIDATE` type | `CandidateKind::Addon` (`crates/oxpinyin-engine/src/candidate.rs`) |
| Addon frequency: addon total, no bigram | `Session::candidate_frequencies` addon branch |

### 4.3 C ABI

| Upstream | oxpinyin |
|---|---|
| `lookup_candidate_type_t` per candidate | `CapiCandidate::candidate_type` (`crates/oxpinyin-capi/src/state.rs`) |
| `pinyin_load_addon_phrase_library` | `crates/oxpinyin-capi/src/config.rs`, `RuntimeDict::load_addon` |
| Prediction and punctuation prepend | `crates/oxpinyin-capi/src/predict.rs`, `crates/oxpinyin-facade/src/predict.rs` |

### 4.4 User store

| Upstream | oxpinyin |
|---|---|
| User, network and addon `USER_FILE` sub-indexes 7, 6 and 5 in the default facade | `oxpinyin_user` (`USER_DICTIONARY`, `NETWORK_DICTIONARY`, `ADDON_DICTIONARY` in `phrase.rs`) |
| User pinyin and phrase tables | `UserLookup` (`crates/oxpinyin-user/src/lookup.rs`) |
| Choose-promotion into nibble 5 | [`addon-choose-promotion.md`](addon-choose-promotion.md) |

## 5. Addon data

The runtime reads the addon data that libpinyin ships: the
`addon_pinyin_index.bin` and `addon_phrase_index.bin` DBMs, attached at
init, and one `MemoryChunk` file per addon library, mapped when
`pinyin_load_addon_phrase_library` loads it
([`data-formats.md`](data-formats.md) §1–§3). There is no conversion or
export step. `oxpinyin-utils`' `gen_binary_files` writes the same files
from the model's `.table` text.

## 6. Design

### 6.1 Union point

oxpinyin mirrors upstream's two-facade split:

- **Default union** = `SystemDictionary` + `UserLookup`, exposed as one
  `Dictionary` implementation (`RuntimeDict`, `crates/oxpinyin-runtime`). Its
  `lookup` returns tokens in ascending library-nibble order — system, then
  default `USER_FILE` entries 5, 6 and 7 — as upstream's append does. Its
  `phrase_prefix_exists` ORs the system probe and the user-file probe. At
  zero user-file data the second half is empty.
- **Addon set** = the `AddonDictionary` with its loaded libraries, kept
  separate and not folded into `lookup`, so an addon token never needs to be
  distinguished from system `MERGED_DICTIONARY` by nibble alone.

### 6.2 Data structures

- `UserLookup` (`crates/oxpinyin-user/src/lookup.rs`) borrows the user
  store's session tables: the user pinyin index (complete and incomplete
  key spaces) and the user phrase table. It holds no second copy.
- `AddonDictionary` holds the addon DBM pair and the loaded addon
  libraries. Loading an already loaded index answers `false`, as
  `_load_phrase_library` does (`get_range` already loaded).

### 6.3 Candidate collection

The `Dictionary` and `LanguageModel` seams carry defaulted addon methods
([`core-trait-seam.md`](core-trait-seam.md)). The shape:

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

In the real-frequency window scan, after the default lookup the engine
also calls `lookup_addon` on each complete key-path and tags those entries
`CandidateKind::Addon`; the widening probe ORs `phrase_prefix_exists_addon`.
Implementors without addon data return empty/false, so the no-addon path is
unchanged.

### 6.4 Scoring/ranking

- Default candidates keep the frozen ranking; the `UserCountDelta` overlay
  adds user unigram deltas into the system LM before the λ blend.
- Addon candidates use a separate denominator, through defaulted
  `LanguageModel` accessors:
  ```rust
  fn addon_unigram_freq(&self, token: &Self::Token) -> Result<Option<u64>, Self::Error> { Ok(None) }
  fn addon_unigram_total(&self) -> Result<Option<u64>, Self::Error> { Ok(None) }
  ```
  `RuntimeLm` supplies these from the loaded addon set. When a candidate is
  `Addon`, `Session::candidate_frequencies` uses the addon path (no bigram
  term, addon total denominator) rather than the system path.
- `CandidateKind` has `Addon`. The predicted candidate types live on the C
  ABI row (§6.6). The engine's sort/dedup is source-aware only where
  frequency comes from.

### 6.5 Prediction

Prediction lives at the facade and C ABI boundary rather than inside the
generic `Session`, because it needs the phrase table and the user bigram:

- `compute_prefixes` (`crates/oxpinyin-facade/src/predict.rs`) reproduces
  `_compute_prefixes`: a suffix search over the system and user phrase
  tables.
- `crates/oxpinyin-capi/src/predict.rs` reproduces
  `_compute_predicted_bigram_candidates` (with the copied `filter = 10`,
  `pinyin.cpp:2311`, `:2349-2350`) and
  `_compute_predicted_prefix_candidates` (`merged_suggestions`, over
  `search_suggestion`), then sort and dedup.
- `pinyin_guess_predicted_candidates_with_punctuations` prepends the
  punctuation candidates from `punct.bin` for each prefix token.
- `pinyin_choose_predicted_candidate` trains the store
  (`UserStore::observe_predicted`, `observe_predicted_prefix`).

### 6.6 C ABI candidate typing

`CapiCandidate` carries its `lookup_candidate_type_t` (a one-byte tag),
populated when the candidate is built, rather than inferring the type from
`CandidateKind` alone:

- default window-scan phrase → `NORMAL_CANDIDATE`;
- addon window-scan phrase → `ADDON_CANDIDATE`;
- predicted bigram/prefix/punctuation → the corresponding enum value;
- sentence/fallback → current mapping.

`pinyin_get_candidate_type` returns that tag directly. This keeps the
token-nibble ambiguity for addon index 4 inside the collection code and out of
the C ABI.

### 6.7 Addon data

The runtime reads the shipped addon files directly (§5). The system
directory's `table.conf` names the addon libraries; nothing is discovered
by scanning.

## 7. Complexity comparison vs upstream

| Concern | Upstream | oxpinyin |
|---|---|---|
| Lookup | Two facades × 16 possible sub-indexes, DBM pinyin-table lookups | The same two facades over the same DBMs; default `lookup` = system + user, addon pass = the addon facade |
| User phrase surface | User pinyin table + user sub-index inside default facade | The same tables in the user store's session state |
| Addon surface | Aggregate addon pinyin/phrase tables + per-library content-file sub-index | The same files |
| Candidate collection | Two `search_matrix` passes per window, range arrays per nibble | One default pass + one addon pass per window |
| User ranking | Shared unigram total over default facade | Additive count merge; the total includes the user delta |
| Addon ranking | Separate addon total, no bigram | `addon_unigram_*` path, separate total |
| Prediction | Phrase-table suffix/prefix search + user bigram walk + punctuation table | The same three inputs |

At zero user data and zero addons the addon and prediction paths are
empty/false, which is the identity property the pins require.

## 8. Verification

### 8.1 Pin identity

The empty-store profile stays bit-identical:

```text
top-1      10190
top-5-set  10190
prefix-10  98930 of 98930
absent     0
tie-swaps  0
```

Movement means a leak into existing behaviour and is a STOP. These pins
are the Class A comparator port's; `oxpinyin-runtime`'s `candidate_surface`
test asserts the candidate surface
([goldens and pins](../runbooks/goldens-and-pins.md)).

### 8.2 Differentials

Recorded evidence: import, addon-load and train-then-predict differentials
compared candidate lists in both engines. The train-then-predict profile
un-blinds the training path the corpus profile skips.

## 9. Decisions (Phase 0, 2026-08-17)

1. **Network index 6 is in scope** — the same default-facade mechanism as
   user. Default nibble 5 (`ADDON_DICTIONARY` / `addon.bin`) is the same
   `USER_FILE` slot and the choose-promotion target (`pinyin.cpp:2532-2561`;
   #105).
2. **Punctuation prediction** reads `punct.bin` (§6.5).
3. **Trait additions are defaulted methods**; other implementors compile
   unchanged.

The "unions up to 16 libraries by token nibble" claim is wrong: §3.2
shows two facades (`m_phrase_index` vs `m_addon_phrase_index`), not one
16-way union (`pinyin.cpp:432-435`).
