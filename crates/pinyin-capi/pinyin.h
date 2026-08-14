#ifndef PINYIN_CAPI_H
#define PINYIN_CAPI_H

#include <stddef.h>
#include <stdbool.h>
#include <stdint.h>

// `lookup_candidate_type_t` from `pinyin.h`.
typedef enum lookup_candidate_type_t {
  // Best sentence-level match.
  NBEST_MATCH_CANDIDATE = 1,
  // Normal word candidate.
  NORMAL_CANDIDATE = 2,
  // Zombie candidate.
  ZOMBIE_CANDIDATE = 3,
  // Predicted bigram candidate.
  PREDICTED_BIGRAM_CANDIDATE = 4,
  // Predicted prefix candidate.
  PREDICTED_PREFIX_CANDIDATE = 5,
  // Addon dictionary candidate.
  ADDON_CANDIDATE = 6,
  // Longer candidate.
  LONGER_CANDIDATE = 7,
  // Predicted punctuation candidate.
  PREDICTED_PUNCTUATION_CANDIDATE = 8,
} lookup_candidate_type_t;

// `sort_option_t` flag bits from `pinyin.h`.
typedef enum sort_option_t {
  // Exclude sentence candidate.
  SORT_WITHOUT_SENTENCE_CANDIDATE = 1,
  // Exclude longer candidates.
  SORT_WITHOUT_LONGER_CANDIDATE = 2,
  // Sort by phrase length.
  SORT_BY_PHRASE_LENGTH = 4,
  // Sort by pinyin length.
  SORT_BY_PINYIN_LENGTH = 8,
  // Sort by frequency.
  SORT_BY_FREQUENCY = 16,
} sort_option_t;

// `DoublePinyinScheme` from `pinyin_custom2.h`.
typedef enum DoublePinyinScheme {
  // Ziran码 scheme.
  DOUBLE_PINYIN_ZRM = 1,
  // Microsoft scheme.
  DOUBLE_PINYIN_MS = 2,
  // Ziguang scheme.
  DOUBLE_PINYIN_ZIGUANG = 3,
  // ABC scheme.
  DOUBLE_PINYIN_ABC = 4,
  // PYJJ scheme.
  DOUBLE_PINYIN_PYJJ = 5,
  // Xiaohe scheme.
  DOUBLE_PINYIN_XHE = 6,
  // User's keyboard.
  DOUBLE_PINYIN_CUSTOMIZED = 30,
} DoublePinyinScheme;

// `ZhuyinScheme` from `pinyin_custom2.h`.
typedef enum ZhuyinScheme {
  // Standard layout.
  ZHUYIN_STANDARD = 1,
  // Hsu layout.
  ZHUYIN_HSU = 2,
  // IBM layout.
  ZHUYIN_IBM = 3,
  // GinYieh layout.
  ZHUYIN_GINYIEH = 4,
  // Eten layout.
  ZHUYIN_ETEN = 5,
  // Eten26 layout.
  ZHUYIN_ETEN26 = 6,
  // Standard Dvorak layout.
  ZHUYIN_STANDARD_DVORAK = 7,
  // Hsu Dvorak layout.
  ZHUYIN_HSU_DVORAK = 8,
  // Dachen CP26 layout.
  ZHUYIN_DACHEN_CP26 = 9,
} ZhuyinScheme;

// Opaque bigram export iterator.
typedef struct bigram_export_iterator_t bigram_export_iterator_t;

// Opaque chewing key.
typedef struct ChewingKey ChewingKey;

// Opaque chewing key rest (position span).
typedef struct ChewingKeyRest ChewingKeyRest;

// Opaque export iterator.
typedef struct export_iterator_t export_iterator_t;

// Opaque import iterator.
typedef struct import_iterator_t import_iterator_t;

// Opaque lookup candidate (instance-borrowed, transient).
typedef struct lookup_candidate_t lookup_candidate_t;

// Opaque pinyin context (one per input mode).
typedef struct pinyin_context_t pinyin_context_t;

// Opaque pinyin instance (one per active editor).
typedef struct pinyin_instance_t pinyin_instance_t;

// `guint` — GLib unsigned int (= `c_uint`).
typedef unsigned int guint;

