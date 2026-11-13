# Addon choose-promotion (default nibble 5)

Status: **reference**

This record characterises the promotion that the pin's
`pinyin_choose_candidate` performs for an `ADDON_CANDIDATE`, and how
oxpinyin reproduces it. Companion records:
[`phrase-union.md`](phrase-union.md) (the union surface) and
[`user-store.md`](user-store.md) §2.2. Pin lines are at libpinyin
`074a2219` (`tools/oracle/oracle-pin.txt`).

## 1. Upstream path

`pinyin_choose_candidate` (`src/pinyin.cpp:2501-2589`). The `ADDON_CANDIDATE`
branch is `:2534-2563`:

```cpp
if (ADDON_CANDIDATE == candidate->m_candidate_type) {
    PhraseItem item;
    context->m_addon_phrase_index->get_phrase_item(candidate->m_token, item);   // :2536-2537

    guint8 len   = item.get_phrase_length();      // :2539
    guint8 npron = item.get_n_pronunciation();    // :2540

    PhraseIndexRange range;
    context->m_phrase_index->get_range(ADDON_DICTIONARY, range);                 // :2543
    phrase_token_t token = range.m_range_end;      // fresh id in default nibble 5 :2545

    for (size_t i = 0; i < npron; ++i) {           // :2548
        ChewingKey keys[MAX_PHRASE_LENGTH]; guint32 freq = 0;
        item.get_nth_pronunciation(i, keys, freq);
        context->m_pinyin_table->add_index(len, keys, token);                    // :2552
    }
    ucs4_t phrase[MAX_PHRASE_LENGTH];
    item.get_phrase_string(phrase);
    context->m_phrase_table->add_index(len, phrase, token);                      // :2557
    context->m_phrase_index->add_phrase_item(token, &item);                      // :2558

    candidate->m_candidate_type = NORMAL_CANDIDATE;   // :2561
    candidate->m_token = token;                        // :2562
}
```

Then the branch **falls through** to the normal tail:
`add_constraint(candidate->m_begin, candidate->m_end, token)` with the *new*
token (`:2578-2588`), or, under `SORT_WITHOUT_SENTENCE_CANDIDATE`, the
unigram-only training of the new token (`:2565-2576`). So the same call
writes the phrase into the default facade and records the constraint under
the promoted token, and a later `pinyin_train` trains the promoted (nibble 5)
token, not the addon-facade one.

Facts fixed by this path:

- **The target sub-index is `ADDON_DICTIONARY = 5`** (`novel_types.h:159`), a
  `USER_FILE` sub-index of the *default* facade `m_phrase_index` (`addon.bin`,
  [`phrase-union.md`](phrase-union.md) §3.2). The addon *candidate* lives in
  the separate, empty-by-default `m_addon_phrase_index`; promotion copies it
  into default nibble 5.
- **The fresh token is `range.m_range_end`**: the same "max id in the
  sub-index + 1" allocation the user and network sub-indexes use. The first id
  in an empty nibble is `PHRASE_INDEX_MAKE_TOKEN(5, 1)`.
- **The whole item is copied**: every pronunciation (`npron` readings, each
  with its own count) and the item's unigram frequency (`add_phrase_item`), not
  a `count * 3` seed as in `_add_phrase`.
- **The candidate is rewritten** to `NORMAL_CANDIDATE` at the new token.

## 2. oxpinyin mapping

The writable, persisted phrase index of the default facade is the user
store (`oxpinyin-user`). Nibbles 6 and 7 live there, and `addon.bin`
(nibble 5) is a `USER_FILE` of the same facade, so promotion writes there
too:

- `UserStore::promote_addon_phrase(text, readings, unigram)`
  (`crates/oxpinyin-user/src/store.rs`) allocates the next nibble-5 token,
  writes the phrase text and each reading's key sequence and count, sets the
  token's unigram to the copied item frequency and adds it to the unigram
  total, in one write transaction, the same shape as `add_phrase_in`.
- `SharedDict::addon_phrase_item(token)`
  (`crates/oxpinyin-capi/src/state.rs`) reads the addon dictionary the
  candidate came from (text, pronunciations, unigram) and converts each
  pronunciation spelling back to `SyllableKey` ids.
- `pinyin_choose_candidate` (`crates/oxpinyin-capi/src/candidates.rs`) runs
  the promotion for an `ADDON` candidate before selecting, rewrites the
  snapshot candidate to `NORMAL_CANDIDATE` at the promoted token, and selects
  the row with that token through `Session::select_target`
  (`crates/oxpinyin-engine/src/session/selection.rs`); a later choose of the
  same handle forces it again.

After promotion the phrase surfaces through `UserLookup` as a
`NORMAL_CANDIDATE` row. `UserLookup` indexes every stored phrase regardless
of nibble, in ascending nibble then token order; nibble 5 sorts before 6 and
7, as in `_append_items`. The addon row of the same string stays in the
list and replaces it (§5).

## 3. Matching behaviour at the edges

- **Promotion without a user directory.** The pin promotes into the
  in-memory default facade with no user-directory check
  (`pinyin.cpp:2534-2563`). A NULL user directory gives oxpinyin a
  transient user store, and `arena_promote` promotes into it in memory.
