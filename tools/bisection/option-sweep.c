/*
 * option-sweep.c — W10 option-bit differential driver.
 *
 * Drives identical parse + candidate sequences into a pinyin shared object
 * (libpinyin_capi.so or the pinned libpinyin.so) for one option word, and
 * prints a deterministic log of parse length, auxiliary text, candidate
 * count and the first ten candidate strings. Run against both libraries and
 * diff the logs.
 *
 * Every parse also logs the parsed keys and key rests, walked by byte
 * offset through pinyin_get_pinyin_key / pinyin_get_pinyin_key_rest (the
 * pin has no pinyin_get_n_pinyin: pinyin.cpp:2889-2900 at 074a2219 is
 * compiled out).
 *
 * With a fifth argument `pairs` the driver runs the parse-only pair sweep
 * instead: every ordered pair of the pinned `pinyin_index` spellings
 * (option-sweep-tables.h) whose concatenation could parse through an alias
 * row the option word admits, i.e. at least one member is such a row. A
 * string with no admitted alias parse parses as under the word without
 * those bits, which the baseline case covers.
 *
 * The runner (run-option-sweep.sh) invokes this once per (case, library).
 */

#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#include "option-sweep-tables.h"

typedef void pinyin_context_t;
typedef void pinyin_instance_t;
typedef void lookup_candidate_t;

typedef uint32_t pinyin_option_t;
typedef uint32_t guint;
typedef char gchar;

/* The sort word is an env-parameterised input of this sweep:
 * OPTION_SWEEP_SORT (hex, default 1e — the parity word every runner
 * passes). The ibus-libpinyin presets 0x14 and 0x1c leave
 * SORT_WITHOUT_LONGER_CANDIDATE (0x2) clear, so on the pin they carry
 * longer-candidate rows; the capi ignores every bit but 0x1 today.
 * This sweep is the instrument that measures that gap per word. */
static guint g_sort_word = (guint)0x1e;
#define CANDIDATE_DEPTH 10u

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef pinyin_instance_t *(*fn_alloc)(pinyin_context_t *);
typedef void (*fn_free_instance)(pinyin_instance_t *);
typedef bool (*fn_set_options)(pinyin_context_t *, pinyin_option_t);
typedef size_t (*fn_parse)(pinyin_instance_t *, const char *);
typedef size_t (*fn_parsed_len)(pinyin_instance_t *);
typedef bool (*fn_aux)(pinyin_instance_t *, size_t, gchar **);
typedef bool (*fn_guess)(pinyin_instance_t *, size_t, guint);
typedef bool (*fn_getn)(pinyin_instance_t *, guint *);
typedef bool (*fn_getc)(pinyin_instance_t *, guint, lookup_candidate_t **);
typedef bool (*fn_getstr)(pinyin_instance_t *, lookup_candidate_t *, const gchar **);
typedef bool (*fn_reset)(pinyin_instance_t *);

/* Upstream chewing_key.h:41-48 / :100-104 mirrors (key-surface-diff.c). */
typedef struct ChewingKey {
    uint16_t m_initial :5;
    uint16_t m_middle :2;
    uint16_t m_final :5;
    uint16_t m_tone :3;
    uint16_t m_zero_padding :1;
} ChewingKey;

typedef struct ChewingKeyRest {
    uint16_t m_raw_begin;
    uint16_t m_raw_end;
} ChewingKeyRest;

_Static_assert(sizeof(ChewingKey) == 2, "packed chewing key must be 2 bytes");
_Static_assert(sizeof(ChewingKeyRest) == 4, "key rest must be 4 bytes");

typedef bool (*fn_key)(pinyin_instance_t *, size_t, ChewingKey **);
typedef bool (*fn_key_rest)(pinyin_instance_t *, size_t, ChewingKeyRest **);
typedef bool (*fn_key_rest_pos)(pinyin_instance_t *, ChewingKeyRest *,
                                uint16_t *, uint16_t *);
typedef bool (*fn_pinyin_string)(pinyin_instance_t *, ChewingKey *, gchar **);

struct syms {
    fn_init init;
    fn_init fixture_init;
    fn_fini fini;
    fn_alloc alloc;
    fn_free_instance free_instance;
    fn_set_options set_options;
    fn_parse parse;
    fn_parsed_len parsed_len;
    fn_aux aux;
    fn_guess guess;
    fn_getn getn;
    fn_getc getc;
    fn_getstr getstr;
    fn_reset reset;
    fn_key key;
    fn_key_rest key_rest;
    fn_key_rest_pos key_rest_pos;
    fn_pinyin_string pinyin_string;
};

typedef void (*fn_g_free)(void *);
static fn_g_free g_free_fn;

static void resolve_g_free(void) {
    g_free_fn = (fn_g_free)free;
    void *glib = dlopen("libglib-2.0.so.0", RTLD_NOW);
    if (glib) {
        fn_g_free sym = (fn_g_free)dlsym(glib, "g_free");
        if (sym)
            g_free_fn = sym;
    }
}

