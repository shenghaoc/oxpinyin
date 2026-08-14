# Findings — frontend-called libpinyin ABI subset

Date: 2026-08-14 · Source tier: Manual source read; human freeze pending.

## Source identity

- Frontend caller: **ibus-libpinyin** tag `1.16.5`, commit
  `2d2cdac0187101aa0cd7ac06694a8340721ddfbb`.
- Backend API: **libpinyin** tag `2.11.91`, commit
  `0c5e80e1200f84fab185d1c5bde458b770a0636c`.
- Header: `libpinyin/src/pinyin.h` (79 exported `pinyin_*` symbols in
  `libpinyin.ver`).
- This is the reference freeze for reproducibility (upstream release state
  as of 2026-07-31).

## Method

1. Cloned both repos at their pinned tags.
2. Read every `.cc` file under `ibus-libpinyin/src/` and extracted unique
   `pinyin_*` call-site identifiers from live (non-`#if 0`) code.
3. Intersected against the 79 declared exports in `libpinyin.ver`.
4. Per-symbol signatures copied from `pinyin.h`; ownership inferred from
   header declarations, consumer usage (free patterns), and doc comments.

The live consumer calls exactly **50 symbols** out of the 79 exported.

Earlier architect capture listed 52; the difference:
- `pinyin_get_pinyin_key` and `pinyin_get_pinyin_string` appear only inside
  a `#if 0` dead-code block at `PYPPinyinEditor.cc:296–333`.
- `pinyin_get_n_pinyin` also only appears in that same dead block.
- `pinyin_get_parsed_input_length` is not called anywhere in ibus-libpinyin
  (used only by `tools/capture/capture.c`).

The corrected count: **50 live symbols**.

---

## 1. Symbol list with call sites

### 1a. Context lifecycle (4 symbols)

| Symbol | Call sites |
|--------|-----------|
| `pinyin_init` | `PYLibPinyin.cc:186` |
| `pinyin_fini` | `PYLibPinyin.cc:224` |
| `pinyin_alloc_instance` | `PYLibPinyin.cc:234,258` |
| `pinyin_free_instance` | `PYLibPinyin.cc:281,296` |

### 1b. Configuration (5 symbols)

| Symbol | Call sites |
|--------|-----------|
| `pinyin_set_options` | `PYLibPinyin.cc:305` |
| `pinyin_set_double_pinyin_scheme` | `PYLibPinyin.cc:316` |
| `pinyin_set_zhuyin_scheme` | `PYLibPinyin.cc:325` |
| `pinyin_load_addon_phrase_library` | `PYLibPinyin.cc:339` |
| `pinyin_save` | `PYLibPinyin.cc:353` |

### 1c. Parsing (4 symbols)

| Symbol | Call sites |
|--------|-----------|
| `pinyin_parse_more_full_pinyins` | `PYPFullPinyinEditor.cc:37`, `PYLibPinyin.cc:443` |
| `pinyin_parse_more_double_pinyins` | `PYPDoublePinyinEditor.cc:37` |
| `pinyin_parse_more_chewings` | `PYPBopomofoEditor.cc:305,311` |
| `pinyin_in_chewing_keyboard` | `PYPBopomofoEditor.cc:180,330` |

### 1d. Sentence / guess (4 symbols)

| Symbol | Call sites |
|--------|-----------|
| `pinyin_guess_sentence` | `PYPFullPinyinEditor.cc:39`, `PYPDoublePinyinEditor.cc:39`, `PYPBopomofoEditor.cc:306,312`, `PYPPhoneticEditor.cc:497` |
| `pinyin_guess_candidates` | `PYPPhoneticEditor.cc:386` |
| `pinyin_guess_predicted_candidates_with_punctuations` | `PYPSuggestionEditor.cc:277` |
| `pinyin_reset` | `PYPPhoneticEditor.cc:380` |

### 1e. Candidate access (7 symbols)

| Symbol | Call sites |
|--------|-----------|
| `pinyin_get_n_candidate` | `PYPPhoneticEditor.cc:392`, `PYPPinyinEditor.cc:252`, `PYPBopomofoEditor.cc:358`, `PYPSuggestionCandidates.cc:34`, `PYPLibPinyinCandidates.cc:80` |
| `pinyin_get_candidate` | `PYPPhoneticEditor.cc:459,480`, `PYPPinyinEditor.cc:271`, `PYPBopomofoEditor.cc:371`, `PYPSuggestionCandidates.cc:38,86`, `PYPLibPinyinCandidates.cc:95,105,128` |
| `pinyin_get_candidate_type` | `PYPPhoneticEditor.cc:463,483`, `PYPPinyinEditor.cc:272`, `PYPBopomofoEditor.cc:372`, `PYPSuggestionCandidates.cc:41`, `PYPLibPinyinCandidates.cc:97` |
| `pinyin_get_candidate_string` | `PYPPhoneticEditor.cc:474`, `PYPSuggestionCandidates.cc:58`, `PYPLibPinyinCandidates.cc:56,77` |
| `pinyin_get_candidate_nbest_index` | `PYPPhoneticEditor.cc:489`, `PYPLibPinyinCandidates.cc:113` |
| `pinyin_is_user_candidate` | `PYPLibPinyinCandidates.cc:63` |
| `pinyin_remove_user_candidate` | `PYPLibPinyinCandidates.cc:69` |