// `gchar` — GLib char (= `c_char`).
typedef char gchar;

// `pinyin_option_t` — bitmask of pinyin table flags.
typedef uint32_t pinyin_option_t;

// Get the number of candidates.
bool pinyin_get_n_candidate(struct pinyin_instance_t *instance, guint *num);

// Get a candidate by index.
bool pinyin_get_candidate(struct pinyin_instance_t *instance,
                          guint _index,
                          struct lookup_candidate_t **candidate);

// Get the type of a lookup candidate.
bool pinyin_get_candidate_type(struct pinyin_instance_t *instance,
                               struct lookup_candidate_t *candidate,
                               enum lookup_candidate_type_t *candidate_type);

// Get the display string of a candidate.
bool pinyin_get_candidate_string(struct pinyin_instance_t *instance,
                                 struct lookup_candidate_t *candidate,
                                 const gchar **utf8_str);

// Get the n-best index of a candidate.
bool pinyin_get_candidate_nbest_index(struct pinyin_instance_t *instance,
                                      struct lookup_candidate_t *candidate,
                                      uint8_t *index);

// Check whether a candidate is a user candidate.
bool pinyin_is_user_candidate(struct pinyin_instance_t *instance,
                              struct lookup_candidate_t *candidate);

// Remove a user candidate from the dictionary.
bool pinyin_remove_user_candidate(struct pinyin_instance_t *instance,
                                  struct lookup_candidate_t *candidate);

// Choose a candidate at an offset, returning the new cursor position.
int pinyin_choose_candidate(struct pinyin_instance_t *instance,
                            size_t _offset,
                            struct lookup_candidate_t *candidate);

// Choose a predicted candidate.
bool pinyin_choose_predicted_candidate(struct pinyin_instance_t *instance,
                                       struct lookup_candidate_t *candidate);

// Train the current sentence with the given n-best index.
bool pinyin_train(struct pinyin_instance_t *instance, uint8_t _index);

// Set pinyin options on the context.
bool pinyin_set_options(struct pinyin_context_t *context, pinyin_option_t _options);

// Set the double pinyin scheme.
bool pinyin_set_double_pinyin_scheme(struct pinyin_context_t *context, int _scheme);

// Set the zhuyin scheme.
bool pinyin_set_zhuyin_scheme(struct pinyin_context_t *context, int _scheme);

// Load an addon phrase library by index.
bool pinyin_load_addon_phrase_library(struct pinyin_context_t *context, uint8_t _index);

// Mask out phrase tokens matching a pattern.
bool pinyin_mask_out(struct pinyin_context_t *context, uint32_t _mask, uint32_t _value);

// Create a new pinyin context.
struct pinyin_context_t *pinyin_init(const char *_systemdir, const char *_userdir);

// Finalize and free a pinyin context.
void pinyin_fini(struct pinyin_context_t *context);

// Save user data.
bool pinyin_save(struct pinyin_context_t *context);

// Get the pinyin key rest at an offset.
bool pinyin_get_pinyin_key_rest(struct pinyin_instance_t *instance,
                                size_t _offset,
                                struct ChewingKeyRest **key_rest);

// Get the begin/end byte positions of a pinyin key rest.
bool pinyin_get_pinyin_key_rest_positions(struct pinyin_instance_t *instance,
                                          struct ChewingKeyRest *key_rest,
                                          uint16_t *begin,
                                          uint16_t *end);

// Get the lookup offset from a user cursor position.
bool pinyin_get_pinyin_offset(struct pinyin_instance_t *instance, size_t _cursor, size_t *offset);

// Get the left offset from a lookup offset.
bool pinyin_get_left_pinyin_offset(struct pinyin_instance_t *instance,
                                   size_t _offset,
                                   size_t *left);

// Get the right offset from a lookup offset.
bool pinyin_get_right_pinyin_offset(struct pinyin_instance_t *instance,
                                    size_t _offset,
                                    size_t *right);

// Allocate a new pinyin instance from a context.
struct pinyin_instance_t *pinyin_alloc_instance(struct pinyin_context_t *context);

// Free a pinyin instance.
void pinyin_free_instance(struct pinyin_instance_t *instance);

