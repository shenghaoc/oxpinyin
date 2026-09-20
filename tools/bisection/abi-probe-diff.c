/*
 * abi-probe-diff.c — whole-surface probe over the full exported pinyin
 * ABI (compatibility-policy §(e): every exported symbol needs a probe
 * that asserts its whole observable surface — return status, out-params
 * and the data they point to, written lengths, and handle state).
 *
 * The driver resolves all 79 exported pinyin_* symbols and refuses to
 * run when any one of them is missing from the library, so a run also
 * proves the export surface is present. It then walks every symbol's
 * observable surface in deterministic phases and prints one label=value
 * row per observation; the runner diffs two such logs (pin vs oxpinyin)
 * line by line.
 *
 * Oracle caller contracts (uncovered-surface-differentials.md
 * § Reproduction) are honoured throughout:
 *   - pinyin_get_sentence is asked only for proved indices — row 0
 *     after a successful guess, and an NBEST row's own
 *     pinyin_get_candidate_nbest_index value;
 *   - predicted candidates never reach pinyin_choose_candidate; only a
 *     PREDICTED_PREFIX (type 5) row goes to
 *     pinyin_choose_predicted_candidate;
 *   - double scheme 30 and zhuyin keyboard 7 (both abort() at the pin)
 *     are never set — the out-of-enum double setter probe uses 99/-1,
 *     which the pin answers with the row-5b half-mutation lie, not a
 *     crash;
 *   - pinyin_guess_sentence runs before pinyin_train.
 *
 * Usage: ./abi-probe-diff <path-to-so> <systemdir> <sort-hex>
 *
 * <sort-hex> is the sort_option_t word every pinyin_guess_candidates
 * call uses (hex, e.g. 1e). The first run of this probe hard-coded 0
 * and its divergence was caused by exactly that: with no sort bits the
 * pin prepends longer AND sentence candidates (pinyin.cpp:2292-2296)
 * and its comparator has no keys (:1678-1709), so the list is unsorted
 * — a word no runner had ever driven. The default parity word is 0x1e
 * (longer suppressed, sentence kept, all three keys), the word every
 * other differential passes; the ibus presets 0x14/0x1c and the raw 0
 * are the consumer-reachable words the sort-option investigation
 * drives.
 *
 * Exit codes: 0 = the walk completed; 1 = a symbol is missing, a
 * handle could not be created, or an accessor failed where the walk
 * cannot continue. Divergence is the runner's verdict (it diffs two
 * logs), never this program's.
 */

#define _POSIX_C_SOURCE 200809L
#include <dirent.h>
#include <dlfcn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef void pinyin_context_t;
typedef void pinyin_instance_t;
typedef void lookup_candidate_t;
typedef void import_iterator_t;
typedef void export_iterator_t;
typedef void bigram_export_iterator_t;
typedef void ChewingKey;
typedef void ChewingKeyRest;
typedef uint32_t guint;
typedef uint32_t guint32;
typedef uint16_t guint16;
typedef uint8_t guint8;
typedef int gint;
typedef uint32_t phrase_token_t;

/* The parity word every consumer's set_options call carries
 * (IS_PINYIN | PINYIN_INCOMPLETE | USE_DIVIDED_TABLE |
 * USE_RESPLIT_TABLE). */
#define PARITY_WORD 0x18au

/* USER_DICTIONARY's index (novel_types.h) — bisect.c's import/export
 * convention — and the mask/value pair that selects exactly the user
 * dictionary's token range (train-diff.c's TRAINDIFF_MASK=user). */
#define USER_DICT_INDEX 7u
#define USER_TOKEN_MASK 0x07000000u

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef pinyin_instance_t *(*fn_alloc)(pinyin_context_t *);
typedef void (*fn_free)(pinyin_instance_t *);
typedef bool (*fn_set_options)(pinyin_context_t *, guint);
typedef bool (*fn_set_double)(pinyin_context_t *, int);
typedef bool (*fn_set_zhuyin)(pinyin_context_t *, int);
typedef bool (*fn_load_addon)(pinyin_context_t *, guint8);
typedef bool (*fn_unload_addon)(pinyin_context_t *, guint8);
typedef bool (*fn_save)(pinyin_context_t *);
typedef bool (*fn_mask_out)(pinyin_context_t *, phrase_token_t,
                            phrase_token_t);
typedef bool (*fn_remember)(pinyin_instance_t *, const char *, gint);
typedef size_t (*fn_parse)(pinyin_instance_t *, const char *);
typedef size_t (*fn_parsed_len)(pinyin_instance_t *);
typedef bool (*fn_guess_sentence)(pinyin_instance_t *);
typedef bool (*fn_guess_candidates)(pinyin_instance_t *, size_t, guint);
typedef bool (*fn_predict)(pinyin_instance_t *, const char *);
typedef bool (*fn_get_sentence)(pinyin_instance_t *, guint8, char **);
typedef bool (*fn_get_char_offset)(pinyin_instance_t *, const char *,
                                   size_t, size_t *);
typedef bool (*fn_n)(pinyin_instance_t *, guint *);
typedef bool (*fn_getc)(pinyin_instance_t *, guint, lookup_candidate_t **);
typedef bool (*fn_gettype)(pinyin_instance_t *, lookup_candidate_t *, int *);
typedef bool (*fn_getstr)(pinyin_instance_t *, lookup_candidate_t *,
                          const char **);
typedef bool (*fn_nbest)(pinyin_instance_t *, lookup_candidate_t *, guint8 *);
typedef bool (*fn_is_user)(pinyin_instance_t *, lookup_candidate_t *);
typedef bool (*fn_remove_user)(pinyin_instance_t *, lookup_candidate_t *);
typedef int (*fn_choose)(pinyin_instance_t *, size_t, lookup_candidate_t *);
typedef bool (*fn_choose_pred)(pinyin_instance_t *, lookup_candidate_t *);
typedef bool (*fn_train)(pinyin_instance_t *, guint8);
typedef bool (*fn_reset)(pinyin_instance_t *);
typedef bool (*fn_aux)(pinyin_instance_t *, size_t, char **);
typedef bool (*fn_key)(pinyin_instance_t *, size_t, ChewingKey **);
typedef bool (*fn_key_rest)(pinyin_instance_t *, size_t, ChewingKeyRest **);
typedef bool (*fn_key_rest_pos)(pinyin_instance_t *, ChewingKeyRest *,
                                guint16 *, guint16 *);
typedef bool (*fn_key_rest_len)(pinyin_instance_t *, ChewingKeyRest *,
                                guint16 *);
typedef bool (*fn_offset)(pinyin_instance_t *, size_t, size_t *);
typedef bool (*fn_pinyin_string)(pinyin_instance_t *, ChewingKey *, char **);
typedef bool (*fn_pinyin_strings)(pinyin_instance_t *, ChewingKey *, char **,
                                  char **);
