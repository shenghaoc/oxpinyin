/*
 * option-sweep.c — W10 option-bit differential driver.
 *
 * Drives identical parse + candidate sequences into a pinyin shared object
 * (libpinyin_capi.so or the pinned libpinyin.so) for one option word, and
 * prints a deterministic log of parse length, auxiliary text, candidate
 * count and the first ten candidate strings. Run against both libraries and
 * diff the logs.
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

typedef void pinyin_context_t;
typedef void pinyin_instance_t;
typedef void lookup_candidate_t;

typedef uint32_t pinyin_option_t;
typedef uint32_t guint;
typedef char gchar;

#define DEFAULT_SORT ((guint)0x1e)
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
}

static int file_exists(const char *dir, const char *name) {
    char path[4096];
    struct stat st;
    if (snprintf(path, sizeof(path), "%s/%s", dir, name) >= (int)sizeof(path))
        return 0;
    return stat(path, &st) == 0 && S_ISREG(st.st_mode);
}

static void drive(const struct syms *s, pinyin_instance_t *inst,
                  const char *input) {
    size_t consumed = s->parse(inst, input);
    size_t parsed_len = s->parsed_len(inst);
    printf("input=%s consumed=%zu parsed=%zu\n", input, consumed, parsed_len);

    gchar *aux = NULL;
    if (s->aux(inst, parsed_len, &aux)) {
        printf("aux=%s\n", aux ? aux : "(null)");
    } else {
        printf("aux=false\n");
    }
    if (aux)
        g_free_fn(aux);

    if (!s->guess(inst, 0, DEFAULT_SORT)) {
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

int main(int argc, char **argv) {
    if (argc != 5) {
        fprintf(stderr,
                "Usage: %s <so> <systemdir> <case> <options-hex>\n",
                argv[0]);
        return 1;
    }
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
        "xian", "fanan", "fangan", "tian", "ang", "can"
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
    for (size_t i = 0; i < n_inputs; i++)
        drive(&s, inst, inputs[i]);

    s.free_instance(inst);
    s.fini(ctx);
    dlclose(handle);
    rmdir(user_dir);
    return 0;
}
