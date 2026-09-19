/*
 * residue-mechanism-diff.c — mechanism probes for the open residues in
 * docs/findings/probe-coverage-abi.md. Diagnosis only: prints the
 * observables that decide class; does not assert parity.
 *
 * Phase D (system-token unigram):
 *   fresh context, lookup_tokens("你好"), token_get_unigram_frequency.
 *   Same systemdir on both .so files settles harness-vs-reader.
 *
 * Phase B (the whole-row-choose train record):
 *   import 你好世界, parse nihaoshijie, guess_sentence, choose row 0
 *   (NBEST), clear_constraint(0) as a CONSTRAINT_ONESTEP probe, train
 *   twice, export the user bigram. The pin's train_result3 writes
 *   nothing when diff_result(best, best) installed no OneStep;
 *   oxpinyin's history fallback still observes.
 *
 * Usage:
 *   ./residue-mechanism-diff <path-to-so> <systemdir>
 *
 * Exit: 0 on a completed walk; 1 on setup/crash. The runner diffs two
 * logs.
 */

#define _POSIX_C_SOURCE 200809L
#include <dirent.h>
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
typedef void import_iterator_t;
typedef void bigram_export_iterator_t;
typedef uint32_t guint;
typedef uint32_t phrase_token_t;
typedef int32_t gint;
typedef char gchar;
typedef struct {
    char *data;
    guint len;
} GArrayPub;

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef pinyin_instance_t *(*fn_alloc)(pinyin_context_t *);
typedef void (*fn_free)(pinyin_instance_t *);
typedef bool (*fn_set_options)(pinyin_context_t *, uint32_t);
typedef size_t (*fn_parse)(pinyin_instance_t *, const char *);
typedef bool (*fn_guess_sentence)(pinyin_instance_t *);
typedef bool (*fn_guess_cands)(pinyin_instance_t *, size_t, guint);
typedef bool (*fn_getn)(pinyin_instance_t *, guint *);
typedef bool (*fn_getc)(pinyin_instance_t *, guint, lookup_candidate_t **);
typedef bool (*fn_getstr)(pinyin_instance_t *, lookup_candidate_t *, const gchar **);
typedef bool (*fn_gettype)(pinyin_instance_t *, lookup_candidate_t *, int *);
typedef bool (*fn_nbest)(pinyin_instance_t *, lookup_candidate_t *, uint8_t *);
typedef int (*fn_choose)(pinyin_instance_t *, size_t, lookup_candidate_t *);
typedef bool (*fn_clear)(pinyin_instance_t *, size_t);
typedef bool (*fn_train)(pinyin_instance_t *, uint8_t);
typedef bool (*fn_save)(pinyin_context_t *);
typedef import_iterator_t *(*fn_begin_add)(pinyin_context_t *, guint);
typedef bool (*fn_add_phrase)(import_iterator_t *, const char *, const char *, gint);
typedef void (*fn_end_add)(import_iterator_t *);
typedef bigram_export_iterator_t *(*fn_begin_bigram)(pinyin_context_t *);
typedef bool (*fn_bigram_has)(bigram_export_iterator_t *);
typedef bool (*fn_bigram_next)(bigram_export_iterator_t *, gchar **, gchar **, gint *);
typedef void (*fn_end_bigram)(bigram_export_iterator_t *);
typedef bool (*fn_lookup_tokens)(pinyin_instance_t *, const char *, void *);
typedef bool (*fn_token_unigram)(pinyin_instance_t *, phrase_token_t, guint *);
typedef bool (*fn_token_phrase)(pinyin_instance_t *, phrase_token_t, guint *, gchar **);
typedef void (*fn_g_free)(void *);
typedef void *(*fn_g_array_new)(int, int, unsigned int);
typedef void (*fn_g_array_free)(void *, int);

struct syms {
    fn_init init;
    fn_fini fini;
    fn_alloc alloc;
    fn_free free_inst;
    fn_set_options set_options;
    fn_parse parse;
    fn_guess_sentence guess_sentence;
    fn_guess_cands guess_cands;
    fn_getn getn;
    fn_getc getc;
    fn_getstr getstr;
    fn_gettype gettype;
    fn_nbest nbest;
    fn_choose choose;
    fn_clear clear;
    fn_train train;
    fn_save save;
    fn_begin_add begin_add;
    fn_add_phrase add_phrase;
    fn_end_add end_add;
    fn_begin_bigram begin_bigram;
    fn_bigram_has bigram_has;
    fn_bigram_next bigram_next;
    fn_end_bigram end_bigram;
    fn_lookup_tokens lookup_tokens;
    fn_token_unigram token_unigram;
    fn_token_phrase token_phrase;
};

static fn_g_free g_free_fn;
static fn_g_array_new g_array_new_fn;
static fn_g_array_free g_array_free_fn;

static void *must(void *h, const char *name) {
    void *s = dlsym(h, name);
    if (!s) {
        fprintf(stderr, "MISSING %s\n", name);
        exit(1);
    }
    return s;
}