### 1f. Candidate selection and training (3 symbols)

| Symbol | Call sites |
|--------|-----------|
| `pinyin_choose_candidate` | `PYPPhoneticEditor.cc:487,494`, `PYPLibPinyinCandidates.cc:111,132,140,147` |
| `pinyin_choose_predicted_candidate` | `PYPSuggestionCandidates.cc:87` |
| `pinyin_train` | `PYPLibPinyinCandidates.cc:117,155` |

### 1g. Sentence retrieval (2 symbols)

| Symbol | Call sites |
|--------|-----------|
| `pinyin_get_sentence` | `PYPPinyinEditor.cc:276`, `PYPBopomofoEditor.cc:376`, `PYPLibPinyinCandidates.cc:51,109` |
| `pinyin_get_character_offset` | `PYPPinyinEditor.cc:290`, `PYPBopomofoEditor.cc:404` |

### 1h. Pinyin key / cursor positioning (5 symbols)

| Symbol | Call sites |
|--------|-----------|
| `pinyin_get_pinyin_key_rest` | `PYPPhoneticEditor.cc:504`, `PYPLibPinyinCandidates.cc:150` |
| `pinyin_get_pinyin_key_rest_positions` | `PYPPhoneticEditor.cc:508`, `PYPLibPinyinCandidates.cc:153` |
| `pinyin_get_pinyin_offset` | `PYPPhoneticEditor.cc:395` |
| `pinyin_get_left_pinyin_offset` | `PYPPhoneticEditor.cc:414` |
| `pinyin_get_right_pinyin_offset` | `PYPPhoneticEditor.cc:430` |

### 1i. Auxiliary text (3 symbols)

| Symbol | Call sites |
|--------|-----------|
| `pinyin_get_full_pinyin_auxiliary_text` | `PYPFullPinyinEditor.cc:84`, `PYPCloudCandidates.cc:704` |
| `pinyin_get_double_pinyin_auxiliary_text` | `PYPDoublePinyinEditor.cc:84` |
| `pinyin_get_chewing_auxiliary_text` | `PYPBopomofoEditor.cc:425` |

### 1j. User data / persistence (6 symbols)

| Symbol | Call sites |
|--------|-----------|
| `pinyin_mask_out` | `PYLibPinyin.cc:388` |
| `pinyin_remember_user_input` | `PYLibPinyin.cc:443` |
| `pinyin_begin_add_phrases` | `PYLibPinyin.cc:408` |
| `pinyin_iterator_add_phrase` | `PYLibPinyin.cc:421` |
| `pinyin_end_add_phrases` | `PYLibPinyin.cc:434` |
| `pinyin_save` | `PYLibPinyin.cc:353` |

### 1k. Phrase / bigram export (7 symbols)

| Symbol | Call sites |
|--------|-----------|
| `pinyin_begin_get_phrases` | `PYLibPinyin.cc:463` |
| `pinyin_iterator_has_next_phrase` | `PYLibPinyin.cc:467,511` |
| `pinyin_iterator_get_next_phrase` | `PYLibPinyin.cc:471,515` |
| `pinyin_end_get_phrases` | `PYLibPinyin.cc:494` |
| `pinyin_begin_get_bigram_phrases` | `PYLibPinyin.cc:507` |
| `pinyin_bigram_iterator_has_next_phrase` | `PYLibPinyin.cc:542` |
| `pinyin_bigram_iterator_get_next_phrase` | `PYLibPinyin.cc:546` |

Note: `pinyin_end_get_bigram_phrases` is also called; see below.

### 1l. Bigram export end (1 symbol)

| Symbol | Call sites |
|--------|-----------|
| `pinyin_end_get_bigram_phrases` | `PYLibPinyin.cc:564` |

**Total: 50 unique live symbols** (some appear in multiple categories via
`pinyin_save`; de-duplicated the count is 50).

---

## 2. Per-symbol signatures and ownership/lifetime semantics

Signatures from `libpinyin/src/pinyin.h`. Ownership column:
- **Handle (caller-managed)**: caller receives an opaque handle and must
  pass it to the matching `free`/`end`/`fini` function.
