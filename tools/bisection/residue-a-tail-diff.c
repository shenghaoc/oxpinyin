/*
 * residue-a-tail-diff.c — ABI-level probe for residue A in
 * docs/findings/probe-coverage-abi.md (the pin's rank-0 user-phrase tail
 * that oxpinyin's k-best never produces) and for the common-root test
 * against residues B and C. Diagnosis only: prints the observables that
 * decide the question; asserts nothing. The runner diffs two logs.
 *
 * Every phase starts from the ABI probe's import state on a fresh user
 * dir: set_options(0x18a), import 你好/5, 你好世界/9, 测试/3 into the
 * user dictionary, save.
 *
 * Phase A (tails):   parse nihaoshijie, guess_sentence, the user-token
 *                    inventory (lookup_tokens + unigram per token), the
 *                    sentence texts for every proved n-best index, the
 *                    0x1e window head.
 * Phase C (window):  guess_candidates(0, 0x1f) after the guess — is the
 *                    imported NORMAL row still there; re-parse, 0x1e —
 *                    are the n-best rows still there.
 * Phase B (train):   fresh instance, guess, choose row 0 (NBEST),
 *                    clear_constraint(0), train twice; the bigram export
 *                    AND the unigram of the user 你好世界 token and of
 *                    system 你好 / 世界 before and after each train —
 *                    the export skips sentence_start pairs on both
 *                    sides, so the unigram is the observable that cannot
 *                    be masked.
 * Phase X (cross):   fresh instance, guess, choose the NBEST row whose
 *                    text is 你好时节 (visible on both sides), then
 *                    clear_constraint(0) and (5): which phrases the
 *                    diff against the 1-best forced; re-choose on a fresh
 *                    instance, guess again (the ibus shape), train once;
 *                    export and unigrams.
 * Phase D (adjust):  context at 0x38a (DYNAMIC_ADJUST on), guess, then
 *                    guess_candidates(5, 0x1e): the window whose bigram
 *                    term reads result[0]'s token at offset 5.
 * Phase E (behind):  the window at an offset BEHIND the composition
 *                    offset a choose advanced to. (E1) whole-composition
 *                    NBEST choose + re-guess, then guess_candidates at
 *                    0/0x1e, 0/0x1f, 5/0x1e and 11/0x1e; (E2) a NORMAL
 *                    你好 choose (cursor 5), then guess_candidates at
 *                    0/0x1e, 0/0x1f and 5/0x1e.
 *
 * Usage:
 *   ./residue-a-tail-diff <path-to-so> <systemdir>
 *
 * Exit: 0 on a completed walk; 1 on setup/crash.
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
typedef bool (*fn_get_sentence)(pinyin_instance_t *, uint8_t, char **);
typedef bool (*fn_guess_cands)(pinyin_instance_t *, size_t, guint);
typedef bool (*fn_getn)(pinyin_instance_t *, guint *);
typedef bool (*fn_getc)(pinyin_instance_t *, guint, lookup_candidate_t **);
typedef bool (*fn_getstr)(pinyin_instance_t *, lookup_candidate_t *, const gchar **);
typedef bool (*fn_gettype)(pinyin_instance_t *, lookup_candidate_t *, int *);
typedef bool (*fn_nbest)(pinyin_instance_t *, lookup_candidate_t *, uint8_t *);
typedef bool (*fn_is_user)(pinyin_instance_t *, lookup_candidate_t *);
typedef int (*fn_choose)(pinyin_instance_t *, size_t, lookup_candidate_t *);
typedef bool (*fn_clear)(pinyin_instance_t *, size_t);
typedef bool (*fn_train)(pinyin_instance_t *, uint8_t);
typedef bool (*fn_reset)(pinyin_instance_t *);
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
    fn_get_sentence get_sentence;
    fn_guess_cands guess_cands;
    fn_getn getn;
    fn_getc getc;
    fn_getstr getstr;
    fn_gettype gettype;
    fn_nbest nbest;
    fn_is_user is_user;
    fn_choose choose;
    fn_clear clear;
    fn_train train;
    fn_reset reset;
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

#define USER_DICT_INDEX 7u
#define PARITY_WORD 0x18au
#define DYNAMIC_ADJUST_WORD 0x38au
#define NBEST_MATCH_CANDIDATE 1

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

/* One fresh context in the ABI probe's import state, on its own user dir. */
static pinyin_context_t *import_context(struct syms *s, const char *systemdir,
                                        uint32_t options, char *userdir_out) {
    strcpy(userdir_out, "/tmp/residue-a-XXXXXX");
    if (!mkdtemp(userdir_out)) {
        perror("mkdtemp");
        return NULL;
    }
    fprintf(stderr, "userdir=%s\n", userdir_out);
    pinyin_context_t *ctx = s->init(systemdir, userdir_out);
    printf("init=%s\n", ctx ? "ok" : "NULL");
    if (!ctx)
        return NULL;
    printf("set_options(0x%x)=%s\n", options, yesno(s->set_options(ctx, options)));
    import_iterator_t *imp = s->begin_add(ctx, USER_DICT_INDEX);
    printf("begin_add(%u)=%s\n", USER_DICT_INDEX, imp ? "ok" : "NULL");
    if (imp) {
        printf("add(你好/ni'hao/5)=%s\n", yesno(s->add_phrase(imp, "你好", "ni'hao", 5)));
        printf("add(你好世界/ni'hao'shi'jie/9)=%s\n",
               yesno(s->add_phrase(imp, "你好世界", "ni'hao'shi'jie", 9)));
        printf("add(测试/ce'shi/3)=%s\n", yesno(s->add_phrase(imp, "测试", "ce'shi", 3)));
        s->end_add(imp);
    }
    printf("save=%s\n", yesno(s->save(ctx)));
    return ctx;
}