static void *load(void *handle, const char *name) {
    void *symbol = dlsym(handle, name);
    if (!symbol) {
        fprintf(stderr, "MISSING: %s\n", name);
        exit(1);
    }
    return symbol;
}

static void resolve_all(void *handle, struct syms *s) {
    s->init = (fn_init)load(handle, "pinyin_init");
    s->fixture_init = (fn_init)dlsym(handle, "oxpinyin_init_for_fixtures");
    s->fini = (fn_fini)load(handle, "pinyin_fini");
    s->alloc = (fn_alloc)load(handle, "pinyin_alloc_instance");
    s->free_instance = (fn_free_instance)load(handle, "pinyin_free_instance");
    s->set_options = (fn_set_options)load(handle, "pinyin_set_options");
    s->parse = (fn_parse)load(handle, "pinyin_parse_more_full_pinyins");
    s->parsed_len = (fn_parsed_len)load(handle, "pinyin_get_parsed_input_length");
    s->aux = (fn_aux)load(handle, "pinyin_get_full_pinyin_auxiliary_text");
    s->guess = (fn_guess)load(handle, "pinyin_guess_candidates");
    s->getn = (fn_getn)load(handle, "pinyin_get_n_candidate");
    s->getc = (fn_getc)load(handle, "pinyin_get_candidate");
    s->getstr = (fn_getstr)load(handle, "pinyin_get_candidate_string");
    s->reset = (fn_reset)load(handle, "pinyin_reset");
    s->key = (fn_key)load(handle, "pinyin_get_pinyin_key");
    s->key_rest = (fn_key_rest)load(handle, "pinyin_get_pinyin_key_rest");
    s->key_rest_pos = (fn_key_rest_pos)load(handle, "pinyin_get_pinyin_key_rest_positions");
    s->pinyin_string = (fn_pinyin_string)load(handle, "pinyin_get_pinyin_string");
}

static int file_exists(const char *dir, const char *name) {
    char path[4096];
    struct stat st;
    if (snprintf(path, sizeof(path), "%s/%s", dir, name) >= (int)sizeof(path))
        return 0;
    return stat(path, &st) == 0 && S_ISREG(st.st_mode);
}

/* The parsed keys from byte 0: `pinyin:hex@begin-end` per key, the next
 * key read at the previous rest's end. Both getters skip a leading
 * apostrophe key (pinyin.cpp:2903-2919 at 074a2219). */
static void print_keys(const struct syms *s, pinyin_instance_t *inst,
                       size_t parsed_len) {
    printf("keys=");
    size_t offset = 0;
    const char *sep = "";
    while (offset < parsed_len) {
        ChewingKey *key = NULL;
        ChewingKeyRest *rest = NULL;
        if (!s->key(inst, offset, &key) || !key)
            break;
        ChewingKey copy = *key;
        if (!s->key_rest(inst, offset, &rest) || !rest)
            break;
        uint16_t begin = 0, end = 0;
        s->key_rest_pos(inst, rest, &begin, &end);
        gchar *text = NULL;
        bool named = s->pinyin_string(inst, &copy, &text) && text;
        uint16_t bits = 0;
        memcpy(&bits, &copy, sizeof(bits));
        printf("%s%s:%04x@%u-%u", sep, named ? text : "?", bits, begin, end);
        if (text)
            g_free_fn(text);
        sep = " ";
        if (end <= offset)
            break;
        offset = end;
    }
    printf("\n");
}

static void drive(const struct syms *s, pinyin_instance_t *inst,
                  const char *input) {
    size_t consumed = s->parse(inst, input);
    size_t parsed_len = s->parsed_len(inst);
    printf("input=%s consumed=%zu parsed=%zu\n", input, consumed, parsed_len);
    print_keys(s, inst, parsed_len);

    gchar *aux = NULL;
    if (s->aux(inst, parsed_len, &aux)) {
        printf("aux=%s\n", aux ? aux : "(null)");
    } else {
        printf("aux=false\n");
    }
    if (aux)
        g_free_fn(aux);

    if (!s->guess(inst, 0, g_sort_word)) {
        printf("guess=false n=0\n");
        s->reset(inst);
        return;
    }

    guint n = 0;
    s->getn(inst, &n);
    printf("n=%u\n", n);
    guint limit = n < CANDIDATE_DEPTH ? n : CANDIDATE_DEPTH;
    for (guint i = 0; i < limit; i++) {
        lookup_candidate_t *cand = NULL;
        if (!s->getc(inst, i, &cand) || !cand) {
            printf("cand[%u]=FAILED\n", i);
            continue;
        }
        const gchar *text = NULL;
        s->getstr(inst, cand, &text);
        printf("cand[%u]=%s\n", i, text ? text : "(null)");
    }
    s->reset(inst);
}

/* PINYIN_AMB_S_SH | PINYIN_AMB_Z_ZH | PINYIN_CORRECT_ALL
 * (pinyin_custom2.h:51-52, :79 at 074a2219). */
#define ALIAS_GATES ((pinyin_option_t)((1u << 11) | (1u << 12) | (0xFFu << 21)))

static bool admitted_alias(unsigned int gate, unsigned long options) {
    return gate != 0 && (gate & options) == gate;
}

