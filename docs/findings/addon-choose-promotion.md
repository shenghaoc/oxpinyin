# Addon choose-promotion (default nibble 5) — #105

Characterisation of the upstream promotion that `pinyin_choose_candidate`
performs for an `ADDON_CANDIDATE`, and how oxpinyin reproduces it. Companion
to `docs/findings/phrase-union.md` (W11 union surface) and
`docs/findings/user-store.md` §2.2.

## 1. Upstream path

`pinyin_choose_candidate` (`src/pinyin.cpp:2499-2587`). The `ADDON_CANDIDATE`
branch is `:2532-2561`:

```cpp
if (ADDON_CANDIDATE == candidate->m_candidate_type) {
    PhraseItem item;
    context->m_addon_phrase_index->get_phrase_item(candidate->m_token, item);   // :2534

    guint8 len   = item.get_phrase_length();      // :2537
    guint8 npron = item.get_n_pronunciation();    // :2538

    PhraseIndexRange range;
    context->m_phrase_index->get_range(ADDON_DICTIONARY, range);                 // :2541
    phrase_token_t token = range.m_range_end;      // fresh id in default nibble 5 :2543

    for (size_t i = 0; i < npron; ++i) {           // :2546
        ChewingKey keys[MAX_PHRASE_LENGTH]; guint32 freq = 0;
        item.get_nth_pronunciation(i, keys, freq);
        context->m_pinyin_table->add_index(len, keys, token);                    // :2550
    }
    ucs4_t phrase[MAX_PHRASE_LENGTH];
    item.get_phrase_string(phrase);
    context->m_phrase_table->add_index(len, phrase, token);                      // :2555
    context->m_phrase_index->add_phrase_item(token, &item);                      // :2556

    candidate->m_candidate_type = NORMAL_CANDIDATE;   // :2559
    candidate->m_token = token;                        // :2560
}
```

Then the branch **falls through** to the normal `NORMAL_CANDIDATE` tail:
`add_constraint(candidate->m_begin, candidate->m_end, token)` with the *new*
token (`:2576-2586`). So the same call both writes the phrase into the default
facade and records the constraint under the promoted token — and a later
`pinyin_train` trains the promoted (nibble 5) token, not the addon-facade one.

### Facts fixed by this path

- **Target sub-index is `ADDON_DICTIONARY = 5`** (`novel_types.h:159`), a
  `USER_FILE` sub-index of the *default* facade `m_phrase_index` (`addon.bin`,
  `docs/findings/phrase-union.md` §3.2). The addon *candidate* lives in the
  separate, empty-by-default `m_addon_phrase_index`; promotion copies it across
  into default nibble 5.
- **Fresh token = `range.m_range_end`** — the same "max id in the sub-index + 1"
  allocation the user/network sub-indexes use; the first id in an empty nibble
  is `PHRASE_INDEX_MAKE_TOKEN(5, 1)`.
- **The whole item is copied** — every pronunciation (`npron` readings, each
  with its own count) and the item's unigram frequency (`add_phrase_item`), not
  a `count * 3` seed like `_add_phrase`.
- **The candidate is rewritten** to `NORMAL_CANDIDATE` at the new token.

## 2. oxpinyin mapping

The default facade's writable, persisted phrase index is the user store
(`oxpinyin-user`), served by `oxpinyin_store::DefaultStore` — Kyoto Cabinet
under the workspace's default features, redb under
`--no-default-features --features redb` (as written; the default is
Berkeley DB since 2026-09-20 and redb was removed that day); nibbles 6/7 already live there and
`addon.bin` (nibble 5) is a `USER_FILE` of the same facade, so promotion
writes there too:

- `UserStore::promote_addon_phrase(text, readings, unigram)`
  (`crates/oxpinyin-user/src/store.rs`) allocates the next nibble-5 token, writes
  the phrase text, each reading's key sequence + count, sets the token unigram to
  the copied item frequency, and bumps the unigram total — one write txn, the
  same shape as `add_phrase_in`.
- `SharedDict::addon_phrase_item(token)`
  (`crates/oxpinyin-capi/src/state.rs`) reads the addon dictionary the candidate
  came from (text, pronunciations, unigram) and converts each pronunciation
  spelling back to `SyllableKey` ids.
- `pinyin_choose_candidate` (`crates/oxpinyin-capi/src/candidates.rs`) runs the
  promotion for an `ADDON` candidate before selecting, rewrites the snapshot
  candidate to `NORMAL_CANDIDATE` at the promoted token, and records the promoted
  token in the sentence via `Session::select_promoted`
  (`crates/oxpinyin-engine/src/session.rs`).

After promotion the phrase surfaces through `UserLookup` (which indexes every
stored phrase regardless of nibble, ascending nibble then token — nibble 5 sorts
ahead of 6/7, matching `_append_items`) as a `NORMAL_CANDIDATE` row. The
addon row of the same string stays in the list and replaces it (section 5).

## 3. Rust-mechanism divergences

Recorded here rather than in `upstream-divergences.md` because they are local to
this path; fold in on the next sweep.

- **Promotion needs a user store.** Upstream promotes into the in-memory default
  facade even without a user dir. oxpinyin has no separate in-memory default
  phrase index; with no user store the choose degrades to a plain select of the
  addon candidate (no promotion). Externally observable only in the (unusual)
  no-user-dir + loaded-addon configuration.
- **Re-promotion merges instead of duplicating.** Upstream allocates a fresh
  nibble-5 token on every choose, so choosing the same addon phrase twice yields
  two identical default-facade entries. oxpinyin dedupes by `(nibble 5, text)`
  like `add_phrase_in`, merging readings onto the existing token. The surfaced
  candidate is identical; only the internal token count differs.

## 4. Acceptance

- The empty-store / no-addon path is untouched (promotion is gated on an `ADDON`
  candidate, which only exists once an addon library is loaded), so the decode
  pins do not move.
- A focused C-ABI test loads addon library 4, chooses the surfaced
  `ADDON_CANDIDATE`, and asserts that the snapshot candidate becomes a
  `NORMAL_CANDIDATE` at a nibble-5 token, that the phrase persists under that
  token in the user store, and that a freshly built `UserLookup` finds the
  promoted phrase — i.e. it will surface as a default-facade candidate.

## 5. The next candidate list: the string dedup (2026-10-19 UTC)

After a promotion, a new candidate list for the same input holds two rows with
the promoted string. One is the `NORMAL_CANDIDATE` row of the new nibble-5
token. The other is the `ADDON_CANDIDATE` row. The pin keeps the addon row.
Pin source read: the checkout at `074a2219`.

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

The contract cases are `addon-promoted-listing-unsorted` and
`addon-promoted-listing-by-frequency` (issue 913). The second case is
a control: a keep-first dedup also gives the pin's row there.
The cases `before-cursor-frequency-dedup` and
`longer-normal-frequency-dedup` cover the combined slices and LONGER.
Each has a `frequency-tie` case with DYNAMIC_ADJUST clear. The before-cursor
tie is a control. The LONGER tie also needs the fix: the old path omitted
the final dedup when sentence rows were suppressed. `longer-normal-disabled`
is its control. All seven cases have tag 913. The LONGER cases also check
sentence precedence and the sentence-suppression bit.