/* Every token behind `phrase`, with its library nibble, text and unigram. */
static void dump_tokens(struct syms *s, pinyin_instance_t *inst, const char *tag,
                        const char *phrase) {
    void *tokens = g_array_new_fn(0, 0, 4);
    bool lt = s->lookup_tokens(inst, phrase, tokens);
    guint n = tokens ? ((GArrayPub *)tokens)->len : 0;
    printf("%s:lookup_tokens(%s)=%s n=%u\n", tag, phrase, yesno(lt), n);
    for (guint i = 0; i < n; i++) {
        phrase_token_t t = ((phrase_token_t *)((GArrayPub *)tokens)->data)[i];
        guint len = 0;
        gchar *ph = NULL;
        bool p1 = s->token_phrase(inst, t, &len, &ph);
        guint freq = 0;
        bool u1 = s->token_unigram(inst, t, &freq);
        printf("%s:token[%u]=0x%08x lib=%u phrase=%s/%u/%s unigram=%s/%u\n", tag, i, t,
               (t >> 24) & 0xff, yesno(p1), len, p1 && ph ? ph : "(null)", yesno(u1),
               freq);
        if (ph)
            g_free_fn(ph);
    }
    g_array_free_fn(tokens, 1);
}

/* The unigram of the first token of `phrase` in library `lib` (0 = any). */
static void dump_unigram_of(struct syms *s, pinyin_instance_t *inst, const char *tag,
                            const char *phrase, unsigned lib) {
    void *tokens = g_array_new_fn(0, 0, 4);
    bool lt = s->lookup_tokens(inst, phrase, tokens);
    guint n = lt && tokens ? ((GArrayPub *)tokens)->len : 0;
    bool found = false;
    for (guint i = 0; i < n && !found; i++) {
        phrase_token_t t = ((phrase_token_t *)((GArrayPub *)tokens)->data)[i];
        if (lib != 0 && ((t >> 24) & 0xff) != lib)
            continue;
        guint freq = 0;
        bool u1 = s->token_unigram(inst, t, &freq);
        printf("%s:unigram(%s,lib=%u)=0x%08x %s/%u\n", tag, phrase, lib, t, yesno(u1),
               freq);
        found = true;
    }
    if (!found)
        printf("%s:unigram(%s,lib=%u)=absent\n", tag, phrase, lib);
    g_array_free_fn(tokens, 1);
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

/* The window head: type, text, n-best rank (NBEST rows only), is_user.
 * Returns the highest n-best rank seen, or -1. */
static int dump_rows(struct syms *s, pinyin_instance_t *inst, const char *tag,
                     guint limit) {
    guint n = 0;
    bool got = s->getn(inst, &n);
    printf("%s:n_cand=%s n=%u\n", tag, yesno(got), n);
    int max_rank = -1;
    if (!got)
        return max_rank;
    guint shown = n > limit ? limit : n;
    unsigned nbest_rows = 0, user_rows = 0;
    for (guint i = 0; i < n; i++) {
        lookup_candidate_t *c = NULL;
        bool ok = s->getc(inst, i, &c) && c;
        if (!ok) {
            if (i < shown)
                printf("%s:cand[%u]=false\n", tag, i);
            continue;
        }
        int type = -1;
        bool t_ok = s->gettype(inst, c, &type);
        const char *text = NULL;
        bool s_ok = s->getstr(inst, c, &text) && text;
        uint8_t rank = 255;
        bool r_ok = false;
        if (t_ok && type == NBEST_MATCH_CANDIDATE) {
            r_ok = s->nbest(inst, c, &rank);
            nbest_rows++;
            if (r_ok && (int)rank > max_rank)
                max_rank = rank;
        }
        bool user = s->is_user(inst, c);
        if (user)
            user_rows++;
        if (i < shown)
            printf("%s:cand[%u] type=%d text=%s nbest=%s/%u is_user=%s\n", tag, i, type,
                   s_ok ? text : "(null)", yesno(r_ok), rank, yesno(user));
        if (s_ok && user && strcmp(text, "你好世界") == 0)
            printf("%s:user_row(你好世界)=present at %u type=%d\n", tag, i, type);
    }
    printf("%s:nbest_rows=%u user_rows=%u max_rank=%d\n", tag, nbest_rows, user_rows,
           max_rank);
    return max_rank;
}

/* The NBEST row whose text is `text`, or NULL. */
static lookup_candidate_t *find_nbest_row(struct syms *s, pinyin_instance_t *inst,
                                          const char *text, guint *index_out) {
    guint n = 0;
    if (!s->getn(inst, &n))
        return NULL;
    for (guint i = 0; i < n; i++) {
        lookup_candidate_t *c = NULL;
        if (!s->getc(inst, i, &c) || !c)
            continue;
        int type = -1;
        if (!s->gettype(inst, c, &type) || type != NBEST_MATCH_CANDIDATE)
            continue;
        const char *t = NULL;
        if (s->getstr(inst, c, &t) && t && strcmp(t, text) == 0) {
            *index_out = i;
            return c;
        }
    }
    return NULL;
}

static void dump_sentences(struct syms *s, pinyin_instance_t *inst, const char *tag,
                           int max_rank) {
    for (int i = 0; i <= max_rank; i++) {
        char *sent = NULL;
        bool ok = s->get_sentence(inst, (uint8_t)i, &sent);
        printf("%s:sentence[%d]=%s text=%s\n", tag, i, yesno(ok), ok && sent ? sent : "(null)");
        if (sent)
            g_free_fn(sent);
    }
}

static void dump_train_unigrams(struct syms *s, pinyin_instance_t *inst, const char *tag) {
    dump_unigram_of(s, inst, tag, "你好世界", USER_DICT_INDEX);
    dump_unigram_of(s, inst, tag, "你好", USER_DICT_INDEX);
    dump_unigram_of(s, inst, tag, "你好", 1);
    dump_unigram_of(s, inst, tag, "世界", 1);
    dump_unigram_of(s, inst, tag, "时节", 1);
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
    s.get_sentence = (fn_get_sentence)must(handle, "pinyin_get_sentence");
    s.guess_cands = (fn_guess_cands)must(handle, "pinyin_guess_candidates");
    s.getn = (fn_getn)must(handle, "pinyin_get_n_candidate");
    s.getc = (fn_getc)must(handle, "pinyin_get_candidate");
    s.getstr = (fn_getstr)must(handle, "pinyin_get_candidate_string");
    s.gettype = (fn_gettype)must(handle, "pinyin_get_candidate_type");
    s.nbest = (fn_nbest)must(handle, "pinyin_get_candidate_nbest_index");
    s.is_user = (fn_is_user)must(handle, "pinyin_is_user_candidate");
    s.choose = (fn_choose)must(handle, "pinyin_choose_candidate");
    s.clear = (fn_clear)must(handle, "pinyin_clear_constraint");
    s.train = (fn_train)must(handle, "pinyin_train");
    s.reset = (fn_reset)must(handle, "pinyin_reset");
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

    fprintf(stderr, "so=%s\n", argv[1]);
    fprintf(stderr, "systemdir=%s\n", argv[2]);

    /* ── Phase A: the tails, the token inventory, the 0x1e window ── */
    printf("=== phase: A-tails ===\n");
    char userdir_a[64];
    pinyin_context_t *ctx = import_context(&s, argv[2], PARITY_WORD, userdir_a);
    if (!ctx)
        return 1;
    pinyin_instance_t *inst = s.alloc(ctx);
    dump_tokens(&s, inst, "A", "你好世界");
    dump_tokens(&s, inst, "A", "你好");
    dump_tokens(&s, inst, "A", "世界");
    dump_tokens(&s, inst, "A", "时节");
    dump_tokens(&s, inst, "A", "是届");
    printf("A:parse(nihaoshijie)=%zu\n", s.parse(inst, "nihaoshijie"));
    printf("A:guess_sentence=%s\n", yesno(s.guess_sentence(inst)));
    printf("A:guess_candidates(0,0x1e)=%s\n", yesno(s.guess_cands(inst, 0, 0x1e)));
    int max_rank = dump_rows(&s, inst, "A-1e", 6);
    /* get_sentence(i) is asked only for proved indices: the pin asserts
     * index < size. Every rank below the highest visible one exists. */
    dump_sentences(&s, inst, "A", max_rank);

    /* ── Phase C: the imported NORMAL after the guess; n-best across parse ── */
    printf("=== phase: C-window ===\n");
    printf("C:guess_candidates(0,0x1f)=%s\n", yesno(s.guess_cands(inst, 0, 0x1f)));
    dump_rows(&s, inst, "C-1f", 4);
    printf("C:reparse(nihaoshijie)=%zu\n", s.parse(inst, "nihaoshijie"));
    printf("C:guess_candidates(0,0x1e)=%s\n", yesno(s.guess_cands(inst, 0, 0x1e)));
    dump_rows(&s, inst, "C-reparse-1e", 4);
    s.free_inst(inst);

    /* ── Phase B: whole-row choose + train, export and unigrams ── */
    printf("=== phase: B-train ===\n");
    inst = s.alloc(ctx);
    printf("B:parse(nihaoshijie)=%zu\n", s.parse(inst, "nihaoshijie"));
    printf("B:guess_sentence=%s\n", yesno(s.guess_sentence(inst)));
    printf("B:guess_candidates(0,0x1e)=%s\n", yesno(s.guess_cands(inst, 0, 0x1e)));
    lookup_candidate_t *row0 = NULL;
    guint n = 0;
    s.getn(inst, &n);
    if (n == 0 || !s.getc(inst, 0, &row0) || !row0) {
        fprintf(stderr, "no row 0\n");
        s.free_inst(inst);
        s.fini(ctx);
        rm_rf(userdir_a);
        return 1;
    }
    int ctype = -1;
    const gchar *text = NULL;
    uint8_t nb = 255;
    s.gettype(inst, row0, &ctype);
    s.getstr(inst, row0, &text);
    bool have_nb = ctype == NBEST_MATCH_CANDIDATE && s.nbest(inst, row0, &nb);
    printf("B:row0 type=%d text=%s nbest=%s/%u\n", ctype, text ? text : "(null)",
           yesno(have_nb), nb);
    printf("B:choose(0,row0)=%d\n", s.choose(inst, 0, row0));
    printf("B:clear_constraint(0)=%s\n", yesno(s.clear(inst, 0)));
    dump_bigram(&s, ctx, "B-before");
    dump_train_unigrams(&s, inst, "B-before");
    printf("B:train(0)=%s\n", yesno(s.train(inst, 0)));
    dump_bigram(&s, ctx, "B-train1");
    dump_train_unigrams(&s, inst, "B-train1");
    printf("B:train(0)again=%s\n", yesno(s.train(inst, 0)));
    dump_bigram(&s, ctx, "B-train2");
    dump_train_unigrams(&s, inst, "B-train2");
    s.free_inst(inst);
    s.fini(ctx);
    rm_rf(userdir_a);

    /* ── Phase X: choose the 你好时节 row — what the diff against the 1-best forces ── */
    printf("=== phase: X-cross ===\n");
    char userdir_x[64];
    ctx = import_context(&s, argv[2], PARITY_WORD, userdir_x);
    if (!ctx)
        return 1;
    inst = s.alloc(ctx);
    printf("X:parse(nihaoshijie)=%zu\n", s.parse(inst, "nihaoshijie"));
    printf("X:guess_sentence=%s\n", yesno(s.guess_sentence(inst)));
    printf("X:guess_candidates(0,0x1e)=%s\n", yesno(s.guess_cands(inst, 0, 0x1e)));
    guint xi = 0;
    lookup_candidate_t *xrow = find_nbest_row(&s, inst, "你好时节", &xi);
    printf("X:row(你好时节)=%s at %u\n", xrow ? "found" : "absent", xi);
    if (xrow) {
        uint8_t xr = 255;
        bool xr_ok = s.nbest(inst, xrow, &xr);
        printf("X:row(你好时节).nbest=%s/%u\n", yesno(xr_ok), xr);
        printf("X:choose(0,row)=%d\n", s.choose(inst, 0, xrow));
        /* clear_constraint answers true iff a OneStep covers the offset:
         * a 1-best of one whole-input token differs from 你好+时节 at
         * both phrases; a 1-best of 你好+世界 differs only at the second. */
        printf("X:clear_constraint(0)=%s\n", yesno(s.clear(inst, 0)));
        printf("X:clear_constraint(5)=%s\n", yesno(s.clear(inst, 5)));
    }
    s.free_inst(inst);
    /* The clears above emptied the store; a fresh instance re-chooses so
     * the train below sees the forcings the choose installed. */
    inst = s.alloc(ctx);
    printf("X2:parse(nihaoshijie)=%zu\n", s.parse(inst, "nihaoshijie"));
    printf("X2:guess_sentence=%s\n", yesno(s.guess_sentence(inst)));
    printf("X2:guess_candidates(0,0x1e)=%s\n", yesno(s.guess_cands(inst, 0, 0x1e)));
    xrow = find_nbest_row(&s, inst, "你好时节", &xi);
    if (xrow) {
        printf("X2:choose(0,row)=%d\n", s.choose(inst, 0, xrow));
        /* The ibus shape: re-decode under the forcings before training —
         * the pin's train_result3 asserts result[0] against the store. */
        printf("X2:guess_sentence(again)=%s\n", yesno(s.guess_sentence(inst)));
        printf("X2:guess_candidates(0,0x1e)=%s\n", yesno(s.guess_cands(inst, 0, 0x1e)));
        dump_rows(&s, inst, "X2-1e", 3);
        dump_sentences(&s, inst, "X2", 0);
        dump_train_unigrams(&s, inst, "X2-before");
        printf("X2:train(0)=%s\n", yesno(s.train(inst, 0)));
        dump_bigram(&s, ctx, "X2-train1");
        dump_train_unigrams(&s, inst, "X2-train1");
    }
    s.free_inst(inst);
    s.fini(ctx);
    rm_rf(userdir_x);

    /* ── Phase D: DYNAMIC_ADJUST reads result[0] at offset 5 ── */
    printf("=== phase: D-dynamic-adjust ===\n");
    char userdir_d[64];
    ctx = import_context(&s, argv[2], DYNAMIC_ADJUST_WORD, userdir_d);
    if (!ctx)
        return 1;
    inst = s.alloc(ctx);
    printf("D:parse(nihaoshijie)=%zu\n", s.parse(inst, "nihaoshijie"));
    printf("D:guess_sentence=%s\n", yesno(s.guess_sentence(inst)));
    printf("D:guess_candidates(5,0x1e)=%s\n", yesno(s.guess_cands(inst, 5, 0x1e)));
    /* The whole window: the bigram term reorders below the head, if at all. */
    dump_rows(&s, inst, "D-5", 400);
    printf("D:guess_candidates(0,0x1e)=%s\n", yesno(s.guess_cands(inst, 0, 0x1e)));
    dump_rows(&s, inst, "D-0", 4);
    s.free_inst(inst);
    s.fini(ctx);
    rm_rf(userdir_d);

    /* ── Phase E: windows behind the composition offset ── */
    printf("=== phase: E-behind ===\n");
    char userdir_e[64];
    ctx = import_context(&s, argv[2], PARITY_WORD, userdir_e);
    if (!ctx)
        return 1;
    /* E1: whole-composition NBEST choose (cursor 11), re-guess, windows. */
    inst = s.alloc(ctx);
    printf("E1:parse(nihaoshijie)=%zu\n", s.parse(inst, "nihaoshijie"));
    printf("E1:guess_sentence=%s\n", yesno(s.guess_sentence(inst)));
    printf("E1:guess_candidates(0,0x1e)=%s\n", yesno(s.guess_cands(inst, 0, 0x1e)));
    lookup_candidate_t *erow = NULL;
    n = 0;
    s.getn(inst, &n);
    if (n > 0 && s.getc(inst, 0, &erow) && erow) {
        printf("E1:choose(0,row0)=%d\n", s.choose(inst, 0, erow));
        printf("E1:guess_sentence(again)=%s\n", yesno(s.guess_sentence(inst)));
        printf("E1:guess_candidates(0,0x1e)=%s\n", yesno(s.guess_cands(inst, 0, 0x1e)));
        dump_rows(&s, inst, "E1-0-1e", 4);
        printf("E1:guess_candidates(0,0x1f)=%s\n", yesno(s.guess_cands(inst, 0, 0x1f)));
        dump_rows(&s, inst, "E1-0-1f", 4);
        printf("E1:guess_candidates(5,0x1e)=%s\n", yesno(s.guess_cands(inst, 5, 0x1e)));
        dump_rows(&s, inst, "E1-5-1e", 4);
        printf("E1:guess_candidates(11,0x1e)=%s\n", yesno(s.guess_cands(inst, 11, 0x1e)));
        dump_rows(&s, inst, "E1-11-1e", 4);
    }
    s.free_inst(inst);
    /* E2: a NORMAL 你好 choose (cursor 5), then the windows at 0 and 5. */
    inst = s.alloc(ctx);
    printf("E2:parse(nihaoshijie)=%zu\n", s.parse(inst, "nihaoshijie"));
    printf("E2:guess_sentence=%s\n", yesno(s.guess_sentence(inst)));
    printf("E2:guess_candidates(0,0x1e)=%s\n", yesno(s.guess_cands(inst, 0, 0x1e)));
    n = 0;
    s.getn(inst, &n);
    lookup_candidate_t *nrow = NULL;
    guint ni = 0;
    for (guint i = 0; i < n; i++) {
        lookup_candidate_t *c = NULL;
        int type = -1;
        const char *t = NULL;
        if (s.getc(inst, i, &c) && c && s.gettype(inst, c, &type) && type == 2 &&
            s.getstr(inst, c, &t) && t && strcmp(t, "你好") == 0 && !s.is_user(inst, c)) {
            nrow = c;
            ni = i;
            break;
        }
    }
    printf("E2:row(NORMAL 你好)=%s at %u\n", nrow ? "found" : "absent", ni);
    if (nrow) {
        printf("E2:choose(0,row)=%d\n", s.choose(inst, 0, nrow));
        printf("E2:guess_sentence(again)=%s\n", yesno(s.guess_sentence(inst)));
        printf("E2:guess_candidates(0,0x1e)=%s\n", yesno(s.guess_cands(inst, 0, 0x1e)));
        dump_rows(&s, inst, "E2-0-1e", 4);
        printf("E2:guess_candidates(0,0x1f)=%s\n", yesno(s.guess_cands(inst, 0, 0x1f)));
        dump_rows(&s, inst, "E2-0-1f", 4);
        printf("E2:guess_candidates(5,0x1e)=%s\n", yesno(s.guess_cands(inst, 5, 0x1e)));
        dump_rows(&s, inst, "E2-5-1e", 4);
    }
    s.free_inst(inst);
    s.fini(ctx);
    rm_rf(userdir_e);

    dlclose(handle);
    return 0;
}
