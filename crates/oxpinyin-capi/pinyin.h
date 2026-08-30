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

// Opaque GArray (glib); callers pass a real glib array.
typedef struct _GArray GArray;

// Upstream carries these through the installed internal headers; here
// they are typedef'd from the same shapes the rest of this header uses.
typedef uint32_t phrase_token_t;
typedef GArray *ChewingKeyVector;

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

// Choose a candidate at an offset, returning the new cursor position. The
// cursor is the candidate's absolute end in the active parse mode's own
// coordinates — never past the parsed input, even when the caller offset
// sits one position past a separator run the candidate's span also covers
// (commit exactly at cursor == parsed length).
int pinyin_choose_candidate(struct pinyin_instance_t *instance,
                            size_t offset,
                            struct lookup_candidate_t *candidate);

// Choose a predicted candidate.
bool pinyin_choose_predicted_candidate(struct pinyin_instance_t *instance,
                                       struct lookup_candidate_t *candidate);

// Clear the constraint a prior choose pinned, by offset in the pinyin
// keys. A hit anywhere inside a forced run clears the whole run; false
// when the offset lands on no constraint or is out of range.
bool pinyin_clear_constraint(struct pinyin_instance_t *instance,
                             size_t offset);

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

// Unload an addon phrase library by index.
bool pinyin_unload_addon_phrase_library(struct pinyin_context_t *context, uint8_t _index);

// Mask out phrase tokens matching a pattern.
bool pinyin_mask_out(struct pinyin_context_t *context, uint32_t mask, uint32_t value);

// Create a new pinyin context.
struct pinyin_context_t *pinyin_init(const char *systemdir, const char *userdir);

// Finalize and free a pinyin context.
void pinyin_fini(struct pinyin_context_t *context);

// Save user data.
bool pinyin_save(struct pinyin_context_t *context);

// Get the pinyin key at an offset.
bool pinyin_get_pinyin_key(struct pinyin_instance_t *instance,
                           size_t offset,
                           struct ChewingKey **key);

// Get the pinyin key rest at an offset.
bool pinyin_get_pinyin_key_rest(struct pinyin_instance_t *instance,
                                size_t offset,
                                struct ChewingKeyRest **key_rest);

// Get the raw byte length of a pinyin key rest.
bool pinyin_get_pinyin_key_rest_length(struct pinyin_instance_t *instance,
                                       struct ChewingKeyRest *key_rest,
                                       uint16_t *length);

// Get the begin/end byte positions of a pinyin key rest.
bool pinyin_get_pinyin_key_rest_positions(struct pinyin_instance_t *instance,
                                          struct ChewingKeyRest *key_rest,
                                          uint16_t *begin,
                                          uint16_t *end);

// Render a pinyin key as its full spelling. Caller frees with g_free.
bool pinyin_get_pinyin_string(struct pinyin_instance_t *instance,
                              struct ChewingKey *key,
                              char **utf8_str);

// Render a pinyin key as its shengmu/yunmu pair. Caller frees with g_free.
bool pinyin_get_pinyin_strings(struct pinyin_instance_t *instance,
                               struct ChewingKey *key,
                               char **shengmu,
                               char **yunmu);

// Render a pinyin key as its Zhuyin spelling. Caller frees with g_free.
bool pinyin_get_zhuyin_string(struct pinyin_instance_t *instance,
                              struct ChewingKey *key,
                              char **utf8_str);

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

// Get the pinyin context from the pinyin instance.
struct pinyin_context_t *pinyin_get_context(struct pinyin_instance_t *instance);

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

// Parse one full pinyin into a key.
bool pinyin_parse_full_pinyin(struct pinyin_instance_t *instance,
                              const char *onepinyin,
                              ChewingKey *onekey);

// Parse one double pinyin into a key.
bool pinyin_parse_double_pinyin(struct pinyin_instance_t *instance,
                                const char *onepinyin,
                                ChewingKey *onekey);

// Parse one chewing (bopomofo) input into a key.
bool pinyin_parse_chewing(struct pinyin_instance_t *instance,
                          const char *onechewing,
                          ChewingKey *onekey);

// Get the zhuyin string of a chewing key.
bool pinyin_get_zhuyin_string(struct pinyin_instance_t *instance,
                              ChewingKey *key,
                              gchar **utf8_str);

// Get the pinyin string of a chewing key.
bool pinyin_get_pinyin_string(struct pinyin_instance_t *instance,
                              ChewingKey *key,
                              gchar **utf8_str);