// Reset the pinyin instance (clear parsing and sentence state).
bool pinyin_reset(struct pinyin_instance_t *instance);

// Begin adding phrases to an index.
struct import_iterator_t *pinyin_begin_add_phrases(struct pinyin_context_t *context,
                                                   uint8_t _index);

// Add a phrase/pinyin pair to the import iterator.
bool pinyin_iterator_add_phrase(struct import_iterator_t *iter,
                                const char *_phrase,
                                const char *_pinyin,
                                int _count);

// End the import iterator and free it.
void pinyin_end_add_phrases(struct import_iterator_t *iter);

// Begin exporting phrases from an index.
struct export_iterator_t *pinyin_begin_get_phrases(struct pinyin_context_t *context, guint _index);

// Check whether the export iterator has a next phrase.
bool pinyin_iterator_has_next_phrase(struct export_iterator_t *iter);

// Get the next phrase from the export iterator.
bool pinyin_iterator_get_next_phrase(struct export_iterator_t *iter,
                                     gchar **phrase,
                                     gchar **pinyin,
                                     int *count);

// End the export iterator and free it.
void pinyin_end_get_phrases(struct export_iterator_t *iter);

// Begin exporting bigram phrases.
struct bigram_export_iterator_t *pinyin_begin_get_bigram_phrases(struct pinyin_context_t *context);

// Check whether the bigram export iterator has a next phrase.
bool pinyin_bigram_iterator_has_next_phrase(struct bigram_export_iterator_t *iter);

// Get the next phrase from the bigram export iterator.
bool pinyin_bigram_iterator_get_next_phrase(struct bigram_export_iterator_t *iter,
                                            gchar **phrase,
                                            gchar **pinyin,
                                            int *count);

// End the bigram export iterator and free it.
void pinyin_end_get_bigram_phrases(struct bigram_export_iterator_t *iter);

// Parse multiple full pinyins.
size_t pinyin_parse_more_full_pinyins(struct pinyin_instance_t *instance, const char *_pinyins);

// Parse multiple double pinyins.
size_t pinyin_parse_more_double_pinyins(struct pinyin_instance_t *instance, const char *_pinyins);

// Parse multiple chewing (bopomofo) inputs.
size_t pinyin_parse_more_chewings(struct pinyin_instance_t *instance, const char *_chewings);

// Check whether an input key is in the current chewing keyboard scheme.
bool pinyin_in_chewing_keyboard(struct pinyin_instance_t *instance, char _key, gchar ***symbols);

// Guess a sentence from saved pinyin keys.
bool pinyin_guess_sentence(struct pinyin_instance_t *instance);

// Guess predicted candidates with punctuations after a prefix.
bool pinyin_guess_predicted_candidates_with_punctuations(struct pinyin_instance_t *instance,
                                                         const char *_prefix);

// Get a sentence string from the instance (n-best variant).
bool pinyin_get_sentence(struct pinyin_instance_t *instance, uint8_t _index, char **sentence);

// Get character offset from a lookup byte offset within a sentence.
bool pinyin_get_character_offset(struct pinyin_instance_t *instance,
                                 const char *_phrase,
                                 size_t _offset,
                                 size_t *length);

// Guess candidates at the given offset with sort option.
bool pinyin_guess_candidates(struct pinyin_instance_t *instance,
                             size_t _offset,
                             guint _sort_option);

// Get auxiliary text for full pinyin display.
bool pinyin_get_full_pinyin_auxiliary_text(struct pinyin_instance_t *instance,
                                           size_t _cursor,
                                           gchar **aux_text);

// Get auxiliary text for double pinyin display.
bool pinyin_get_double_pinyin_auxiliary_text(struct pinyin_instance_t *instance,
                                             size_t _cursor,
                                             gchar **aux_text);

// Get auxiliary text for chewing (bopomofo) display.
bool pinyin_get_chewing_auxiliary_text(struct pinyin_instance_t *instance,
                                       size_t _cursor,
                                       gchar **aux_text);

// Remember a user-provided phrase with its current pinyin context.
bool pinyin_remember_user_input(struct pinyin_instance_t *instance,
                                const char *_phrase,
                                int _count);

#endif  /* PINYIN_CAPI_H */