static const char *yesno(bool v) { return v ? "true" : "false"; }

static void rm_rf(const char *dir) {
    DIR *d = opendir(dir);
    if (d) {
        struct dirent *entry;
        while ((entry = readdir(d)) != NULL) {
            if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0)
                continue;
            char path[4096];
            if (snprintf(path, sizeof(path), "%s/%s", dir, entry->d_name) >=
                (int)sizeof(path))
                continue;
            unlink(path);
        }
        closedir(d);
    }
    rmdir(dir);
}

static void dump_bigram(struct syms *s, pinyin_context_t *ctx, const char *tag) {
    bigram_export_iterator_t *it = s->begin_bigram(ctx);
    printf("%s:begin_bigram=%s\n", tag, it ? "ok" : "NULL");
    if (!it)
        return;
    unsigned row = 0;
    while (s->bigram_has(it)) {
        gchar *phrase = NULL, *pinyin = NULL;
        gint count = 0;
        bool ok = s->bigram_next(it, &phrase, &pinyin, &count);
        printf("%s:bigram[%u]=%s %s|%s|%d\n", tag, row, yesno(ok),
               phrase ? phrase : "(null)", pinyin ? pinyin : "(null)", count);
        if (phrase)
            g_free_fn(phrase);
        if (pinyin)
            g_free_fn(pinyin);
        row++;
        if (row > 32)
            break;
    }
    printf("%s:bigram_rows=%u\n", tag, row);
    s->end_bigram(it);
}