- **Missing sub-index 5.** With a custom `table.conf` row 5 of `NOT_USED`,
  `get_range` returns `ERROR_NO_SUB_PHRASE_INDEX` and leaves the range
  unwritten (`phrase_index.cpp:610-613`). Promotion uses that undefined end
  (`pinyin.cpp:2541-2545`). Ordinary import reads its range only after
  `ERROR_OK` (`:585-607`). The stock row is `USER_FILE`
  (`data/table.conf.in:13`), so init loads it or creates it empty
  (`pinyin.cpp:303-321`). oxpinyin skips promotion when the range is absent
  and leaves the candidate as `ADDON_CANDIDATE` (register row 121).
- **Re-promotion duplicates.** The pin takes `range.m_range_end` as the new
  token on every choose (`:2543-2545`), so choosing the same addon phrase
  twice gives two identical nibble-5 items. `arena_promote` takes the
  library's range end in the same way and does not deduplicate by text.

## 4. Tests

- Promotion is gated on an `ADDON` candidate, which exists only once an
  addon library is loaded, so the empty-store, no-addon path does not change.
- `oxpinyin-capi`'s
  `union_e2e_tests::chosen_addon_candidate_is_promoted_into_default_nibble_5`
  loads addon library 4 and chooses the surfaced `ADDON_CANDIDATE`. It
  asserts that the snapshot candidate becomes a `NORMAL_CANDIDATE` at a
  nibble-5 token, that the phrase persists under that token in the user
  store, and that a freshly built `UserLookup` finds the promoted phrase as a
  default-facade candidate.
- `oxpinyin-user`'s `promote_addon_phrase_allocates_nibble_5_and_copies_frequency`
  promotes into a transient store, and a second promotion of the same
  phrase takes the next range end (§3).

## 5. The next candidate list: the string dedup (2026-10-19 UTC)

After a promotion, a new candidate list for the same input holds two rows with
the promoted string. One is the `NORMAL_CANDIDATE` row of the new nibble-5
token. The other is the `ADDON_CANDIDATE` row. The pin keeps the addon row.

- `pinyin_guess_candidates` appends the normal rows of a span and then the
  addon rows of the same span (`src/pinyin.cpp:2247`, `:2253`).
- `_compute_frequency_of_items` sets `m_freq` of an addon row from the total
  frequency of the addon phrase index (`:1828-1842`). It sets `m_freq` of a
  normal row from the total frequency of the whole phrase index
  (`:1855-1866`). The addon total is much smaller, so the addon row has the
  larger `m_freq`.
- `_remove_duplicated_items_by_phrase_string` sorts the indices by string with
  a stable sort. Of two rows with the same string, it keeps the row with the
  larger `m_freq`. On a tie it keeps the earlier row (`:2058-2145`, the
  comparison at `:2128-2138`). `zhuyin.cpp:1406-1416` has the same rule.

The sort word does not change the result. With `sort_option` 0 the normal row
is first in the list and the addon row still wins. With
`SORT_BY_FREQUENCY` set, the sort puts the addon row first.

The engine implements the rule in `dedup_ranked_by_text`
(`crates/oxpinyin-engine/src/session/mod.rs`). It runs on the sorted phrase
rows, where the true `m_freq` of each row is known. It includes the LONGER
row before this dedup. The sentence prepend then keeps an n-best row over
any phrase row with its text (`src/pinyin.cpp:2116-2125`).

The pin gives LONGER only a unigram term (`src/pinyin.cpp:1916-1927`). A
NORMAL row can have the same text through a divided reading and get a
larger frequency from DYNAMIC_ADJUST (`:1845-1866`). Both rows reach the
final dedup (`:2292-2300`). The LONGER row keeps its frequency until then.

The before-cursor path also keeps each slice's frequency when it joins the
list (`src/zhuyin.cpp:1607-1612`). The final dedup compares all slices
(`:1626-1630`, `:1406-1416`). A later slice can win when it has a different
previous token. On a frequency tie the earlier slice still wins. The
engine retains each slice's rank keys through that comparison.

The before-cursor case guesses `nianhao`, then parses `ni''hao`. Parsing
keeps the sentence result (`src/zhuyin.cpp:1017-1042`). That result gives
start 4 a predecessor, while separator starts 2 and 3 have none
(`:1188-1197`). The bigram makes start 4 win. The LONGER case imports one
text in libraries 6 and 7 with readings `xi'an` and `xian'ni`. Each new
item gets its own pinyin index entry (`src/pinyin.cpp:584-605`).

Tests: the pin cases `addon-promoted-listing-unsorted`,
`addon-promoted-listing-by-frequency`, `before-cursor-frequency-dedup`,
`longer-normal-frequency-dedup` and `longer-normal-disabled`, with their
`frequency-tie` variants; `oxpinyin-engine`'s
`session::tests::the_string_dedup_keeps_the_larger_m_freq_and_the_earlier_tie`
and `session::tests::a_longer_row_surfaces_when_bit_2_clear_and_vanishes_when_set`.
The final dedup also runs when sentence rows are suppressed.