typedef bool (*fn_zhuyin_string)(pinyin_instance_t *, ChewingKey *, char **);
typedef bool (*fn_clear_constraint)(pinyin_instance_t *, size_t);
typedef bool (*fn_in_chewing)(pinyin_instance_t *, char, char ***);
typedef struct { char *data; guint len; } GArrayPub;
typedef pinyin_context_t *(*fn_get_context)(pinyin_instance_t *);
typedef bool (*fn_set_full_scheme)(pinyin_context_t *, int);
typedef bool (*fn_parse_one)(pinyin_instance_t *, const char *, ChewingKey *);
typedef bool (*fn_is_incomplete)(pinyin_instance_t *, ChewingKey *);
typedef bool (*fn_get_string)(pinyin_instance_t *, ChewingKey *, char **);
typedef bool (*fn_segment)(pinyin_instance_t *, const char *);
typedef bool (*fn_n_phrase)(pinyin_instance_t *, guint *);
typedef bool (*fn_phrase_token)(pinyin_instance_t *, guint, phrase_token_t *);
typedef bool (*fn_sentence_prefix)(pinyin_instance_t *, const char *);
typedef bool (*fn_predict_plain)(pinyin_instance_t *, const char *);
typedef bool (*fn_lookup_tokens)(pinyin_instance_t *, const char *, void *);
typedef bool (*fn_token_phrase)(pinyin_instance_t *, phrase_token_t, guint *, char **);
typedef bool (*fn_token_n_pron)(pinyin_instance_t *, phrase_token_t, guint *);
typedef bool (*fn_token_nth_pron)(pinyin_instance_t *, phrase_token_t, guint, void *);
typedef bool (*fn_token_unigram)(pinyin_instance_t *, phrase_token_t, guint *);
typedef bool (*fn_token_add)(pinyin_instance_t *, phrase_token_t, guint);
typedef bool (*fn_load_lib)(pinyin_context_t *, guint8);
typedef bool (*fn_unload_lib)(pinyin_context_t *, guint8);
typedef import_iterator_t *(*fn_begin_add)(pinyin_context_t *, guint8);
typedef bool (*fn_add_phrase)(import_iterator_t *, const char *, const char *,
                              gint);
typedef void (*fn_end_add)(import_iterator_t *);
typedef export_iterator_t *(*fn_begin_get)(pinyin_context_t *, guint);
typedef bool (*fn_has_next)(export_iterator_t *);
typedef bool (*fn_get_next)(export_iterator_t *, char **, char **, gint *);
typedef void (*fn_end_get)(export_iterator_t *);
typedef bigram_export_iterator_t *(*fn_begin_bigram)(pinyin_context_t *);
typedef bool (*fn_bigram_has_next)(bigram_export_iterator_t *);
typedef bool (*fn_bigram_get_next)(bigram_export_iterator_t *, char **,
                                   char **, gint *);
typedef void (*fn_end_bigram)(bigram_export_iterator_t *);

struct api {
    fn_init init;
    fn_fini fini;
    fn_alloc alloc;
    fn_free free_inst;
    fn_set_options set_options;
    fn_set_double set_double;
    fn_set_zhuyin set_zhuyin;
    fn_load_addon load_addon;
    fn_unload_addon unload_addon;
    fn_save save;
    fn_mask_out mask_out;
    fn_remember remember;
    fn_parse parse_full, parse_double, parse_chewing;
    fn_parsed_len parsed_len;
    fn_guess_sentence guess_sentence;
    fn_guess_candidates guess_candidates;
    fn_predict predict;
    fn_get_sentence get_sentence;
    fn_get_char_offset char_offset;
    fn_n n_cand;
    fn_getc get_cand;
    fn_gettype get_type;
    fn_getstr get_str;
    fn_nbest nbest_index;
    fn_is_user is_user;
    fn_remove_user remove_user;
    fn_choose choose;
    fn_choose_pred choose_pred;
    fn_train train;
    fn_reset reset;
    fn_aux full_aux, double_aux, chewing_aux;
    fn_key get_key;
    fn_key_rest get_key_rest;
    fn_key_rest_pos key_rest_pos;
    fn_key_rest_len key_rest_len;
    fn_offset get_offset, left_offset, right_offset;
    fn_pinyin_string pinyin_string;
    fn_pinyin_strings pinyin_strings;
    fn_zhuyin_string zhuyin_string;
    fn_clear_constraint clear_constraint;
    fn_in_chewing in_chewing_keyboard;
    fn_get_context get_context;
    fn_set_full_scheme set_full_scheme;
    fn_parse_one parse_full_pinyin, parse_double_pinyin, parse_chewing_one;
    fn_is_incomplete is_incomplete;
    fn_get_string luoma_string, secondary_zhuyin_string;
    fn_segment phrase_segment;
    fn_n_phrase n_phrase;
    fn_phrase_token phrase_token;
    fn_sentence_prefix sentence_with_prefix;
    fn_predict_plain predict_plain;
    fn_lookup_tokens lookup_tokens;
    fn_token_phrase token_phrase;
    fn_token_n_pron token_n_pron;
    fn_token_nth_pron token_nth_pron;
    fn_token_unigram token_unigram;
    fn_token_add token_add;
    fn_load_lib load_lib;
    fn_unload_lib unload_lib;
    fn_begin_add begin_add;
    fn_add_phrase add_phrase;
    fn_end_add end_add;
    fn_begin_get begin_get;
    fn_has_next has_next;
    fn_get_next get_next;
    fn_end_get end_get;
    fn_begin_bigram begin_bigram;
    fn_bigram_has_next bigram_has_next;
    fn_bigram_get_next bigram_get_next;
    fn_end_bigram end_bigram;
};

typedef void (*fn_g_free)(void *);
typedef void *(*fn_g_array_new)(int, int, unsigned int);
typedef void (*fn_g_array_free)(void *, int);
typedef void (*fn_g_strfreev)(char **);
static fn_g_free g_free_fn;
static fn_g_array_new g_array_new_fn;
static fn_g_array_free g_array_free_fn;
static fn_g_strfreev g_strfreev_fn;

static void *must(void *handle, const char *name) {
    void *symbol = dlsym(handle, name);
    if (!symbol) {
        fprintf(stderr, "MISSING: %s\n", name);
        exit(1);
    }
    return symbol;
}

/* Removes `dir` and everything under it, so no exit path leaks the
 * mkdtemp userdir even when the engine already wrote user data into it. */
static void rm_rf(const char *dir) {
    DIR *d = opendir(dir);
    if (d) {
        struct dirent *entry;
        while ((entry = readdir(d)) != NULL) {
            if (strcmp(entry->d_name, ".") == 0 ||
                strcmp(entry->d_name, "..") == 0)
                continue;
            char path[4096];
            if (snprintf(path, sizeof(path), "%s/%s", dir, entry->d_name) >=
                (int)sizeof(path))
                continue;
            unlink(path); /* the store writes regular files only */
        }
        closedir(d);
    }
    rmdir(dir);
}

/* ChewingKey / ChewingKeyRest are 32-bit packed structs at the pin
 * (key-surface-diff.c's convention): print the packed word. */
static guint packed(const void *p) {
    guint v = 0;
    memcpy(&v, p, sizeof(v));
    return v;
}