// Get the luoma pinyin string of a chewing key.
bool pinyin_get_luoma_pinyin_string(struct pinyin_instance_t *instance,
                                    ChewingKey *key,
                                    gchar **utf8_str);

// Get the secondary zhuyin string of a chewing key.
bool pinyin_get_secondary_zhuyin_string(struct pinyin_instance_t *instance,
                                        ChewingKey *key,
                                        gchar **utf8_str);

// Get the shengmu and yunmu strings of a chewing key.
bool pinyin_get_pinyin_strings(struct pinyin_instance_t *instance,
                               ChewingKey *key,
                               gchar **shengmu,
                               gchar **yunmu);

// Whether a chewing key carries no middle and no final.
bool pinyin_get_pinyin_is_incomplete(struct pinyin_instance_t *instance, ChewingKey *key);

// Check whether an input key is in the current chewing keyboard scheme.
bool pinyin_in_chewing_keyboard(struct pinyin_instance_t *instance, char _key, gchar ***symbols);

// Guess a sentence from saved pinyin keys.
bool pinyin_guess_sentence(struct pinyin_instance_t *instance);

// Guess predicted candidates with punctuations after a prefix.
bool pinyin_guess_predicted_candidates_with_punctuations(struct pinyin_instance_t *instance,
                                                         const char *_prefix);

// Guess a sentence seeded with prefix tokens.
bool pinyin_guess_sentence_with_prefix(struct pinyin_instance_t *instance, const char *_prefix);

// Guess predicted candidates for a prefix (plain variant).
bool pinyin_guess_predicted_candidates(struct pinyin_instance_t *instance, const char *_prefix);

// Get a sentence string from the instance (n-best variant).
bool pinyin_get_sentence(struct pinyin_instance_t *instance, uint8_t _index, char **sentence);

// Segment an arbitrary sentence string into phrase tokens.
bool pinyin_phrase_segment(struct pinyin_instance_t *instance, const char *sentence);

// Get the number of phrase tokens in the phrase result.
bool pinyin_get_n_phrase(struct pinyin_instance_t *instance, guint *num);

// Get the phrase token at an index of the phrase result.
bool pinyin_get_phrase_token(struct pinyin_instance_t *instance, unsigned int _index,
                             phrase_token_t *token);

// Look up the phrase tokens stored for an exact phrase string.
bool pinyin_lookup_tokens(struct pinyin_instance_t *instance, const char *_phrase,
                          GArray *tokenarray);

// Get the phrase text of a token.
bool pinyin_token_get_phrase(struct pinyin_instance_t *instance, phrase_token_t token,
                             guint *len, gchar **utf8_str);

// Get the number of pronunciations of a token.
bool pinyin_token_get_n_pronunciation(struct pinyin_instance_t *instance,
                                      phrase_token_t token, guint *num);

// Get the nth pronunciation of a token as chewing keys.
bool pinyin_token_get_nth_pronunciation(struct pinyin_instance_t *instance,
                                        phrase_token_t token, guint nth,
                                        ChewingKeyVector keys);

// Get the unigram frequency of a token.
bool pinyin_token_get_unigram_frequency(struct pinyin_instance_t *instance,
                                        phrase_token_t token, guint *freq);

// Add a unigram-frequency delta to a token.
bool pinyin_token_add_unigram_frequency(struct pinyin_instance_t *instance,
                                        phrase_token_t token, guint delta);

// Load a default phrase library by index.
bool pinyin_load_phrase_library(struct pinyin_context_t *context, uint8_t _index);

// Unload a default phrase library by index.
bool pinyin_unload_phrase_library(struct pinyin_context_t *context, uint8_t _index);

// Get character offset from a lookup byte offset within a sentence.
bool pinyin_get_character_offset(struct pinyin_instance_t *instance,
                                 const char *phrase,
                                 size_t offset,
                                 size_t *length);

// Guess candidates at the given offset with sort option. The offset lives
// in the active parse mode's own coordinates. Where a composition can hold
// a zero ChewingKey (a separator, e.g. "'" — full pinyin and the
// Luoma/secondary-zhuyin schemes), an offset one position past the
// separator run is normalized back to the run's first byte before the
// lookup. Double pinyin and the zhuyin keyboards admit no zero key ("'" is
// not a scheme key there, or is a content symbol such as Eten's), so no
// normalization applies. An offset one past a leading separator run cannot
// normalize, and an offset beyond the parsed input's one-past-end position
// is out of range: either call returns false and clears the candidate list
// (C++ libpinyin asserts, or reads its matrix out of bounds, instead).
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