/* Parse-only sweep: one `input=` and one `keys=` line per pair. */
static void sweep_pairs(const struct syms *s, pinyin_instance_t *inst,
                        unsigned long options) {
    static const size_t n = sizeof(PINYIN_INDEX_SPELLINGS) /
                            sizeof(PINYIN_INDEX_SPELLINGS[0]);
    char input[32];
    size_t count = 0;
    for (size_t i = 0; i < n; i++) {
        bool first = admitted_alias(PINYIN_INDEX_SPELLINGS[i].gate, options);
        for (size_t j = 0; j < n; j++) {
            if (!first && !admitted_alias(PINYIN_INDEX_SPELLINGS[j].gate, options))
                continue;
            snprintf(input, sizeof(input), "%s%s", PINYIN_INDEX_SPELLINGS[i].spelling,
                     PINYIN_INDEX_SPELLINGS[j].spelling);
            size_t consumed = s->parse(inst, input);
            size_t parsed_len = s->parsed_len(inst);
            printf("input=%s consumed=%zu parsed=%zu\n", input, consumed, parsed_len);
            print_keys(s, inst, parsed_len);
            s->reset(inst);
            count++;
        }
    }
    printf("pairs=%zu\n", count);
}

int main(int argc, char **argv) {
    const char *sort_env = getenv("OPTION_SWEEP_SORT");
    if (sort_env && *sort_env) {
        /* Reject malformed words outright: strtoul silently accepts a
         * valid prefix ("1x" -> 1) and answers 0 for digit-less input,
         * and a sort word that is not what the operator typed would
         * score the sweep at the wrong word with no visible sign. */
        char *end = NULL;
        unsigned long parsed = strtoul(sort_env, &end, 16);
        if (end == sort_env || *end != '\0') {
            fprintf(stderr,
                    "OPTION_SWEEP_SORT=%s is not a hex word\n",
                    sort_env);
            return 1;
        }
        g_sort_word = (guint)parsed;
    }
    printf("sort=0x%02x\n", g_sort_word);
    if (argc != 5 && !(argc == 6 && strcmp(argv[5], "pairs") == 0)) {
        fprintf(stderr,
                "Usage: %s <so> <systemdir> <case> <options-hex> [pairs]\n",
                argv[0]);
        return 1;
    }
    bool pairs = argc == 6;
    const char *so_path = argv[1];
    const char *system_dir = argv[2];
    const char *case_name = argv[3];
    unsigned long options = strtoul(argv[4], NULL, 16);

    /* Correction/fuzzy triggers, plus the xian/fanan resplit-divided class
     * from the frozen matrix tables (docs/findings/matrix-split-tables.md).
     * `xian` is the divided-table entry xian→xi+an; `fanan` is the resplit
     * pair fa+nan→fan+an; `fangan` is the textbook fan+gan / fang+an mix. */
    static const char *inputs[] = {
        "agn", "amg", "diou", "duei", "duen", "lue", "jv", "zon",
        "cang", "sua", "zua", "sang", "zang", "fang", "gang", "lan",
        "ban", "ben", "bin", "nihao", "lve", "dui", "dun", "zong",
        "xian", "fanan", "fangan", "tian", "ang", "can",
        /* #830: equal length and key count, the pin keeps the lower total
         * correction distance (pinyin_parser2.cpp:305-309 at 074a2219). */
        "chonga", "chongan", "zhonge"
    };
    static const size_t n_inputs = sizeof(inputs) / sizeof(inputs[0]);

    void *handle = dlopen(so_path, RTLD_NOW);
    if (!handle) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }
    struct syms s;
    memset(&s, 0, sizeof(s));
    resolve_all(handle, &s);
    resolve_g_free();

    char user_dir[] = "/tmp/option-sweep-user-XXXXXX";
    if (!mkdtemp(user_dir)) {
        perror("mkdtemp");
        return 1;
    }

    fn_init init = (s.fixture_init && !file_exists(system_dir, "interpolation2.text"))
        ? s.fixture_init
        : s.init;
    pinyin_context_t *ctx = init(system_dir, user_dir);
    if (!ctx) {
        fprintf(stderr, "pinyin_init failed\n");
        return 1;
    }
    pinyin_instance_t *inst = s.alloc(ctx);
    if (!inst) {
        fprintf(stderr, "pinyin_alloc_instance failed\n");
        return 1;
    }
    if (!s.set_options(ctx, (pinyin_option_t)options)) {
        fprintf(stderr, "pinyin_set_options failed\n");
        return 1;
    }

    printf("case=%s options=0x%08lx\n", case_name, options);
    if (pairs) {
        if (!(options & ALIAS_GATES)) {
            fprintf(stderr, "pairs: option word 0x%lx admits no alias row\n", options);
            return 1;
        }
        sweep_pairs(&s, inst, options);
    } else {
        for (size_t i = 0; i < n_inputs; i++)
            drive(&s, inst, inputs[i]);
    }

    s.free_instance(inst);
    s.fini(ctx);
    dlclose(handle);
    rmdir(user_dir);
    return 0;
}
