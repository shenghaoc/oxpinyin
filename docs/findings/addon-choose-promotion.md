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

The default facade's writable, persisted phrase index is the redb user store
(`oxpinyin-user`); nibbles 6/7 already live there and `addon.bin` (nibble 5) is
a `USER_FILE` of the same facade, so promotion writes there too:

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
ahead of 6/7, matching `_append_items`) as a `NORMAL_CANDIDATE`.

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
