# Findings — frontend-called libpinyin ABI subset

Date: 2026-08-07 · Source tier: Architect capture; human freeze pending.

## Source identity

- Frontend caller: ibus-libpinyin tag `1.16.5`, commit
  `2d2cdac0187101aa0cd7ac06694a8340721ddfbb`.
- Backend API: libpinyin tag `2.11.91`, commit
  `0c5e80e1200f84fab185d1c5bde458b770a0636c`.
- This is the **reference freeze for reproducibility (upstream release state
  as of 2026-07-31)**.

## Method

1. Read tracked frontend source directly from tag `1.16.5` and extract unique
   `pinyin_*` identifiers without using local build output.
2. Read `src/libpinyin.ver` directly from backend tag `2.11.91` and extract
   its 79 declared `pinyin_*` exports.
3. Take the sorted intersection, mechanically excluding frontend-local names,
   types and variables.

The resulting Stage 1 C-ABI implementation queue contains **52 symbols**:

```text
pinyin_alloc_instance
pinyin_begin_add_phrases
pinyin_begin_get_bigram_phrases
pinyin_begin_get_phrases
pinyin_bigram_iterator_get_next_phrase
pinyin_bigram_iterator_has_next_phrase
pinyin_choose_candidate
pinyin_choose_predicted_candidate
pinyin_end_add_phrases
pinyin_end_get_bigram_phrases
pinyin_end_get_phrases
pinyin_fini
pinyin_free_instance
pinyin_get_candidate
pinyin_get_candidate_nbest_index
pinyin_get_candidate_string
pinyin_get_candidate_type
pinyin_get_character_offset
pinyin_get_chewing_auxiliary_text
pinyin_get_double_pinyin_auxiliary_text
pinyin_get_full_pinyin_auxiliary_text
pinyin_get_left_pinyin_offset
pinyin_get_n_candidate
pinyin_get_pinyin_key
pinyin_get_pinyin_key_rest
pinyin_get_pinyin_key_rest_positions
pinyin_get_pinyin_offset
pinyin_get_pinyin_string
pinyin_get_right_pinyin_offset
pinyin_get_sentence
pinyin_guess_candidates
pinyin_guess_predicted_candidates_with_punctuations
pinyin_guess_sentence
pinyin_in_chewing_keyboard
pinyin_init
pinyin_is_user_candidate
pinyin_iterator_add_phrase
pinyin_iterator_get_next_phrase
pinyin_iterator_has_next_phrase
pinyin_load_addon_phrase_library
pinyin_mask_out
pinyin_parse_more_chewings
pinyin_parse_more_double_pinyins
pinyin_parse_more_full_pinyins
pinyin_remember_user_input
pinyin_remove_user_candidate
pinyin_reset
pinyin_save
pinyin_set_double_pinyin_scheme
pinyin_set_options
pinyin_set_zhuyin_scheme
pinyin_train
```

## Boundary notes

This is the frontend-called subset, not a promise to clone all of libpinyin.
Symbols needed only by the differential harness may be added to
`pinyin-oracle` without expanding the supported `pinyin-capi` surface.
Every C-ABI symbol requires a dedicated task, a `// SAFETY:` argument for
each unsafe block, NULL/invalid-input coverage, and an oracle-backed
behavioural test before freeze.
