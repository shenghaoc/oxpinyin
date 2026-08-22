#ifndef PINYIN_CAPI_H
#define PINYIN_CAPI_H

#include <stddef.h>
#include <stdbool.h>
#include <stdint.h>
#define PHRASE_MASK 0x00FFFFFF
#define PHRASE_INDEX_LIBRARY_MASK 0x0F000000
#define PHRASE_INDEX_MAKE_TOKEN(phrase_index, token)                    \
    ( ( (phrase_index<<24) & PHRASE_INDEX_LIBRARY_MASK)|(token & PHRASE_MASK))
#define DOUBLE_PINYIN_DEFAULT DOUBLE_PINYIN_MS
#define ZHUYIN_DEFAULT ZHUYIN_STANDARD
typedef struct ChewingKey PinyinKey;
typedef struct ChewingKeyRest PinyinKeyPos;


// `null_token` = 0 (`novel_types.h:121`, tag 2.11.91).
#define null_token 0

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

// `PinyinTableFlag` constants the fork compiles against
typedef enum PinyinTableFlag {
  // `PINYIN_INCOMPLETE = 1U << 3` (`pinyin_custom2.h:34`).
  PINYIN_INCOMPLETE = (1 << 3),
  // `ZHUYIN_INCOMPLETE = 1U << 4` (`pinyin_custom2.h:35`).
  ZHUYIN_INCOMPLETE = (1 << 4),
  // `USE_TONE = 1U << 5` (`pinyin_custom2.h:36`).
  USE_TONE = (1 << 5),
  // `USE_DIVIDED_TABLE = 1U << 7` (`pinyin_custom2.h:38`).
  USE_DIVIDED_TABLE = (1 << 7),
  // `USE_RESPLIT_TABLE = 1U << 8` (`pinyin_custom2.h:39`).
  USE_RESPLIT_TABLE = (1 << 8),
  // `DYNAMIC_ADJUST = 1U << 9` (`pinyin_custom2.h:40`).
  DYNAMIC_ADJUST = (1 << 9),
} PinyinTableFlag;

// `PinyinAmbiguity2` fuzzy-pinyin bits the fork compiles against
typedef enum PinyinAmbiguity2 {
  // `PINYIN_AMB_C_CH = 1U << 10` (`pinyin_custom2.h:50`).
  PINYIN_AMB_C_CH = (1 << 10),
  // `PINYIN_AMB_S_SH = 1U << 11` (`pinyin_custom2.h:51`).
  PINYIN_AMB_S_SH = (1 << 11),
  // `PINYIN_AMB_Z_ZH = 1U << 12` (`pinyin_custom2.h:52`).
  PINYIN_AMB_Z_ZH = (1 << 12),
  // `PINYIN_AMB_F_H = 1U << 13` (`pinyin_custom2.h:53`).
  PINYIN_AMB_F_H = (1 << 13),
  // `PINYIN_AMB_G_K = 1U << 14` (`pinyin_custom2.h:54`).
  PINYIN_AMB_G_K = (1 << 14),
  // `PINYIN_AMB_L_N = 1U << 15` (`pinyin_custom2.h:55`).
  PINYIN_AMB_L_N = (1 << 15),
  // `PINYIN_AMB_L_R = 1U << 16` (`pinyin_custom2.h:56`).
  PINYIN_AMB_L_R = (1 << 16),
  // `PINYIN_AMB_AN_ANG = 1U << 17` (`pinyin_custom2.h:57`).
  PINYIN_AMB_AN_ANG = (1 << 17),
  // `PINYIN_AMB_EN_ENG = 1U << 18` (`pinyin_custom2.h:58`).
  PINYIN_AMB_EN_ENG = (1 << 18),
  // `PINYIN_AMB_IN_ING = 1U << 19` (`pinyin_custom2.h:59`).
  PINYIN_AMB_IN_ING = (1 << 19),
  // `PINYIN_AMB_ALL = 0x3FFU << 10` (`pinyin_custom2.h:60`).
  PINYIN_AMB_ALL = (1023 << 10),
} PinyinAmbiguity2;

// `PinyinCorrection2` correct-pinyin bits the fork compiles against
typedef enum PinyinCorrection2 {
  // `PINYIN_CORRECT_GN_NG = 1U << 21` (`pinyin_custom2.h:71`).
  PINYIN_CORRECT_GN_NG = (1 << 21),
  // `PINYIN_CORRECT_MG_NG = 1U << 22` (`pinyin_custom2.h:72`).
  PINYIN_CORRECT_MG_NG = (1 << 22),
  // `PINYIN_CORRECT_IOU_IU = 1U << 23` (`pinyin_custom2.h:73`).
  PINYIN_CORRECT_IOU_IU = (1 << 23),
  // `PINYIN_CORRECT_UEI_UI = 1U << 24` (`pinyin_custom2.h:74`).
  PINYIN_CORRECT_UEI_UI = (1 << 24),
  // `PINYIN_CORRECT_UEN_UN = 1U << 25` (`pinyin_custom2.h:75`).
  PINYIN_CORRECT_UEN_UN = (1 << 25),
  // `PINYIN_CORRECT_UE_VE = 1U << 26` (`pinyin_custom2.h:76`).
  PINYIN_CORRECT_UE_VE = (1 << 26),
  // `PINYIN_CORRECT_V_U = 1U << 27` (`pinyin_custom2.h:77`).
  PINYIN_CORRECT_V_U = (1 << 27),
  // `PINYIN_CORRECT_ON_ONG = 1U << 28` (`pinyin_custom2.h:78`).
  PINYIN_CORRECT_ON_ONG = (1 << 28),
  // `PINYIN_CORRECT_ALL = 0xFFU << 21` (`pinyin_custom2.h:79`).
  PINYIN_CORRECT_ALL = (255 << 21),
} PinyinCorrection2;