int main(int argc, char **argv) {
    if (argc != 3) {
        fprintf(stderr, "usage: %s <so> <systemdir>\n", argv[0]);
        return 1;
    }
    void *handle = dlopen(argv[1], RTLD_NOW);
    if (!handle) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }
    void *glib = dlopen("libglib-2.0.so.0", RTLD_NOW);
    if (glib) {
        g_free_fn = (fn_g_free)dlsym(glib, "g_free");
        g_array_new_fn = (fn_g_array_new)dlsym(glib, "g_array_new");
        g_array_free_fn = (fn_g_array_free)dlsym(glib, "g_array_free");
    }
    if (!g_free_fn || !g_array_new_fn || !g_array_free_fn) {
        fprintf(stderr, "fatal: glib helpers unavailable\n");
        return 1;
    }

    struct syms s;
    s.init = (fn_init)must(handle, "pinyin_init");
    s.fini = (fn_fini)must(handle, "pinyin_fini");
    s.alloc = (fn_alloc)must(handle, "pinyin_alloc_instance");
    s.free_inst = (fn_free)must(handle, "pinyin_free_instance");
    s.set_options = (fn_set_options)must(handle, "pinyin_set_options");
    s.parse = (fn_parse)must(handle, "pinyin_parse_more_full_pinyins");
    s.guess_sentence = (fn_guess_sentence)must(handle, "pinyin_guess_sentence");
    s.guess_cands = (fn_guess_cands)must(handle, "pinyin_guess_candidates");
    s.getn = (fn_getn)must(handle, "pinyin_get_n_candidate");
    s.getc = (fn_getc)must(handle, "pinyin_get_candidate");
    s.getstr = (fn_getstr)must(handle, "pinyin_get_candidate_string");
    s.gettype = (fn_gettype)must(handle, "pinyin_get_candidate_type");
    s.nbest = (fn_nbest)must(handle, "pinyin_get_candidate_nbest_index");
    s.choose = (fn_choose)must(handle, "pinyin_choose_candidate");
    s.clear = (fn_clear)must(handle, "pinyin_clear_constraint");
    s.train = (fn_train)must(handle, "pinyin_train");
    s.save = (fn_save)must(handle, "pinyin_save");
    s.begin_add = (fn_begin_add)must(handle, "pinyin_begin_add_phrases");
    s.add_phrase = (fn_add_phrase)must(handle, "pinyin_iterator_add_phrase");
    s.end_add = (fn_end_add)must(handle, "pinyin_end_add_phrases");
    s.begin_bigram = (fn_begin_bigram)must(handle, "pinyin_begin_get_bigram_phrases");
    s.bigram_has = (fn_bigram_has)must(handle, "pinyin_bigram_iterator_has_next_phrase");
    s.bigram_next = (fn_bigram_next)must(handle, "pinyin_bigram_iterator_get_next_phrase");
    s.end_bigram = (fn_end_bigram)must(handle, "pinyin_end_get_bigram_phrases");
    s.lookup_tokens = (fn_lookup_tokens)must(handle, "pinyin_lookup_tokens");
    s.token_unigram = (fn_token_unigram)must(handle, "pinyin_token_get_unigram_frequency");
    s.token_phrase = (fn_token_phrase)must(handle, "pinyin_token_get_phrase");

    char userdir[] = "/tmp/residue-mech-XXXXXX";
    if (!mkdtemp(userdir)) {
        perror("mkdtemp");
        return 1;
    }
    fprintf(stderr, "so=%s\n", argv[1]);
    fprintf(stderr, "systemdir=%s\n", argv[2]);
    fprintf(stderr, "userdir=%s\n", userdir);

    /* ── Phase D: fresh context, no import ─────────────────────────── */
    printf("=== phase: D-system-unigram ===\n");
    pinyin_context_t *ctx_d = s.init(argv[2], userdir);
    if (!ctx_d) {
        fprintf(stderr, "init failed for phase D\n");
        rm_rf(userdir);
        return 1;
    }
    s.set_options(ctx_d, 0x1e);
    pinyin_instance_t *inst_d = s.alloc(ctx_d);
    void *tokens = g_array_new_fn(0, 0, 4);
    bool lt = s.lookup_tokens(inst_d, "你好", tokens);
    guint ntok = tokens ? ((GArrayPub *)tokens)->len : 0;
    printf("lookup_tokens(你好)=%s n=%u\n", yesno(lt), ntok);
    if (lt && ntok > 0) {
        phrase_token_t t0 =
            ((phrase_token_t *)((GArrayPub *)tokens)->data)[0];
        guint len = 0;
        gchar *ph = NULL;
        bool p1 = s.token_phrase(inst_d, t0, &len, &ph);
        printf("token=0x%08x phrase=%s/%u/%s\n", t0, yesno(p1), len,
               p1 && ph ? ph : "(null)");
        if (ph)
            g_free_fn(ph);
        guint freq = 0;
        bool u1 = s.token_unigram(inst_d, t0, &freq);
        printf("token_unigram=%s/%u\n", yesno(u1), freq);
    }
    g_array_free_fn(tokens, 1);
    s.free_inst(inst_d);
    s.fini(ctx_d);

    /* Fresh user dir for phase B so D cannot leak. */
    char userdir_b[] = "/tmp/residue-mech-b-XXXXXX";
    if (!mkdtemp(userdir_b)) {
        perror("mkdtemp B");
        rm_rf(userdir);
        return 1;
    }
    printf("=== phase: B-whole-row-train ===\n");
    fprintf(stderr, "userdir_b=%s\n", userdir_b);
    pinyin_context_t *ctx = s.init(argv[2], userdir_b);
    if (!ctx) {
        fprintf(stderr, "init failed for phase B\n");
        rm_rf(userdir);
        rm_rf(userdir_b);
        return 1;
    }
    s.set_options(ctx, 0x1e);
    /* USER_DICTIONARY = 7 */
    import_iterator_t *imp = s.begin_add(ctx, 7);
    printf("begin_add=%s\n", imp ? "ok" : "NULL");
    if (imp) {
        printf("add(你好世界)=%s\n",
               yesno(s.add_phrase(imp, "你好世界", "ni'hao'shi'jie", 9)));
        s.end_add(imp);
        printf("save=%s\n", yesno(s.save(ctx)));
    }
    pinyin_instance_t *inst = s.alloc(ctx);
    size_t parsed = s.parse(inst, "nihaoshijie");
    printf("parse(nihaoshijie)=%zu\n", parsed);
    bool gs = s.guess_sentence(inst);
    printf("guess_sentence=%s\n", yesno(gs));
    s.guess_cands(inst, 0, 0x1e);
    guint n = 0;
    s.getn(inst, &n);
    printf("n_candidate=%u\n", n);
    lookup_candidate_t *row0 = NULL;
    if (n == 0 || !s.getc(inst, 0, &row0) || !row0) {
        fprintf(stderr, "no row 0\n");
        rm_rf(userdir);
        rm_rf(userdir_b);
        return 1;
    }
    int ctype = -1;
    const gchar *text = NULL;
    uint8_t nb = 255;
    s.gettype(inst, row0, &ctype);
    s.getstr(inst, row0, &text);
    bool have_nb = s.nbest(inst, row0, &nb);
    printf("row0: type=%d text=%s nbest=%s/%u\n", ctype, text ? text : "(null)",
           yesno(have_nb), nb);
    int cur = s.choose(inst, 0, row0);
    printf("choose(0,row0)=%d\n", cur);
    /* clear_constraint(0) is true iff a CONSTRAINT_ONESTEP covers offset 0.
     * A whole-row choose of the 1-best runs diff_result(best,best) and
     * installs nothing — both sides should answer false here. */
    bool cleared = s.clear(inst, 0);
    printf("clear_constraint(0)=%s\n", yesno(cleared));
    dump_bigram(&s, ctx, "before_train");
    bool t1 = s.train(inst, 0);
    printf("train(0)=%s\n", yesno(t1));
    dump_bigram(&s, ctx, "after_train1");
    bool t2 = s.train(inst, 0);
    printf("train(0)again=%s\n", yesno(t2));
    dump_bigram(&s, ctx, "after_train2");
    s.free_inst(inst);
    s.fini(ctx);
    dlclose(handle);
    rm_rf(userdir);
    rm_rf(userdir_b);
    return 0;
}