static const char *yesno(bool b) { return b ? "true" : "false"; }

/* Candidate rows: type, nbest out-param (ret AND value), text, and the
 * user-candidate predicate. The first `limit` rows of the current list. */
static void dump_rows(const struct api *s, pinyin_instance_t *inst,
                      const char *tag, guint limit) {
    guint n = 0;
    bool got = s->n_cand(inst, &n);
    printf("%s:n_cand=%s n=%u\n", tag, yesno(got), n);
    if (!got)
        return;
    if (n > limit)
        n = limit;
    for (guint i = 0; i < n; i++) {
        lookup_candidate_t *c = NULL;
        bool ok = s->get_cand(inst, i, &c) && c;
        printf("%s:cand[%u]=%s", tag, i, yesno(ok));
        if (!ok) {
            printf("\n");
            continue;
        }
        int type = -1;
        bool t_ok = s->get_type(inst, c, &type);
        const char *text = NULL;
        bool s_ok = s->get_str(inst, c, &text) && text;
        /* The pin asserts NBEST_MATCH inside get_candidate_nbest_index
         * (pinyin.cpp:2883) — ask only the rows whose type proves it. */
        guint8 nbest = 255;
        bool b_ok = false;
        bool b_asked = (t_ok && type == 1 /* NBEST_MATCH_CANDIDATE */);
        if (b_asked)
            b_ok = s->nbest_index(inst, c, &nbest);
        printf(" type=%s/%d text=%s/%s nbest=%s/%s/%u is_user=%s",
               yesno(t_ok), type, yesno(s_ok), s_ok ? text : "(null)",
               b_asked ? "asked" : "skipped", yesno(b_ok), nbest,
               yesno(s->is_user(inst, c)));
        printf("\n");
    }
}

int main(int argc, char **argv) {
    if (argc != 4) {
        fprintf(stderr, "usage: %s <so> <systemdir> <sort-hex>\n", argv[0]);
        return 1;
    }
    /* Reject malformed words outright: strtoul silently accepts a
     * valid prefix and answers 0 for digit-less input, and a sort
     * word that is not what the operator typed would score the probe
     * at the wrong word with no visible sign (the same refusal the
     * option-sweep driver makes for OPTION_SWEEP_SORT). */
    char *sort_end = NULL;
    unsigned long sort_parsed = strtoul(argv[3], &sort_end, 16);
    if (sort_end == argv[3] || *sort_end != '\0') {
        fprintf(stderr, "sort word %s is not a hex word\n", argv[3]);
        return 1;
    }
    guint sort_word = (guint)sort_parsed;
    printf("sort=0x%02x\n", sort_word);
    void *handle = dlopen(argv[1], RTLD_NOW);
    if (!handle) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }
    void *glib = dlopen("libglib-2.0.so.0", RTLD_NOW);
    if (glib) {
        g_free_fn = (fn_g_free)dlsym(glib, "g_free");
        g_strfreev_fn = (fn_g_strfreev)dlsym(glib, "g_strfreev");
        g_array_new_fn = (fn_g_array_new)dlsym(glib, "g_array_new");
        g_array_free_fn = (fn_g_array_free)dlsym(glib, "g_array_free");
    }
    if (!g_free_fn || !g_strfreev_fn || !g_array_new_fn || !g_array_free_fn) {
        fprintf(stderr, "fatal: g_free unavailable\n");
        return 1;
    }

    struct api s;
#define LOAD(field, fntype, name) s.field = (fntype)must(handle, name)
    LOAD(init, fn_init, "pinyin_init");
    LOAD(fini, fn_fini, "pinyin_fini");
    LOAD(alloc, fn_alloc, "pinyin_alloc_instance");
    LOAD(free_inst, fn_free, "pinyin_free_instance");
    LOAD(set_options, fn_set_options, "pinyin_set_options");
    LOAD(set_double, fn_set_double, "pinyin_set_double_pinyin_scheme");
    LOAD(set_zhuyin, fn_set_zhuyin, "pinyin_set_zhuyin_scheme");
    LOAD(load_addon, fn_load_addon, "pinyin_load_addon_phrase_library");
    LOAD(unload_addon, fn_unload_addon, "pinyin_unload_addon_phrase_library");
    LOAD(save, fn_save, "pinyin_save");
    LOAD(mask_out, fn_mask_out, "pinyin_mask_out");
    LOAD(remember, fn_remember, "pinyin_remember_user_input");
    LOAD(parse_full, fn_parse, "pinyin_parse_more_full_pinyins");
    LOAD(parse_double, fn_parse, "pinyin_parse_more_double_pinyins");
    LOAD(parse_chewing, fn_parse, "pinyin_parse_more_chewings");
    LOAD(parsed_len, fn_parsed_len, "pinyin_get_parsed_input_length");
    LOAD(guess_sentence, fn_guess_sentence, "pinyin_guess_sentence");
    LOAD(guess_candidates, fn_guess_candidates, "pinyin_guess_candidates");
    LOAD(predict, fn_predict, "pinyin_guess_predicted_candidates_with_punctuations");
    LOAD(get_sentence, fn_get_sentence, "pinyin_get_sentence");
    LOAD(char_offset, fn_get_char_offset, "pinyin_get_character_offset");
    LOAD(n_cand, fn_n, "pinyin_get_n_candidate");
    LOAD(get_cand, fn_getc, "pinyin_get_candidate");
    LOAD(get_type, fn_gettype, "pinyin_get_candidate_type");
    LOAD(get_str, fn_getstr, "pinyin_get_candidate_string");
    LOAD(nbest_index, fn_nbest, "pinyin_get_candidate_nbest_index");
    LOAD(is_user, fn_is_user, "pinyin_is_user_candidate");
    LOAD(remove_user, fn_remove_user, "pinyin_remove_user_candidate");
    LOAD(choose, fn_choose, "pinyin_choose_candidate");
    LOAD(choose_pred, fn_choose_pred, "pinyin_choose_predicted_candidate");
    LOAD(train, fn_train, "pinyin_train");
    LOAD(reset, fn_reset, "pinyin_reset");
    LOAD(full_aux, fn_aux, "pinyin_get_full_pinyin_auxiliary_text");
    LOAD(double_aux, fn_aux, "pinyin_get_double_pinyin_auxiliary_text");
    LOAD(chewing_aux, fn_aux, "pinyin_get_chewing_auxiliary_text");
    LOAD(get_key, fn_key, "pinyin_get_pinyin_key");
    LOAD(get_key_rest, fn_key_rest, "pinyin_get_pinyin_key_rest");
    LOAD(key_rest_pos, fn_key_rest_pos, "pinyin_get_pinyin_key_rest_positions");
    LOAD(key_rest_len, fn_key_rest_len, "pinyin_get_pinyin_key_rest_length");
    LOAD(get_offset, fn_offset, "pinyin_get_pinyin_offset");
    LOAD(left_offset, fn_offset, "pinyin_get_left_pinyin_offset");
    LOAD(right_offset, fn_offset, "pinyin_get_right_pinyin_offset");
    LOAD(pinyin_string, fn_pinyin_string, "pinyin_get_pinyin_string");
    LOAD(pinyin_strings, fn_pinyin_strings, "pinyin_get_pinyin_strings");
    LOAD(zhuyin_string, fn_zhuyin_string, "pinyin_get_zhuyin_string");
    LOAD(clear_constraint, fn_clear_constraint, "pinyin_clear_constraint");
    LOAD(in_chewing_keyboard, fn_in_chewing, "pinyin_in_chewing_keyboard");
    LOAD(get_context, fn_get_context, "pinyin_get_context");
    LOAD(set_full_scheme, fn_set_full_scheme, "pinyin_set_full_pinyin_scheme");
    LOAD(parse_full_pinyin, fn_parse_one, "pinyin_parse_full_pinyin");
    LOAD(parse_double_pinyin, fn_parse_one, "pinyin_parse_double_pinyin");
    LOAD(parse_chewing_one, fn_parse_one, "pinyin_parse_chewing");
    LOAD(is_incomplete, fn_is_incomplete, "pinyin_get_pinyin_is_incomplete");
    LOAD(luoma_string, fn_get_string, "pinyin_get_luoma_pinyin_string");
    LOAD(secondary_zhuyin_string, fn_get_string, "pinyin_get_secondary_zhuyin_string");
    LOAD(phrase_segment, fn_segment, "pinyin_phrase_segment");
    LOAD(n_phrase, fn_n_phrase, "pinyin_get_n_phrase");
    LOAD(phrase_token, fn_phrase_token, "pinyin_get_phrase_token");
    LOAD(sentence_with_prefix, fn_sentence_prefix, "pinyin_guess_sentence_with_prefix");
    LOAD(predict_plain, fn_predict_plain, "pinyin_guess_predicted_candidates");
    LOAD(lookup_tokens, fn_lookup_tokens, "pinyin_lookup_tokens");
    LOAD(token_phrase, fn_token_phrase, "pinyin_token_get_phrase");
    LOAD(token_n_pron, fn_token_n_pron, "pinyin_token_get_n_pronunciation");
    LOAD(token_nth_pron, fn_token_nth_pron, "pinyin_token_get_nth_pronunciation");
    LOAD(token_unigram, fn_token_unigram, "pinyin_token_get_unigram_frequency");
    LOAD(token_add, fn_token_add, "pinyin_token_add_unigram_frequency");
    LOAD(load_lib, fn_load_lib, "pinyin_load_phrase_library");
    LOAD(unload_lib, fn_unload_lib, "pinyin_unload_phrase_library");
    LOAD(begin_add, fn_begin_add, "pinyin_begin_add_phrases");
    LOAD(add_phrase, fn_add_phrase, "pinyin_iterator_add_phrase");
    LOAD(end_add, fn_end_add, "pinyin_end_add_phrases");
    LOAD(begin_get, fn_begin_get, "pinyin_begin_get_phrases");
    LOAD(has_next, fn_has_next, "pinyin_iterator_has_next_phrase");
    LOAD(get_next, fn_get_next, "pinyin_iterator_get_next_phrase");
    LOAD(end_get, fn_end_get, "pinyin_end_get_phrases");
    LOAD(begin_bigram, fn_begin_bigram, "pinyin_begin_get_bigram_phrases");
    LOAD(bigram_has_next, fn_bigram_has_next, "pinyin_bigram_iterator_has_next_phrase");
    LOAD(bigram_get_next, fn_bigram_get_next, "pinyin_bigram_iterator_get_next_phrase");
    LOAD(end_bigram, fn_end_bigram, "pinyin_end_get_bigram_phrases");