// `PHRASE_INDEX_LIBRARIES` ids the fork compiles against
typedef enum PhraseIndexLibraries {
  // `ADDON_DICTIONARY = 5` (`novel_types.h:159`).
  ADDON_DICTIONARY = 5,
  // `NETWORK_DICTIONARY = 6` (`novel_types.h:160`).
  NETWORK_DICTIONARY = 6,
  // `USER_DICTIONARY = 7` (`novel_types.h:161`).
  USER_DICTIONARY = 7,
} PhraseIndexLibraries;

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

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

// Get the number of candidates.
bool pinyin_get_n_candidate(struct pinyin_instance_t *instance, guint *num);

// Get a candidate by index.
bool pinyin_get_candidate(struct pinyin_instance_t *instance,
                          guint index,
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
                            size_t offset,
                            struct lookup_candidate_t *candidate);

// Choose a predicted candidate.
bool pinyin_choose_predicted_candidate(struct pinyin_instance_t *instance,
                                       struct lookup_candidate_t *candidate);

// Train the current sentence with the given n-best index.
bool pinyin_train(struct pinyin_instance_t *instance, uint8_t _index);

// Set pinyin options on the context.
bool pinyin_set_options(struct pinyin_context_t *context, pinyin_option_t _options);

// Set the full pinyin scheme.
bool pinyin_set_full_pinyin_scheme(struct pinyin_context_t *context, int _scheme);

// Set the double pinyin scheme.
bool pinyin_set_double_pinyin_scheme(struct pinyin_context_t *context, int _scheme);

// Set the zhuyin scheme.
bool pinyin_set_zhuyin_scheme(struct pinyin_context_t *context, int _scheme);

// Load an addon phrase library by index.
bool pinyin_load_addon_phrase_library(struct pinyin_context_t *context, uint8_t _index);

// Mask out phrase tokens matching a pattern.
bool pinyin_mask_out(struct pinyin_context_t *context, uint32_t mask, uint32_t value);

// Create a new pinyin context.
struct pinyin_context_t *pinyin_init(const char *systemdir, const char *userdir);

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
bool pinyin_get_pinyin_offset(struct pinyin_instance_t *instance, size_t cursor, size_t *offset);

// Get the left offset from a lookup offset.
bool pinyin_get_left_pinyin_offset(struct pinyin_instance_t *instance, size_t offset, size_t *left);

// Get the right offset from a lookup offset.
bool pinyin_get_right_pinyin_offset(struct pinyin_instance_t *instance,
                                    size_t offset,
                                    size_t *right);

// Allocate a new pinyin instance from a context.
struct pinyin_instance_t *pinyin_alloc_instance(struct pinyin_context_t *context);

// Free a pinyin instance.
void pinyin_free_instance(struct pinyin_instance_t *instance);

// Reset the pinyin instance (clear parsing and sentence state).
bool pinyin_reset(struct pinyin_instance_t *instance);

// Begin adding phrases to an index.
struct import_iterator_t *pinyin_begin_add_phrases(struct pinyin_context_t *context, uint8_t index);

// Add a phrase/pinyin pair to the import iterator.
bool pinyin_iterator_add_phrase(struct import_iterator_t *iter,
                                const char *phrase,
                                const char *pinyin,
                                int count);

// End the import iterator, arm `m_modified`, and free it.
void pinyin_end_add_phrases(struct import_iterator_t *iter);

// Begin exporting phrases from an index.
struct export_iterator_t *pinyin_begin_get_phrases(struct pinyin_context_t *context, guint index);

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
size_t pinyin_parse_more_full_pinyins(struct pinyin_instance_t *instance, const char *pinyins);

// Parse multiple double pinyins.
size_t pinyin_parse_more_double_pinyins(struct pinyin_instance_t *instance, const char *pinyins);

// Parse multiple chewing (bopomofo) inputs.
size_t pinyin_parse_more_chewings(struct pinyin_instance_t *instance, const char *chewings);

// Get the parsed length of the input.
size_t pinyin_get_parsed_input_length(struct pinyin_instance_t *instance);

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
                                 const char *phrase,
                                 size_t offset,
                                 size_t *length);

// Guess candidates at the given offset with sort option. The offset may
// point one position past a zero ChewingKey (a separator, e.g. "'"); it is
// normalized back to the preceding separator internally before the lookup.
// The normalization applies to plain full-pinyin input only; the double,
// chewing and Luoma parse paths keep original-coordinate offsets. An offset
// one past a leading separator run cannot normalize, and an offset beyond
// the input's one-past-end position is out of range: either call returns
// false and clears the candidate list.
bool pinyin_guess_candidates(struct pinyin_instance_t *instance,
                             size_t offset,
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
bool pinyin_remember_user_input(struct pinyin_instance_t *instance, const char *phrase, int count);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* PINYIN_CAPI_H */