- **Caller-owned (g_free)**: caller receives a `gchar*` / `char*` that
  must be freed with `g_free()`.
- **Caller-owned (g_strfreev)**: caller receives a `gchar**` array that
  must be freed with `g_strfreev()`.
- **Instance-borrowed**: returned pointer is owned by the instance; valid
  until the next mutating call on that instance. Never freed by caller.
- **Out-param (scalar)**: value written to a caller-supplied `guint*`,
  `guint8*`, `guint16*`, `size_t*`, or `gboolean*` pointer.
- **N/A**: void return, or boolean success indicator only.

### Context lifecycle

```
pinyin_context_t * pinyin_init(const char * systemdir, const char * userdir);
```
Returns: **Handle (caller-managed)** — freed by `pinyin_fini`.

```
void pinyin_fini(pinyin_context_t * context);
```
Returns: **N/A** — releases the context handle.

```
pinyin_instance_t * pinyin_alloc_instance(pinyin_context_t * context);
```
Returns: **Handle (caller-managed)** — freed by `pinyin_free_instance`.

```
void pinyin_free_instance(pinyin_instance_t * instance);
```
Returns: **N/A** — releases the instance handle.

### Configuration

```
bool pinyin_set_options(pinyin_context_t * context, pinyin_option_t options);
```
Returns: **N/A** (bool success). Applies to all instances sharing this context.

```
bool pinyin_set_double_pinyin_scheme(pinyin_context_t * context,
                                     DoublePinyinScheme scheme);
```
Returns: **N/A** (bool success).

```
bool pinyin_set_zhuyin_scheme(pinyin_context_t * context,
                               ZhuyinScheme scheme);
```
Returns: **N/A** (bool success).

```
bool pinyin_load_addon_phrase_library(pinyin_context_t * context,
                                      guint8 index);
```
Returns: **N/A** (bool success).

```
bool pinyin_save(pinyin_context_t * context);
```
Returns: **N/A** (bool success).

### Parsing

```
size_t pinyin_parse_more_full_pinyins(pinyin_instance_t * instance,
                                      const char * pinyins);
```
Returns: `size_t` — number of bytes consumed from `pinyins`.

```
size_t pinyin_parse_more_double_pinyins(pinyin_instance_t * instance,
                                        const char * pinyins);
```
Returns: `size_t` — bytes consumed.

```
size_t pinyin_parse_more_chewings(pinyin_instance_t * instance,
                                   const char * chewings);
```
Returns: `size_t` — bytes consumed.

```
bool pinyin_in_chewing_keyboard(pinyin_instance_t * instance,
                                 const char key,
                                 gchar *** symbols);
```
Returns: **N/A** (bool). Out-param `symbols`: **Caller-owned (g_strfreev)**.

### Sentence / guess

```
bool pinyin_guess_sentence(pinyin_instance_t * instance);
```
Returns: **N/A** (bool success). Populates the instance's internal sentence
buffer. Must be called after parsing and after `pinyin_choose_candidate`.

```
bool pinyin_guess_candidates(pinyin_instance_t * instance,
                              size_t offset,
                              sort_option_t sort_option);
```
Returns: **N/A** (bool success). Populates the instance's candidate list.

```
bool pinyin_guess_predicted_candidates_with_punctuations
    (pinyin_instance_t * instance, const char * prefix);
```
Returns: **N/A** (bool success). Populates predicted candidates for the
suggestion editor (post-commit next-word prediction).

```
bool pinyin_reset(pinyin_instance_t * instance);
```
Returns: **N/A** (bool success). Clears parsing + sentence state.

### Candidate access

```
bool pinyin_get_n_candidate(pinyin_instance_t * instance, guint * num);
```
Returns: **N/A** (bool success). Out-param `num`: **Out-param (scalar)**.

```
bool pinyin_get_candidate(pinyin_instance_t * instance,
                           guint index,
                           lookup_candidate_t ** candidate);
```
Returns: **N/A** (bool success). Out-param `candidate`:
**Instance-borrowed** — pointer into the instance's candidate array; valid
until the next `pinyin_guess_candidates`/`pinyin_choose_candidate` call.

```
bool pinyin_get_candidate_type(pinyin_instance_t * instance,
                                lookup_candidate_t * candidate,
                                lookup_candidate_type_t * type);
```
Returns: **N/A** (bool). Out-param `type`: **Out-param (scalar)**.
Values: `NBEST_MATCH_CANDIDATE`, `NORMAL_CANDIDATE`,
`PREDICTED_BIGRAM_CANDIDATE`, `PREDICTED_PREFIX_CANDIDATE`,
`PREDICTED_PUNCTUATION_CANDIDATE`, and others.