#undef LOAD
    printf("resolved=79\n");

    char userdir[] = "/tmp/unionprobe-user-XXXXXX";
    if (!mkdtemp(userdir)) {
        perror("mkdtemp");
        return 1;
    }

    /* ── setup: context lifecycle and configuration ─────────────────── */
    printf("=== phase: setup ===\n");
    pinyin_context_t *ctx = s.init(argv[2], userdir);
    printf("init=%s\n", ctx ? "ok" : "NULL");
    if (!ctx) {
        rm_rf(userdir);
        return 1;
    }
    printf("set_options(0x18a)=%s\n", yesno(s.set_options(ctx, PARITY_WORD)));
    printf("set_double_scheme(1/ZRM)=%s\n", yesno(s.set_double(ctx, 1)));
    printf("set_zhuyin_scheme(1/STANDARD)=%s\n", yesno(s.set_zhuyin(ctx, 1)));
    printf("load_addon(0)=%s\n", yesno(s.load_addon(ctx, 0)));
    printf("load_addon(250/out-of-range)=%s\n", yesno(s.load_addon(ctx, 250)));
    /* No out-of-range unload: the pin asserts (pinyin.cpp:499,
     * `index < PHRASE_INDEX_LIBRARY_COUNT`) — same never-send class as
     * double scheme 30 and zhuyin keyboard 7. */
    printf("unload_addon(0)=%s\n", yesno(s.unload_addon(ctx, 0)));
    printf("load_addon(0/reload)=%s\n", yesno(s.load_addon(ctx, 0)));

    /* ── import: the user-dictionary write seam ─────────────────────── */
    printf("=== phase: import ===\n");
    import_iterator_t *imp = s.begin_add(ctx, USER_DICT_INDEX);
    printf("begin_add(%u)=%s\n", USER_DICT_INDEX, imp ? "ok" : "NULL");
    if (imp) {
        printf("add(你好/ni'hao/5)=%s\n",
               yesno(s.add_phrase(imp, "你好", "ni'hao", 5)));
        printf("add(你好世界/ni'hao'shi'jie/9)=%s\n",
               yesno(s.add_phrase(imp, "你好世界", "ni'hao'shi'jie", 9)));
        printf("add(测试/ce'shi/3)=%s\n",
               yesno(s.add_phrase(imp, "测试", "ce'shi", 3)));
        s.end_add(imp);
        printf("end_add=ok\n");
    }
    printf("save(after-import)=%s\n", yesno(s.save(ctx)));

    pinyin_instance_t *inst = s.alloc(ctx);
    printf("alloc_instance=%s\n", inst ? "ok" : "NULL");
    if (!inst) {
        s.fini(ctx);
        rm_rf(userdir);
        return 1;
    }

    /* ── full-pinyin surface ────────────────────────────────────────── */
    printf("=== phase: full ===\n");
    static const char *const full_inputs[] = {"nihaoshijie", "xian"};
    for (unsigned fi = 0; fi < 2; fi++) {
        const char *in = full_inputs[fi];
        size_t consumed = s.parse_full(inst, in);
        printf("full(%s): consumed=%zu parsed=%zu\n", in, consumed,
               s.parsed_len(inst));
        for (size_t c = 0; c <= consumed; c++) {
            char *aux = NULL;
            bool ok = s.full_aux(inst, c, &aux);
            printf("full_aux(%s,%zu)=%s text=%s\n", in, c, yesno(ok),
                   ok && aux ? aux : "(null)");
            if (aux)
                g_free_fn(aux);
        }
        bool guessed = s.guess_sentence(inst);
        printf("guess_sentence(%s)=%s\n", in, yesno(guessed));
        char *sent = NULL;
        /* row 0 is proved only by a successful guess: an empty result
         * set answers false for every index, so the query is skipped
         * when the guess failed. */
        bool sok = guessed && s.get_sentence(inst, 0, &sent);
        printf("sentence0(%s)=%s text=%s\n", in, yesno(sok),
               sok && sent ? sent : "(null)");
        if (sok && sent) {
            size_t len = 0;
            bool cok = s.char_offset(inst, sent, 0, &len);
            printf("char_offset(%s,0)=%s len=%zu\n", in, yesno(cok), len);
        }
        if (sent)
            g_free_fn(sent);
        printf("guess_candidates(%s,0)=%s\n", in,
               yesno(s.guess_candidates(inst, 0, sort_word)));
        char tag[64];
        snprintf(tag, sizeof(tag), "full(%s)", in);
        dump_rows(&s, inst, tag, 10);

        /* key surface at offset 0 of this parse */
        ChewingKey *key = NULL;
        bool kok = s.get_key(inst, 0, &key) && key;
        printf("key(%s,0)=%s", in, yesno(kok));
        if (kok) {
            printf(" packed=%08x", packed(key));
            char *str = NULL;
            bool p1 = s.pinyin_string(inst, key, &str);
            printf(" pinyin_string=%s/%s", yesno(p1), p1 && str ? str : "(null)");
            if (str)
                g_free_fn(str);
            char *sh = NULL, *yu = NULL;
            bool p2 = s.pinyin_strings(inst, key, &sh, &yu);
            printf(" pinyin_strings=%s/%s+%s", yesno(p2), p2 && sh ? sh : "(null)",
                   p2 && yu ? yu : "(null)");
            if (sh)
                g_free_fn(sh);
            if (yu)
                g_free_fn(yu);
            char *zy = NULL;
            bool p3 = s.zhuyin_string(inst, key, &zy);
            printf(" zhuyin_string=%s/%s", yesno(p3), p3 && zy ? zy : "(null)");
            if (zy)
                g_free_fn(zy);
        }
        printf("\n");
        ChewingKeyRest *rest = NULL;
        bool rok = s.get_key_rest(inst, 0, &rest) && rest;
        printf("key_rest(%s,0)=%s", in, yesno(rok));
        if (rok) {
            guint16 b = 0, e = 0, l = 0;
            bool p1 = s.key_rest_pos(inst, rest, &b, &e);
            bool p2 = s.key_rest_len(inst, rest, &l);
            printf(" pos=%s/%u:%u len=%s/%u", yesno(p1), b, e, yesno(p2), l);
        }
        printf("\n");

        /* Cursor surface: every byte cursor through get_pinyin_offset. */
        for (size_t c = 0; c <= consumed; c++) {
            size_t off = (size_t)-1;
            bool o1 = s.get_offset(inst, c, &off);
            printf("offsets(%s,%zu)=%s/%zu\n", in, c, yesno(o1),
                   o1 ? off : (size_t)-1);
        }
        /* Word-level left/right moves at the smoke-proved-safe cursors
         * ONLY (uncovered-surface-diff.c's phase-D set): the pin runs a
         * second _check_offset on the offset it computes
         * (pinyin.cpp:3055/:3090) and asserts for tail cursors of the
         * composition — a pin landmine upstream later turned into
         * `return false` (libpinyin 95e3af7, after the pin). */
        if (strcmp(in, "nihaoshijie") == 0) {
            static const size_t probes[] = {0, 2, 5, 8};
            for (size_t i = 0; i < sizeof(probes) / sizeof(probes[0]); i++) {
                size_t off = (size_t)-1;
                if (!s.get_offset(inst, probes[i], &off)) {
                    printf("left_right(%s,@%zu) off=false\n", in, probes[i]);
                    continue;
                }
                size_t left = (size_t)-1, right = (size_t)-1;
                bool o2 = s.left_offset(inst, off, &left);
                bool o3 = s.right_offset(inst, off, &right);
                printf("left_right(%s,@%zu) off=%zu left=%s/%zu right=%s/%zu\n",
                       in, probes[i], off, yesno(o2), o2 ? left : (size_t)-1,
                       yesno(o3), o3 ? right : (size_t)-1);
            }
        }
        printf("clear_constraint(%s,0/free-cell)=%s\n", in,
               yesno(s.clear_constraint(inst, 0)));
        printf("reset(%s)=%s parsed_after=%zu\n", in, yesno(s.reset(inst)),
               s.parsed_len(inst));
    }

    /* ── §9: choose a LONGER row, assert the cursor, train, dump ──── */
    /* Runs BEFORE choose-train on purpose: that phase's whole-row train
     * leaves the §10 user-bigram residue in the store, and a dump here
     * after it would re-report those rows instead of measuring this
     * flow's own writes (the pin's constraint-free train writes none).
     * The pin's LONGER branch (pinyin.cpp:2521-2530) trains the row's
     * token +483 unigram through the phrase index and answers cursor 1
     * (true) without touching the constraints; pinyin_train then walks
     * a constraint-free result, writes nothing to the user bigram, and
     * answers true. Sort words with bit 0x2 set (e.g. 0x1e) never
     * surface a LONGER row: the phase finds the type-7 row when the
     * running word carries one and reports the skip when it does not,
     * so the same binary scores every word. */
    printf("=== phase: longer-choose ===\n");
    /* "fang": one full-pinyin key whose two-char extensions (方面/方便/
     * 方向 …) exist in the system tables, so the pin's suggestion walk
     * finds a strictly-longer phrase and the prepend surfaces a type-7
     * row at every word with bit 0x2 clear (0x1c and 0x14 included) —
     * the option-sweep corpus row that showed the gap. */
    s.parse_full(inst, "fang");
    s.guess_sentence(inst);
    s.guess_candidates(inst, 0, sort_word);
    lookup_candidate_t *longer = NULL;
    guint longer_at = 0;
    {
        guint n = 0;
        if (s.n_cand(inst, &n)) {
            for (guint i = 0; i < n && i < 10; i++) {
                lookup_candidate_t *c = NULL;
                int type = -1;
                if (s.get_cand(inst, i, &c) && c &&
                    s.get_type(inst, c, &type) && type == 7 /* LONGER */) {
                    longer = c;
                    longer_at = i;
                    break;
                }
            }
        }
    }
    if (!longer) {
        printf("longer-row=skipped(no-type7-at-0x%02x)\n", sort_word);
    } else {
        const char *ltext = NULL;
        s.get_str(inst, longer, &ltext);
        printf("longer-row=%u text=%s\n", longer_at,
               ltext ? ltext : "(null)");
        /* The trained observable: the row's phrase tokens' unigrams,
         * read BEFORE the choose and again after the train so the dump
         * shows the +483 delta on whichever token the branch trained
         * (the ABI exports no candidate-token getter, so every token
         * the phrase resolves to is read — the trained one is among
         * them and moves by exactly 483; the others must not move). */
        void *ltoks = NULL;
        guint nlt = 0;
        if (ltext) {
            ltoks = g_array_new_fn(0, 0, 4);
            bool lt = s.lookup_tokens(inst, ltext, ltoks);
            nlt = ltoks ? ((GArrayPub *)ltoks)->len : 0;
            printf("longer-lookup(%s)=%s n=%u\n", ltext, yesno(lt), nlt);
            for (guint i = 0; i < nlt && i < 10; i++) {
                phrase_token_t t =
                    ((phrase_token_t *)((GArrayPub *)ltoks)->data)[i];
                guint f = 0;
                /* Sequence the read call before the printf: an argument
                 * expression that writes f while another reads it is
                 * unsequenced UB (C11 6.5.2.2p10) and could print the
                 * pre-call zero instead of the frequency. */
                bool ok = s.token_unigram(inst, t, &f);
                printf("longer-unigram[%u](before,0x%08x)=%s/%u\n", i, t,
                       yesno(ok), f);
            }
        }
        int lcur = s.choose(inst, 0, longer);
        printf("choose(longer)=%d\n", lcur);
        printf("train(after-longer)=%s\n", yesno(s.train(inst, 0)));
        for (guint i = 0; i < nlt && i < 10; i++) {
            phrase_token_t t =
                ((phrase_token_t *)((GArrayPub *)ltoks)->data)[i];
            guint f = 0;
            bool ok = s.token_unigram(inst, t, &f);
            printf("longer-unigram[%u](after,0x%08x)=%s/%u\n", i, t,
                   yesno(ok), f);
        }
        if (ltoks)
            g_array_free_fn(ltoks, 1);
        /* The user-bigram dump: the constraint-free train must have
         * written nothing beyond the imported baseline. */
        bigram_export_iterator_t *lbx = s.begin_bigram(ctx);
        unsigned lrow = 0;
        if (lbx) {
            while (s.bigram_has_next(lbx) && lrow < 20) {
                char *phrase = NULL, *pinyins = NULL;
                gint count = 0;
                if (!s.bigram_get_next(lbx, &phrase, &pinyins, &count))
                    break;
                printf("longer-bigram[%u]=%s|%s|%d\n", lrow,
                       phrase ? phrase : "(null)",
                       pinyins ? pinyins : "(null)", count);
                if (phrase)
                    g_free_fn(phrase);
                if (pinyins)
                    g_free_fn(pinyins);
                lrow++;
            }
            s.end_bigram(lbx);
        }
        printf("longer-bigram-rows=%u\n", lrow);
        printf("train(after-longer,2)=%s\n", yesno(s.train(inst, 0)));
    }
    printf("reset(longer-choose)=%s\n", yesno(s.reset(inst)));

    /* ── choose / nbest / train / remember ──────────────────────────── */
    printf("=== phase: choose-train ===\n");
    size_t consumed = s.parse_full(inst, "nihaoshijie");
    (void)consumed;
    s.guess_sentence(inst);
    s.guess_candidates(inst, 0, sort_word);
    lookup_candidate_t *first = NULL;
    guint8 proved_nbest = 0;
    bool have_nbest = false;
    if (s.get_cand(inst, 0, &first) && first) {
        int type = -1;
        s.get_type(inst, first, &type);
        const char *text = NULL;
        s.get_str(inst, first, &text);
        printf("choose-row0: type=%d text=%s\n", type, text ? text : "(null)");
        if (type == 1 /* NBEST_MATCH_CANDIDATE */) {
            guint8 nb = 255;
            have_nbest = s.nbest_index(inst, first, &nb);
            proved_nbest = nb;
            printf("proved-nbest=%s/%u\n", yesno(have_nbest), nb);
        }
        int cur = s.choose(inst, 0, first);
        printf("choose(0,row0)=%d\n", cur);
        /* No left/right move at the post-choose cursor: a full-width
         * row-0 choose lands it on the composition tail, where the pin's
         * second _check_offset asserts (see the phase-full comment). */
        size_t off = 0;
        bool o = s.get_offset(inst, (size_t)cur, &off);
        printf("offset(after-choose)=%s/%zu\n", yesno(o), o ? off : (size_t)-1);
    }
    /* remove_user is assert-fenced at the pin on BOTH sides of its
     * surface: NORMAL_CANDIDATE type (pinyin.cpp:3734) and a
     * USER_DICTIONARY token (:3738) — a system row aborts the pinned
     * library. ibus only ever calls it behind a true
     * pinyin_is_user_candidate, and so does this probe: the false path
     * of remove_user is unmeasurable against the oracle and stays
     * covered by the capi-side contract tests. */
    {
        bool removed_asked = false;
        guint n = 0;
        if (s.n_cand(inst, &n)) {
            for (guint i = 0; i < n && i < 10; i++) {
                lookup_candidate_t *c = NULL;
                if (s.get_cand(inst, i, &c) && c && s.is_user(inst, c)) {
                    printf("remove_user(user-row%u)=%s\n", i,
                           yesno(s.remove_user(inst, c)));
                    removed_asked = true;
                    break;
                }
            }
        }
        if (!removed_asked)
            printf("remove_user=skipped(no-user-row)\n");
    }
    printf("train(0)=%s\n", yesno(s.train(inst, 0)));
    if (have_nbest) {
        char *sent = NULL;
        bool sok = s.get_sentence(inst, proved_nbest, &sent);
        printf("sentence[nbest=%u]=%s text=%s\n", proved_nbest, yesno(sok),
               sok && sent ? sent : "(null)");
        if (sent)
            g_free_fn(sent);
        printf("train(nbest=%u)=%s\n", proved_nbest,
               yesno(s.train(inst, proved_nbest)));
    }
    printf("remember(你好/1)=%s\n", yesno(s.remember(inst, "你好", 1)));
    printf("reset(choose-train)=%s\n", yesno(s.reset(inst)));

    /* ── prediction surface ─────────────────────────────────────────── */
    printf("=== phase: predict ===\n");
    s.parse_full(inst, "ni");
    s.guess_sentence(inst);
    s.guess_candidates(inst, 0, sort_word);
    lookup_candidate_t *ni0 = NULL;
    if (s.get_cand(inst, 0, &ni0) && ni0) {
        int type = -1;
        s.get_type(inst, ni0, &type);
        if (type != 5 && type != 8) { /* never choose a predicted row */
            printf("choose(ni,row0)=%d type=%d\n", s.choose(inst, 0, ni0),
                   type);
        }
    }
    printf("predict(ni)=%s\n", yesno(s.predict(inst, "ni")));
    dump_rows(&s, inst, "predict(ni)", 10);
    /* choose one PREDICTED_PREFIX row through its own setter */
    {
        guint n = 0;
        if (s.n_cand(inst, &n)) {
            for (guint i = 0; i < n && i < 10; i++) {
                lookup_candidate_t *c = NULL;
                int type = -1;
                if (s.get_cand(inst, i, &c) && c && s.get_type(inst, c, &type) &&
                    type == 5) {
                    printf("choose_predicted(row%u)=%s\n", i,
                           yesno(s.choose_pred(inst, c)));
                    break;
                }
            }
        }
    }
    printf("reset(predict)=%s\n", yesno(s.reset(inst)));

    /* ── double-pinyin surface, incl. the row-5b out-of-enum lie ───── */
    printf("=== phase: double ===\n");
    {
        size_t c = s.parse_double(inst, "aa");
        printf("double(aa@ZRM): consumed=%zu parsed=%zu\n", c,
               s.parsed_len(inst));
        for (size_t k = 0; k <= c; k++) {
            char *aux = NULL;
            bool ok = s.double_aux(inst, k, &aux);
            printf("double_aux(%zu)=%s text=%s\n", k, yesno(ok),
                   ok && aux ? aux : "(null)");
            if (aux)
                g_free_fn(aux);
        }
        printf("double_guess(aa@ZRM)=%s\n",
               yesno(s.guess_candidates(inst, 0, sort_word)));
        dump_rows(&s, inst, "double(aa@ZRM)", 5);
    }
    printf("set_double_scheme(99/out-of-enum)=%s\n", yesno(s.set_double(ctx, 99)));
    {
        size_t c = s.parse_double(inst, "aa");
        printf("double(aa@99): consumed=%zu parsed=%zu\n", c,
               s.parsed_len(inst));
        printf("double_guess(aa@99)=%s\n",
               yesno(s.guess_candidates(inst, 0, sort_word)));
        dump_rows(&s, inst, "double(aa@99)", 5);
    }
    printf("set_double_scheme(-1/out-of-enum)=%s\n", yesno(s.set_double(ctx, -1)));
    {
        size_t c = s.parse_double(inst, "aa");
        printf("double(aa@-1): consumed=%zu parsed=%zu\n", c,
               s.parsed_len(inst));
    }
    printf("set_double_scheme(1/restore)=%s\n", yesno(s.set_double(ctx, 1)));
    {
        size_t c = s.parse_double(inst, "aa");
        printf("double(aa@restored): consumed=%zu parsed=%zu\n", c,
               s.parsed_len(inst));
        printf("double_guess(aa@restored)=%s\n",
               yesno(s.guess_candidates(inst, 0, sort_word)));
        dump_rows(&s, inst, "double(aa@restored)", 5);
    }
    printf("reset(double)=%s\n", yesno(s.reset(inst)));

    /* ── chewing surface (STANDARD keyboard; 7 is never set) ────────── */
    printf("=== phase: chewing ===\n");
    {
        for (int k = 'a'; k <= 'z'; k++) {
            char **symbols = NULL;
            bool ok = s.in_chewing_keyboard(inst, (char)k, &symbols);
            printf("in_chewing_keyboard(%c)=%s symbols=", (char)k, yesno(ok));
            if (ok && symbols) {
                for (char **sp = symbols; *sp; sp++)
                    printf("%s%s", sp == symbols ? "" : ",", *sp);
                g_strfreev_fn(symbols);
            }
            printf("\n");
        }
        size_t c = s.parse_chewing(inst, "su");
        printf("chewing(su): consumed=%zu parsed=%zu\n", c,
               s.parsed_len(inst));
        for (size_t k = 0; k <= c; k++) {
            char *aux = NULL;
            bool ok = s.chewing_aux(inst, k, &aux);
            printf("chewing_aux(%zu)=%s text=%s\n", k, yesno(ok),
                   ok && aux ? aux : "(null)");
            if (aux)
                g_free_fn(aux);
        }
        printf("chewing_guess(su)=%s\n",
               yesno(s.guess_candidates(inst, 0, sort_word)));
        dump_rows(&s, inst, "chewing(su)", 5);
        printf("reset(chewing)=%s\n", yesno(s.reset(inst)));
    }

    /* ── export seams, mask-out, export again ───────────────────────── */
    printf("=== phase: export ===\n");
    export_iterator_t *ex = s.begin_get(ctx, USER_DICT_INDEX);
    printf("begin_get(%u)=%s\n", USER_DICT_INDEX, ex ? "ok" : "NULL");
    if (ex) {
        unsigned row = 0;
        bool more = s.has_next(ex);
        printf("has_next[%u]=%s\n", row, yesno(more));
        while (more) {
            char *phrase = NULL, *pinyins = NULL;
            gint count = 0;
            if (!s.get_next(ex, &phrase, &pinyins, &count)) {
                printf("get_next[%u]=false\n", row);
                break;
            }
            printf("phrase[%u]=%s|%s|%d\n", row, phrase ? phrase : "(null)",
                   pinyins ? pinyins : "(null)", count);
            if (phrase)
                g_free_fn(phrase);
            if (pinyins)
                g_free_fn(pinyins);
            if (++row >= 20)
                break; /* the walk imported three; bound the loop anyway */
            more = s.has_next(ex);
            printf("has_next[%u]=%s\n", row, yesno(more));
        }
        printf("has_next(exhausted)=%s\n", yesno(s.has_next(ex)));
        s.end_get(ex);
        printf("end_get=ok\n");
    }
    bigram_export_iterator_t *bx = s.begin_bigram(ctx);
    printf("begin_bigram=%s\n", bx ? "ok" : "NULL");
    if (bx) {
        unsigned row = 0;
        bool bmore = s.bigram_has_next(bx);
        printf("bigram_has_next[%u]=%s\n", row, yesno(bmore));
        while (bmore) {
            char *phrase = NULL, *pinyins = NULL;
            gint count = 0;
            if (!s.bigram_get_next(bx, &phrase, &pinyins, &count)) {
                printf("bigram_get_next[%u]=false\n", row);
                break;
            }
            printf("bigram[%u]=%s|%s|%d\n", row, phrase ? phrase : "(null)",
                   pinyins ? pinyins : "(null)", count);
            if (phrase)
                g_free_fn(phrase);
            if (pinyins)
                g_free_fn(pinyins);
            if (++row >= 20)
                break;
            bmore = s.bigram_has_next(bx);
            printf("bigram_has_next[%u]=%s\n", row, yesno(bmore));
        }
        printf("bigram_has_next(exhausted)=%s\n",
               yesno(s.bigram_has_next(bx)));
        s.end_bigram(bx);
        printf("end_bigram=ok\n");
    }
    /* mask_out's observable effect: the user-range export drains. */
    printf("mask_out(user-range)=%s\n",
           yesno(s.mask_out(ctx, USER_TOKEN_MASK, USER_TOKEN_MASK)));
    printf("save(after-mask)=%s\n", yesno(s.save(ctx)));
    ex = s.begin_get(ctx, USER_DICT_INDEX);
    printf("begin_get(after-mask)=%s\n", ex ? "ok" : "NULL");
    if (ex) {
        printf("has_next(after-mask)=%s\n", yesno(s.has_next(ex)));
        s.end_get(ex);
        printf("end_get(after-mask)=ok\n");
    }

    /* ── the rest of the exported ABI ───────────────────────────────── */
    /* Drives the 21 exported symbols the earlier phases do not reach,
     * on a fresh context (their scheme/segmentation surfaces want a
     * clean state). The pin's caller contracts: full-pinyin schemes
     * stay in 1..3 (out-of-enum aborts, pinyin_parser2.cpp:398);
     * unload_phrase_library asserts in-range (pinyin.cpp:466) and
     * answers false for the non-GBK default tables, so only in-range
     * indexes are sent; the single-key parsers, the introspection
     * family and both string getters are driven the way key-surface
     * and dict-surface drive them. */
    printf("=== phase: abi-extras ===\n");
    pinyin_context_t *ctx2 = s.init(argv[2], userdir);
    if (!ctx2) {
        /* The extras phase is a quarter of the exported surface: a
         * context that will not build there means the walk never
         * exercised those symbols, which is a failure, not a pass —
         * the primary context succeeded, so the refusal is specific
         * to the second handle. */
        fprintf(stderr, "extras context init failed\n");
        s.fini(ctx);
        return 1;
    }
    printf("extras-init=ok\n");
    {
        printf("set_options(0x18a)=%s\n", yesno(s.set_options(ctx2, PARITY_WORD)));
        printf("set_full_scheme(1/HANYU)=%s\n", yesno(s.set_full_scheme(ctx2, 1)));
        pinyin_instance_t *inst2 = s.alloc(ctx2);
        printf("extras-alloc=%s\n", inst2 ? "ok" : "NULL");
        printf("get_context=%s\n",
               s.get_context(inst2) == ctx2 ? "equal" : "DIFFERS");
        guint32 keybuf = 0;
        ChewingKey *key = (ChewingKey *)&keybuf;
        bool pok = s.parse_full_pinyin(inst2, "ni", key);
        printf("parse_full_pinyin(ni)=%s packed=%08x\n", yesno(pok), keybuf);
        printf("is_incomplete(ni)=%s\n", yesno(s.is_incomplete(inst2, key)));
        bool dok = s.parse_double_pinyin(inst2, "aa", key);
        printf("parse_double_pinyin(aa@ZRM)=%s packed=%08x\n", yesno(dok), keybuf);
        bool cok = s.parse_chewing_one(inst2, "su", key);
        printf("parse_chewing(su)=%s packed=%08x\n", yesno(cok), keybuf);
        for (int scheme = 2; scheme <= 3; scheme++) {
            printf("set_full_scheme(%d)=%s\n", scheme,
                   yesno(s.set_full_scheme(ctx2, scheme)));
            s.parse_full(inst2, "nihao");
            s.guess_sentence(inst2);
            ChewingKey *k2 = NULL;
            if (s.get_key(inst2, 0, &k2) && k2) {
                char *lu = NULL, *se = NULL;
                bool l1 = s.luoma_string(inst2, k2, &lu);
                bool l2 = s.secondary_zhuyin_string(inst2, k2, &se);
                printf("luoma(%d)=%s/%s secondary(%d)=%s/%s\n", scheme,
                       yesno(l1), l1 && lu ? lu : "(null)", scheme, yesno(l2),
                       l2 && se ? se : "(null)");
                if (lu) g_free_fn(lu);
                if (se) g_free_fn(se);
            }
            printf("reset(extras %d)=%s\n", scheme, yesno(s.reset(inst2)));
        }
        printf("set_full_scheme(1/restore)=%s\n", yesno(s.set_full_scheme(ctx2, 1)));
        printf("phrase_segment(你好世界)=%s\n",
               yesno(s.phrase_segment(inst2, "你好世界")));
        guint np = 0;
        printf("n_phrase=%s/%u\n", yesno(s.n_phrase(inst2, &np)), np);
        phrase_token_t tok = 0;
        bool tok_ok = np > 0 && s.phrase_token(inst2, 0, &tok);
        printf("phrase_token[0]=%s/0x%08x\n", yesno(tok_ok), tok_ok ? tok : 0);
        printf("sentence_with_prefix(ni)=%s\n",
               yesno(s.sentence_with_prefix(inst2, "ni")));
        printf("predict_plain(ni)=%s\n", yesno(s.predict_plain(inst2, "ni")));
        dump_rows(&s, inst2, "predict-plain(ni)", 5);
        void *tokens = g_array_new_fn(0, 0, 4);
        bool lt = s.lookup_tokens(inst2, "你好", tokens);
        guint ntok = tokens ? ((GArrayPub *)tokens)->len : 0;
        printf("lookup_tokens(你好)=%s n=%u\n", yesno(lt), ntok);
        if (lt && ntok > 0) {
            phrase_token_t t0 =
                ((phrase_token_t *)((GArrayPub *)tokens)->data)[0];
            guint len = 0;
            char *ph = NULL;
            bool p1 = s.token_phrase(inst2, t0, &len, &ph);
            printf("token_phrase(0x%08x)=%s/%u/%s\n", t0, yesno(p1), len,
                   p1 && ph ? ph : "(null)");
            if (p1 && ph) g_free_fn(ph);
            guint npro = 0;
            printf("token_n_pron=%s/%u\n",
                   yesno(s.token_n_pron(inst2, t0, &npro)), npro);
            void *keys = g_array_new_fn(0, 0, 4);
            printf("token_nth_pron(0)=%s\n",
                   yesno(s.token_nth_pron(inst2, t0, 0, keys)));
            g_array_free_fn(keys, 1);
            guint freq = 0;
            bool u1 = s.token_unigram(inst2, t0, &freq);
            printf("token_unigram=%s/%u\n", yesno(u1), freq);
            printf("token_add(+1)=%s\n", yesno(s.token_add(inst2, t0, 1)));
            guint freq2 = 0;
            printf("token_unigram(after add)=%s/%u\n",
                   yesno(s.token_unigram(inst2, t0, &freq2)), freq2);
        }
        g_array_free_fn(tokens, 1);
        /* dict-surface-diff.c's smoke-proved-safe shapes: load only the
         * provisioned indexes (an unprovisioned one asserts inside the
         * pin, pinyin.cpp:457), unload sweeps every in-range index and
         * answers false for the non-GBK defaults. */
        for (int li = 0; li < 4; li++) {
            guint8 ix = (guint8[]){1, 2, 4, 7}[li];
            printf("load_lib(%u)=%s\n", ix, yesno(s.load_lib(ctx2, ix)));
        }
        printf("load_lib(250)=%s\n", yesno(s.load_lib(ctx2, 250)));
        for (guint8 ix = 0; ix < 8; ix++)
            printf("unload_lib(%u)=%s\n", ix, yesno(s.unload_lib(ctx2, ix)));
        printf("unload_lib(250)=%s [never sent at the pin: asserts]\n",
               "skipped");
        s.free_inst(inst2);
        printf("extras-free=ok\n");
        s.fini(ctx2);
        printf("extras-fini=ok\n");
    }

    /* ── teardown ───────────────────────────────────────────────────── */
    printf("=== phase: teardown ===\n");
    s.free_inst(inst);
    printf("free_instance=ok\n");
    s.fini(ctx);
    printf("fini=ok\n");

    dlclose(handle);
    rm_rf(userdir);
    return 0;
}