```
bool pinyin_get_candidate_string(pinyin_instance_t * instance,
                                  lookup_candidate_t * candidate,
                                  const gchar ** utf8_str);
```
Returns: **N/A** (bool). Out-param `utf8_str`: **Instance-borrowed** — the
string is owned by the candidate object inside the instance. Valid until
the candidate array is invalidated. Consumer must copy before reuse
(ibus-libpinyin copies into `EnhancedCandidate.m_display_string` via
`std::string` assignment).

```
bool pinyin_get_candidate_nbest_index(pinyin_instance_t * instance,
                                       lookup_candidate_t * candidate,
                                       guint8 * index);
```
Returns: **N/A** (bool). Out-param `index`: **Out-param (scalar)**.
Only meaningful for `NBEST_MATCH_CANDIDATE` type.

```
bool pinyin_is_user_candidate(pinyin_instance_t * instance,
                               lookup_candidate_t * candidate,
                               gboolean * is_user);
```
Returns: **N/A** (bool). Out-param `is_user`: **Out-param (scalar)**.

```
bool pinyin_remove_user_candidate(pinyin_instance_t * instance,
                                   guint index,
                                   lookup_candidate_t * candidate);
```
Returns: **N/A** (bool success).

### Candidate selection and training

```
guint pinyin_choose_candidate(pinyin_instance_t * instance,
                               size_t offset,
                               lookup_candidate_t * candidate);
```
Returns: `guint` — the new lookup cursor position (byte offset into input
after the chosen candidate's pinyin span). The instance's internal state is
mutated: the candidate is committed at the given offset.

```
bool pinyin_choose_predicted_candidate(pinyin_instance_t * instance,
                                        lookup_candidate_t * candidate);
```
Returns: **N/A** (bool success). Used only in the suggestion editor path.

```
bool pinyin_train(pinyin_instance_t * instance, guint8 index);
```
Returns: **N/A** (bool success). Trains the n-gram model with the
committed sentence at the given n-best index. Index comes from
`pinyin_get_candidate_nbest_index`.

### Sentence retrieval

```
bool pinyin_get_sentence(pinyin_instance_t * instance,
                          guint8 index,
                          char ** sentence);
```
Returns: **N/A** (bool). Out-param `sentence`: **Caller-owned (g_free)**.
The index parameter selects the n-best sentence variant; typically 0
for the best match.

```
bool pinyin_get_character_offset(pinyin_instance_t * instance,
                                  const char * sentence,
                                  size_t offset,
                                  size_t * character_offset);
```
Returns: **N/A** (bool). Out-param `character_offset`: **Out-param (scalar)**.
Converts a pinyin byte offset to a character offset within the sentence
(for cursor positioning in preedit text).

### Pinyin key / cursor positioning

```
bool pinyin_get_pinyin_key_rest(pinyin_instance_t * instance,
                                 guint index,
                                 PinyinKeyPos ** key_rest);
```
Returns: **N/A** (bool). Out-param `key_rest`: **Instance-borrowed** —
pointer into the instance's key-rest array.

```
bool pinyin_get_pinyin_key_rest_positions(pinyin_instance_t * instance,
                                           PinyinKeyPos * key_rest,
                                           guint16 * begin,
                                           guint16 * end);
```
Returns: **N/A** (bool). Out-params `begin`, `end`:
**Out-param (scalar)** — byte positions of this pinyin key's span.
Either pointer may be NULL to skip.

```
bool pinyin_get_pinyin_offset(pinyin_instance_t * instance,
                               size_t cursor,
                               size_t * offset);
```
Returns: **N/A** (bool). Out-param `offset`: **Out-param (scalar)**.

```
bool pinyin_get_left_pinyin_offset(pinyin_instance_t * instance,
                                    size_t offset,
                                    size_t * left);
```
Returns: **N/A** (bool). Out-param `left`: **Out-param (scalar)**.

```
bool pinyin_get_right_pinyin_offset(pinyin_instance_t * instance,
                                     size_t offset,
                                     size_t * right);
```
Returns: **N/A** (bool). Out-param `right`: **Out-param (scalar)**.

### Auxiliary text

```
bool pinyin_get_full_pinyin_auxiliary_text(pinyin_instance_t * instance,
                                            guint cursor,
                                            gchar ** aux_text);
```
Returns: **N/A** (bool). Out-param `aux_text`: **Caller-owned (g_free)**.

```
bool pinyin_get_double_pinyin_auxiliary_text(pinyin_instance_t * instance,
                                              guint cursor,
                                              gchar ** aux_text);
```
Returns: **N/A** (bool). Out-param `aux_text`: **Caller-owned (g_free)**.

```
bool pinyin_get_chewing_auxiliary_text(pinyin_instance_t * instance,
                                        guint cursor,
                                        gchar ** aux_text);
```
Returns: **N/A** (bool). Out-param `aux_text`: **Caller-owned (g_free)**.

### User data / persistence

```
bool pinyin_mask_out(pinyin_context_t * context,
                      phrase_token_t mask,
                      phrase_token_t value);
```
Returns: **N/A** (bool success). Masks out phrase tokens matching the
pattern before re-import.

```
bool pinyin_remember_user_input(pinyin_instance_t * instance,
                                 const char * pinyins,
                                 const char * phrase);
```
Returns: **N/A** (bool success). Records a user-provided phrase with its
pinyin spelling.

```
import_iterator_t * pinyin_begin_add_phrases(pinyin_context_t * context,
                                              guint8 index);
```
Returns: **Handle (caller-managed)** — freed by `pinyin_end_add_phrases`.

```
bool pinyin_iterator_add_phrase(import_iterator_t * iter,
                                 const char * phrase,
                                 const char * pinyin);
```
Returns: **N/A** (bool success).

```
void pinyin_end_add_phrases(import_iterator_t * iter);
```
Returns: **N/A** — releases the import iterator handle.

### Phrase export

```
export_iterator_t * pinyin_begin_get_phrases(pinyin_context_t * context,
                                              guint8 index);
```
Returns: **Handle (caller-managed)** — freed by `pinyin_end_get_phrases`.

```
bool pinyin_iterator_has_next_phrase(export_iterator_t * iter);
```
Returns: bool — whether more phrases remain.

```
bool pinyin_iterator_get_next_phrase(export_iterator_t * iter,
                                      gchar ** phrase,
                                      gchar ** pinyin,
                                      gint * count);
```
Returns: **N/A** (bool). Out-params `phrase`, `pinyin`:
**Caller-owned (g_free)** each. Out-param `count`:
**Out-param (scalar)**.

```
void pinyin_end_get_phrases(export_iterator_t * iter);
```
Returns: **N/A** — releases the export iterator handle.

### Bigram export

```
bigram_export_iterator_t * pinyin_begin_get_bigram_phrases
    (pinyin_context_t * context, guint8 index);
```
Returns: **Handle (caller-managed)** — freed by
`pinyin_end_get_bigram_phrases`.

```
bool pinyin_bigram_iterator_has_next_phrase
    (bigram_export_iterator_t * iter);
```
Returns: bool — whether more bigram entries remain.

```
bool pinyin_bigram_iterator_get_next_phrase
    (bigram_export_iterator_t * iter,
     gchar ** phrase, gchar ** pinyin, gint * count);
```
Returns: **N/A** (bool). Out-params `phrase`, `pinyin`:
**Caller-owned (g_free)** each. Out-param `count`:
**Out-param (scalar)**.

```
void pinyin_end_get_bigram_phrases(bigram_export_iterator_t * iter);
```
Returns: **N/A** — releases the bigram export iterator handle.

---

## 3. Call ordering and state model

### Opaque types

| Type | Created by | Destroyed by | Multiplicity |
|------|-----------|-------------|-------------|
| `pinyin_context_t` | `pinyin_init` | `pinyin_fini` | One per input mode (pinyin, bopomofo) |
| `pinyin_instance_t` | `pinyin_alloc_instance` | `pinyin_free_instance` | One per active editor |
| `lookup_candidate_t` | `pinyin_get_candidate` (borrow) | Invalidated by next guess/choose | Transient |
| `PinyinKeyPos` (`ChewingKeyRest`) | `pinyin_get_pinyin_key_rest` (borrow) | Invalidated by next parse | Transient |
| `import_iterator_t` | `pinyin_begin_add_phrases` | `pinyin_end_add_phrases` | One at a time per context |
| `export_iterator_t` | `pinyin_begin_get_phrases` | `pinyin_end_get_phrases` | One at a time per context |
| `bigram_export_iterator_t` | `pinyin_begin_get_bigram_phrases` | `pinyin_end_get_bigram_phrases` | One at a time per context |

### Lifecycle phases

**Phase 1 — Initialization (once at startup)**

```
context = pinyin_init(systemdir, userdir)
pinyin_set_options(context, options)
pinyin_set_double_pinyin_scheme(context, scheme)    // if double-pinyin mode
pinyin_set_zhuyin_scheme(context, scheme)            // if bopomofo mode
pinyin_load_addon_phrase_library(context, index)     // for each addon library
instance = pinyin_alloc_instance(context)
```

Context-level configuration (`set_options`, `set_*_scheme`,
`load_addon_phrase_library`) can be called at any time and applies to all
instances sharing the context. ibus-libpinyin calls them from
`PYLibPinyin.cc` whenever GSettings values change.

**Phase 2 — Input loop (per keystroke)**

```
// Parse input
pinyin_len = pinyin_parse_more_full_pinyins(instance, text)
// or:       pinyin_parse_more_double_pinyins(instance, text)
// or:       pinyin_parse_more_chewings(instance, text)

// Generate sentence hypothesis
pinyin_guess_sentence(instance)

// Get candidates at cursor position
pinyin_guess_candidates(instance, offset, sort_option)
pinyin_get_n_candidate(instance, &num)

// Access each candidate
pinyin_get_candidate(instance, i, &candidate)
pinyin_get_candidate_type(instance, candidate, &type)
pinyin_get_candidate_string(instance, candidate, &str)
```

**Phase 3 — Candidate selection**

Two paths depending on candidate type:

**Path A — NBEST_MATCH_CANDIDATE (sentence-level selection at offset 0):**

```
pinyin_choose_candidate(instance, 0, candidate)
pinyin_get_candidate_nbest_index(instance, candidate, &index)
pinyin_get_sentence(instance, index, &sentence)
pinyin_train(instance, index)
g_free(sentence)
// → commit the sentence text
```

**Path B — Normal candidate (word-level iterative selection):**

```
lookup_cursor = pinyin_choose_candidate(instance, lookup_cursor, candidate)
pinyin_guess_sentence(instance)

if (lookup_cursor == text.length()) {
    // All input consumed → commit
    pinyin_get_sentence(instance, 0, &sentence)
    pinyin_train(instance, 0)
    g_free(sentence)
} else {
    // Partial selection → update cursor and continue
    pinyin_get_pinyin_key_rest(instance, lookup_cursor, &pos)
    pinyin_get_pinyin_key_rest_positions(instance, pos, &begin, NULL)
    // set cursor to begin, continue input loop
}
```

**Phase 4 — Prediction (after commit)**

```
pinyin_guess_predicted_candidates_with_punctuations(instance, prefix)
// access candidates the same way as Phase 2
pinyin_choose_predicted_candidate(instance, candidate)
// → commit predicted text
```

**Phase 5 — Auxiliary text (for display)**

```
pinyin_get_full_pinyin_auxiliary_text(instance, cursor, &aux_text)
// or: pinyin_get_double_pinyin_auxiliary_text(instance, cursor, &aux_text)
// or: pinyin_get_chewing_auxiliary_text(instance, cursor, &aux_text)
g_free(aux_text)
```

**Phase 6 — Cursor navigation helpers**

```
pinyin_get_pinyin_offset(instance, cursor, &offset)
pinyin_get_left_pinyin_offset(instance, offset, &left)
pinyin_get_right_pinyin_offset(instance, offset, &right)
pinyin_get_character_offset(instance, sentence, cursor, &char_offset)
```

**Phase 7 — Reset (on Escape/cancel)**

```
pinyin_reset(instance)
```

**Phase 8 — Periodic save (5-minute GLib timer)**

```
pinyin_save(context)
```

The consumer runs this on a `LIBPINYIN_SAVE_TIMEOUT = 5 * 60` (300s)
GLib timer in `PYLibPinyin.cc`. Also called at shutdown before `pinyin_fini`.

**Phase 9 — User data import/export (settings UI)**

```
// Import
pinyin_mask_out(context, mask, value)
iter = pinyin_begin_add_phrases(context, index)
pinyin_iterator_add_phrase(iter, phrase, pinyin)
pinyin_end_add_phrases(iter)
pinyin_save(context)

// Export phrases
iter = pinyin_begin_get_phrases(context, index)
while (pinyin_iterator_has_next_phrase(iter)) {
    pinyin_iterator_get_next_phrase(iter, &phrase, &pinyin, &count)
    g_free(phrase); g_free(pinyin);
}
pinyin_end_get_phrases(iter)

// Export bigrams
iter = pinyin_begin_get_bigram_phrases(context, index)
while (pinyin_bigram_iterator_has_next_phrase(iter)) {
    pinyin_bigram_iterator_get_next_phrase(iter, &phrase, &pinyin, &count)
    g_free(phrase); g_free(pinyin);
}
pinyin_end_get_bigram_phrases(iter)
```

**Phase 10 — Shutdown**

```
pinyin_free_instance(instance)   // for each instance
pinyin_save(context)
pinyin_fini(context)
```

### Cloud input special case

`pinyin_remember_user_input` and `pinyin_parse_more_full_pinyins` are also
called from `PYLibPinyin.cc:rememberCloudInput()`, which records a
cloud-sourced phrase into the user dictionary. This uses a dedicated
instance (`allocPinyinInstance`) for the parse, then calls
`pinyin_remember_user_input` on that instance.

---

## 4. Config surface

All configuration is applied through `pinyin_set_options` using bitmask
flags. The consumer constructs these from GSettings keys in `PYPConfig.cc`.

### pinyin_option_t flags

**Incomplete pinyin (consumer default: ON):**
- `PINYIN_INCOMPLETE` (0x01000000)
- `ZHUYIN_INCOMPLETE` (0x02000000)

**Correction flags (consumer default: all ON via `PINYIN_CORRECT_ALL`):**
- `PINYIN_CORRECT_GN_NG`, `PINYIN_CORRECT_MG_NG`, `PINYIN_CORRECT_IOU_IU`,
  `PINYIN_CORRECT_UEI_UI`, `PINYIN_CORRECT_UEN_UN`, `PINYIN_CORRECT_UE_VE`,
  `PINYIN_CORRECT_V_U`, `PINYIN_CORRECT_ON_ONG`

**Ambiguity flags (consumer default: all OFF):**
- `PINYIN_AMB_C_CH`, `PINYIN_AMB_S_SH`, `PINYIN_AMB_Z_ZH`,
  `PINYIN_AMB_F_H`, `PINYIN_AMB_G_K`, `PINYIN_AMB_L_N`,
  `PINYIN_AMB_L_R`, `PINYIN_AMB_AN_ANG`, `PINYIN_AMB_EN_ENG`,
  `PINYIN_AMB_IN_ING`

**Always added by `setPinyinOptions` (hard-coded in consumer):**
- `USE_RESPLIT_TABLE`
- `USE_DIVIDED_TABLE`

**Dynamic adjustment (configurable):**
- `DYNAMIC_ADJUST` — enabled by default; user-togglable via GSettings key
  `dynamic-adjust`.

Consumer default formula:
```
PINYIN_DEFAULT_OPTION = PINYIN_INCOMPLETE | ZHUYIN_INCOMPLETE | PINYIN_CORRECT_ALL
final_options = user_toggled_flags | USE_RESPLIT_TABLE | USE_DIVIDED_TABLE
```

### sort_option_t

Three presets selectable via GSettings `sort-option`:

| Value | Constant | Meaning |
|-------|----------|---------|
| 0 | `SORT_BY_PHRASE_LENGTH \| SORT_BY_PINYIN_LENGTH \| SORT_BY_FREQUENCY` | Default |
| 1 | `SORT_BY_PHRASE_LENGTH \| SORT_BY_PINYIN_LENGTH \| SORT_BY_FREQUENCY \| SORT_WITHOUT_SENTENCE_CANDIDATE` | No sentence candidate (legacy) |
| 2 | `SORT_BY_PHRASE_LENGTH \| SORT_BY_PINYIN_LENGTH \| SORT_BY_FREQUENCY \| SORT_WITHOUT_SENTENCE_CANDIDATE \| SORT_WITHOUT_LONGER_CANDIDATE` | Short candidates only |

### Double-pinyin schemes (6)

`DoublePinyinScheme` enum: `DOUBLE_PINYIN_MS` (0), `DOUBLE_PINYIN_ZRM` (1),
`DOUBLE_PINYIN_ABC` (2), `DOUBLE_PINYIN_ZIGUANG` (3),
`DOUBLE_PINYIN_PYJJ` (4), `DOUBLE_PINYIN_XHE` (5).

### Zhuyin/Bopomofo schemes (4)

`ZhuyinScheme` enum: `ZHUYIN_STANDARD` (0), `ZHUYIN_HSU` (1),
`ZHUYIN_IBM` (2), `ZHUYIN_ETEN` (3).

### Display styles

`display_style_t` enum (consumer-side only, not passed to libpinyin):
`DISPLAY_STYLE_TRADITIONAL` (0), `DISPLAY_STYLE_COMPACT` (1),
`DISPLAY_STYLE_COMPATIBILITY` (2).

These affect how ibus-libpinyin renders preedit/auxiliary text but do not
change which libpinyin symbols are called.

---

## 5. Sentence / n-best path — resolved

**Finding: YES, ibus-libpinyin fully exercises the sentence/n-best path.**

The consumer uses the n-best sentence mechanism in two files:

### PYPPhoneticEditor.cc (selectCandidateInPage)

At `PYPPhoneticEditor.cc:485–491`:
```c
if (NBEST_MATCH_CANDIDATE == type) {
    pinyin_choose_candidate (m_instance, 0, candidate);
    guint8 index = 0;
    pinyin_get_candidate_nbest_index(m_instance, candidate, &index);
    commit (index);
    return TRUE;
}
```

The `commit(index)` path calls `pinyin_get_sentence(instance, index, &sentence)`
and `pinyin_train(instance, index)`.

### PYPLibPinyinCandidates.cc (selectCandidate)

At `PYPLibPinyinCandidates.cc:105–117`:
```c
if (NBEST_MATCH_CANDIDATE == type) {
    pinyin_choose_candidate (instance, 0, candidate);
    guint8 index = 0;
    pinyin_get_candidate_nbest_index (instance, candidate, &index);

    pinyin_get_sentence (instance, index, &sentence);
    pinyin_train (instance, index);
    // ... commit sentence
}
```

### Implication for pinyin-capi

The `pinyin_get_sentence` and `pinyin_train` functions must support a
non-zero n-best index parameter. The n-best index is obtained from
`pinyin_get_candidate_nbest_index` and passed through to both sentence
retrieval and training. A `pinyin-capi` implementation cannot simplify
these to index-0-only.

---

## 6. Out-of-subset symbols (29 not called by ibus-libpinyin)

The following 29 symbols are exported by `libpinyin.ver` but never called
by ibus-libpinyin at tag 1.16.5:

```text
pinyin_convert_full_pinyin_to_double_pinyin
pinyin_convert_full_pinyin_to_zhuyin
pinyin_end_get_bigram_phrases_by_phrase_string
pinyin_begin_get_bigram_phrases_by_phrase_string
pinyin_bigram_string_iterator_has_next_phrase
pinyin_bigram_string_iterator_get_next_phrase
pinyin_get_bopomofo_string
pinyin_get_chewing_key
pinyin_get_full_pinyin_string
pinyin_get_luoma_pinyin_string
pinyin_get_n_pinyin
pinyin_get_parsed_input_length
pinyin_get_pinyin_key
pinyin_get_pinyin_string
pinyin_get_pinyin_strings_of_key
pinyin_get_predicted_candidates
pinyin_get_secondary_zhuyin_string
pinyin_get_zhuyin_key
pinyin_get_zhuyin_string
pinyin_guess_predicted_candidates
pinyin_has_unparsed_tail
pinyin_lookup_candidates
pinyin_lookup_pinyin_candidates
pinyin_parse_full_pinyin
pinyin_parse_more_full_pinyins_with_options
pinyin_phrase_segment
pinyin_token_get_n_pronunciation
pinyin_token_get_phrase
pinyin_token_get_pronunciation
```

Notes:
- `pinyin_get_n_pinyin`, `pinyin_get_pinyin_key`, `pinyin_get_pinyin_string`
  appear only in `#if 0` dead code at `PYPPinyinEditor.cc:296–333` and are
  therefore excluded from the live subset.
- `pinyin_get_predicted_candidates` and `pinyin_guess_predicted_candidates`
  are older variants superseded by
  `pinyin_guess_predicted_candidates_with_punctuations`.
- `pinyin_parse_full_pinyin` is the single-syllable parser; ibus-libpinyin
  uses only the `parse_more_*` (multi-syllable) variants.
- `pinyin_lookup_candidates` and `pinyin_lookup_pinyin_candidates` are
  lower-level lookup functions not used by the consumer.

---

## 7. Error and abort behaviour

### libpinyin error model

libpinyin functions follow a simple error convention:
- Functions returning `bool` return `false` on failure.
- Functions returning pointers return `NULL` on failure.
- Functions returning `size_t` return 0 on empty/failed parse.
- There are no error codes, error strings, or `errno`-style state.

### Consumer error handling

ibus-libpinyin generally does **not** check return values from libpinyin
calls. The consumer assumes:
- `pinyin_init` succeeds (no NULL check; failure would crash).
- `pinyin_alloc_instance` succeeds (no NULL check).
- `pinyin_get_candidate` succeeds for indices < `pinyin_get_n_candidate`.
- `pinyin_get_sentence` succeeds when a sentence has been guessed.

The consumer uses `assert()` in a few places:
- `PYPSuggestionCandidates.cc:54` — asserts candidate type is one of the
  three predicted types.
- `PYPSuggestionEditor.cc:377` — `assert(FALSE)` in default case of
  candidate type switch (unreachable in normal operation).
- `PYPLibPinyinCandidates.cc:348` — asserts `lookup_cursor <=
  m_text.length()` after `pinyin_choose_candidate`.

### Implications for pinyin-capi

A Rust implementation should:
1. Return `Result` from all public APIs (per AGENTS.md constitution §4).
2. Never panic on any input.
3. Treat the consumer's unchecked calls as implicit contracts: these
   functions must succeed for in-range inputs with valid state. An
   implementation that returned errors in these cases would break the
   consumer's expectations.
4. The `assert()` sites in the consumer are the places where unexpected
   state would surface as crashes — these are the most important
   behavioural contracts to maintain.

---

## Boundary notes

This is the frontend-called subset, not a promise to clone all of libpinyin.
Symbols needed only by the differential harness may be added to
`pinyin-oracle` without expanding the supported `pinyin-capi` surface.
Every C-ABI symbol requires a dedicated task, a `// SAFETY:` argument for
each unsafe block, NULL/invalid-input coverage, and an oracle-backed
behavioural test before freeze.
