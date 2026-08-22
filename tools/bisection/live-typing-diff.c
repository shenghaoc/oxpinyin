/*
 * live-typing-diff.c — post-choose live-typing + decoded-continuation
 * train differential driver.
 *
 * Drives an identical scripted C-ABI sequence into a pinyin shared object
 * (libpinyin_capi.so or the pinned libpinyin.so) and prints the surfaces
 * no frozen pin gates: what the composition looks like keystroke by
 * keystroke AFTER a choose, and what self-learning records when the user
 * commits a decoded continuation instead of choosing every phrase.
 *
 * Phases:
 *
 *   baseline  — probe "nihao" on an empty user store (a choose-free
 *               control: both sides must agree before anything below).
 *   live type — choose 你 for "ni", then feed "haoshijie" one byte at a
 *               time; after each byte guess_sentence + guess_candidates
 *               at the cursor and print the sentence rows 0..2 plus the
 *               candidate head. No training in this phase: a NORMAL
 *               choose writes nothing to the user store (user-store.md
 *               §2.2), so the export below stays attributable to the
 *               train phase alone.
 *   train gap — the decoded-continuation case: parse "nihao", choose 你,
 *               re-decode so 好 remains decoded (NO second choose), then
 *               pinyin_train. Upstream's constraint-aware train_result3
 *               walks the constrained decode and trains 你→好; the
 *               engine's record holds only the explicitly chosen 你.
 *               Repeated LIVETYPING_ROUNDS times (default 3) so
 *               reselection doubling widens any count gap.
 *   export    — the user-store triples, one export per process.
 *
 * Usage:
 *   LIVETYPING_ROUNDS=<n> ./live-typing-diff <path-to-so> <systemdir>
 *   LIVETYPING_BACKSPACE=1 adds the opt-in backspace-after-choose phase
 *     (bp-*): its divergence is the recorded parse-survival divergence
 *     and is measured, not gated.
 */

#define _POSIX_C_SOURCE 200809L
#include <dirent.h>
#include <dlfcn.h>
#include <errno.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/* ── Opaque handle types (match pinyin.h) ─────────────────────────────── */

typedef void pinyin_context_t;
typedef void pinyin_instance_t;
typedef void lookup_candidate_t;
typedef void export_iterator_t;
typedef void bigram_export_iterator_t;

/* ── Scalar types (match pinyin.h / glib) ─────────────────────────────── */

typedef uint32_t guint;
typedef int32_t gint;
typedef char gchar;
typedef uint8_t guint8;

#define DEFAULT_SORT ((guint)0x1e) /* SORT_BY_PHRASE_LENGTH|PINYIN|FREQUENCY */

/* The frozen parity option word (docs/findings/option-bits.md): USE_TONE
 * | PINYIN_INCOMPLETE | 0x80 | 0x100. The two libraries' bare defaults
 * differ (the capi instance starts at PINYIN_INCOMPLETE, the oracle at
 * USE_TONE), so the driver sets this word on both sides first — the
 * compared surfaces then sit under the same configuration a parity run
 * uses, and a default-config artifact cannot masquerade as a divergence. */
#define PARITY_OPTIONS ((guint)0x18a)

/* ── Function pointer types ───────────────────────────────────────────── */

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef pinyin_instance_t *(*fn_alloc)(pinyin_context_t *);
typedef void (*fn_free_instance)(pinyin_instance_t *);
typedef size_t (*fn_parse)(pinyin_instance_t *, const char *);
typedef bool (*fn_sentence)(pinyin_instance_t *);
typedef bool (*fn_guess)(pinyin_instance_t *, size_t, guint);
typedef bool (*fn_getn)(pinyin_instance_t *, guint *);
typedef bool (*fn_getc)(pinyin_instance_t *, guint, lookup_candidate_t **);
typedef bool (*fn_gettype)(pinyin_instance_t *, lookup_candidate_t *, int *);
typedef bool (*fn_getstr)(pinyin_instance_t *, lookup_candidate_t *, const gchar **);
typedef bool (*fn_getnbest)(pinyin_instance_t *, lookup_candidate_t *, guint8 *);
typedef bool (*fn_get_sentence)(pinyin_instance_t *, guint8, gchar **);
typedef int (*fn_choose)(pinyin_instance_t *, size_t, lookup_candidate_t *);
typedef bool (*fn_train)(pinyin_instance_t *, uint8_t);
typedef bool (*fn_reset)(pinyin_instance_t *);
typedef bool (*fn_set_options)(pinyin_context_t *, guint);
typedef export_iterator_t *(*fn_begin_phrases)(pinyin_context_t *, guint);
typedef bool (*fn_has_next)(export_iterator_t *);
typedef bool (*fn_get_next)(export_iterator_t *, gchar **, gchar **, gint *);
typedef void (*fn_end_phrases)(export_iterator_t *);
typedef bigram_export_iterator_t *(*fn_begin_bigram)(pinyin_context_t *);
typedef bool (*fn_bigram_has_next)(bigram_export_iterator_t *);
typedef bool (*fn_bigram_get_next)(bigram_export_iterator_t *, gchar **, gchar **, gint *);
typedef void (*fn_end_bigram)(bigram_export_iterator_t *);

struct syms {
    fn_init init;
    fn_fini fini;
    fn_alloc alloc;
    fn_free_instance free_instance;
    fn_parse parse;
    fn_sentence sentence;
    fn_guess guess;
    fn_getn getn;
    fn_getc getc;
    fn_gettype gettype;
    fn_getstr getstr;
    fn_getnbest getnbest;
    fn_get_sentence get_sentence;
    fn_choose choose;
    fn_train train;
    fn_reset reset;
    fn_set_options set_options;
    fn_begin_phrases begin_phrases;
    fn_has_next has_next;
    fn_get_next get_next;
    fn_end_phrases end_phrases;
    fn_begin_bigram begin_bigram;
    fn_bigram_has_next bigram_has_next;
    fn_bigram_get_next bigram_get_next;
    fn_end_bigram end_bigram;
};

static void *load(const char *name, void *handle) {
    void *symbol = dlsym(handle, name);
    if (!symbol) {
        fprintf(stderr, "  MISSING: %s\n", name);
        exit(1);
    }
    return symbol;
}

static void resolve_all(void *handle, struct syms *s) {
    s->init = (fn_init)load("pinyin_init", handle);
    s->fini = (fn_fini)load("pinyin_fini", handle);
    s->alloc = (fn_alloc)load("pinyin_alloc_instance", handle);
    s->free_instance = (fn_free_instance)load("pinyin_free_instance", handle);
    s->parse = (fn_parse)load("pinyin_parse_more_full_pinyins", handle);
    s->sentence = (fn_sentence)load("pinyin_guess_sentence", handle);
    s->guess = (fn_guess)load("pinyin_guess_candidates", handle);
    s->getn = (fn_getn)load("pinyin_get_n_candidate", handle);
    s->getc = (fn_getc)load("pinyin_get_candidate", handle);
    s->gettype = (fn_gettype)load("pinyin_get_candidate_type", handle);
    s->getstr = (fn_getstr)load("pinyin_get_candidate_string", handle);
    s->getnbest = (fn_getnbest)load("pinyin_get_candidate_nbest_index", handle);
    s->get_sentence = (fn_get_sentence)load("pinyin_get_sentence", handle);
    s->choose = (fn_choose)load("pinyin_choose_candidate", handle);
    s->train = (fn_train)load("pinyin_train", handle);
    s->reset = (fn_reset)load("pinyin_reset", handle);
    s->set_options = (fn_set_options)load("pinyin_set_options", handle);
    s->begin_phrases = (fn_begin_phrases)load("pinyin_begin_get_phrases", handle);
    s->has_next = (fn_has_next)load("pinyin_iterator_has_next_phrase", handle);
    s->get_next = (fn_get_next)load("pinyin_iterator_get_next_phrase", handle);
    s->end_phrases = (fn_end_phrases)load("pinyin_end_get_phrases", handle);
    s->begin_bigram = (fn_begin_bigram)load("pinyin_begin_get_bigram_phrases", handle);
    s->bigram_has_next =
        (fn_bigram_has_next)load("pinyin_bigram_iterator_has_next_phrase", handle);
    s->bigram_get_next =
        (fn_bigram_get_next)load("pinyin_bigram_iterator_get_next_phrase", handle);
    s->end_bigram = (fn_end_bigram)load("pinyin_end_get_bigram_phrases", handle);
}

/* ── Caller-owned string release ──────────────────────────────────────── */

typedef void (*fn_g_free)(void *);
static fn_g_free g_free_fn;

static void resolve_g_free(void) {
    g_free_fn = (fn_g_free)free;
    void *glib = dlopen("libglib-2.0.so.0", RTLD_NOW);
    if (glib) {
        fn_g_free symbol = (fn_g_free)dlsym(glib, "g_free");
        if (symbol)
            g_free_fn = symbol;
    }
}

/* ── Helpers (shared shape with nbest-train-diff.c) ───────────────────── */

/* Removes `dir` and everything under it, so no exit path can leak the
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

/* Reads a rounds env var into `*out`, rejecting everything atoi would
 * silently accept: empty values, trailing text ("3junk"), conversion
 * errors, overflow, and anything outside 1..8. Unset keeps the caller's
 * default. */
static int parse_rounds_env(const char *name, int *out) {
    const char *value = getenv(name);
    if (!value)
        return 0;
    char *end = NULL;
    errno = 0;
    long parsed = strtol(value, &end, 10);
    if (value[0] == '\0' || *end != '\0' || errno != 0 ||
        parsed < 1 || parsed > 8) {
        fprintf(stderr, "%s must be an integer in 1..8, got \"%s\"\n",
                name, value);
        return 1;
    }
    *out = (int)parsed;
    return 0;
}

/* Parses `input` and requires it to consume exactly `expect` bytes — a
 * partial parse would silently shrink every later surface. */
static int parse_expect(const struct syms *s, pinyin_instance_t *inst,
                        const char *input, size_t expect) {
    size_t parsed = s->parse(inst, input);
    if (parsed != expect) {
        fprintf(stderr, "parse(%s) consumed %zu bytes, expected %zu\n",
                input, parsed, expect);
        return 1;
    }
    return 0;
}

static lookup_candidate_t *find_by_text(const struct syms *s,
                                        pinyin_instance_t *inst,
                                        const char *want) {
    guint count = 0;
    if (!s->getn(inst, &count)) {
        fprintf(stderr, "find_by_text: pinyin_get_n_candidate failed\n");
        return NULL;
    }
    for (guint i = 0; i < count; i++) {
        lookup_candidate_t *cand = NULL;
        if (!s->getc(inst, i, &cand) || !cand)
            continue;
        const gchar *text = NULL;
        s->getstr(inst, cand, &text);
        if (text && strcmp(text, want) == 0)
            return cand;
    }
    return NULL;
}

static const char *type_name(int type) {
    switch (type) {
    case 1: return "NBEST";
    case 2: return "NORMAL";
    case 3: return "ZOMBIE";
    case 4: return "PRED_BIGRAM";
    case 5: return "PRED_PREFIX";
    case 6: return "ADDON";
    case 7: return "LONGER";
    case 8: return "PRED_PUNCT";
    default: return "?";
    }
}

/* Prints sentence rows 0..=upto (the get_sentence half of a probe). A
 * `false` return is the expected empty row, printed as "-". Only indices
 * the candidate list proves may be asked: upstream's pinyin_get_sentence
 * answers false for an EMPTY result set but asserts `index <
 * results.size()` on a non-empty one (pinyin.cpp:1474), so an unproved
 * index is not a legal caller question — the frontend renders exactly
 * the NBEST rows the candidate list carries. */
static int print_sentences(const struct syms *s, pinyin_instance_t *inst,
                           const char *label, int upto) {
    for (int i = 0; i <= upto && i < 3; i++) {
        gchar *text = NULL;
        bool got = s->get_sentence(inst, (guint8)i, &text);
        if (got && !text) {
            fprintf(stderr, "probe %s: get_sentence(%d) true with no text\n",
                    label, i);
            return 1;
        }
        printf("probe:%s sentence[%d]=%s\n", label, i, got ? text : "-");
        /* Caller-owned per the C ABI contract; g_free(NULL) is a no-op. */
        g_free_fn(text);
    }
    return 0;
}

/* guess_sentence → guess_candidates at `offset` → the candidate head →
 * the sentence rows the head proves. Keeps the instance state (no
 * reset), so the live-typing phase can chain calls. The guess retval is
 * PRINTED, never bailed on: a `false` is a legitimate compared surface
 * here — with the input fully consumed the capi answers false while the
 * oracle still walks the constrained full matrix and emits the forced
 * phrase as a row. Returns 1 only on accessor failures. */
static int probe_at(const struct syms *s, pinyin_instance_t *inst,
                    const char *label, size_t offset) {
    bool guessed = s->sentence(inst);
    printf("probe:%s guess=%d\n", label, (int)guessed);
    if (!s->guess(inst, offset, DEFAULT_SORT)) {
        fprintf(stderr, "probe %s: guess_candidates failed\n", label);
        return 1;
    }
    guint n = 0;
    if (!s->getn(inst, &n)) {
        fprintf(stderr, "probe %s: get_n_candidate failed\n", label);
        return 1;
    }
    printf("probe:%s n=%u\n", label, n);
    guint limit = n < 8 ? n : 8;
    int proven = -1;
    for (guint i = 0; i < limit; i++) {
        lookup_candidate_t *cand = NULL;
        if (!s->getc(inst, i, &cand) || !cand) {
            fprintf(stderr, "probe %s: get_candidate(%u) failed\n", label, i);
            return 1;
        }
        int type = 0;
        if (!s->gettype(inst, cand, &type)) {
            fprintf(stderr, "probe %s: get_candidate_type(%u) failed\n", label, i);
            return 1;
        }
        const gchar *text = NULL;
        if (!s->getstr(inst, cand, &text)) {
            fprintf(stderr, "probe %s: get_candidate_string(%u) failed\n", label, i);
            return 1;
        }
        if (type == 1) { /* NBEST: the oracle asserts nbest_index is only
                          asked of NBEST rows */
            guint8 idx = 255;
            if (!s->getnbest(inst, cand, &idx)) {
                fprintf(stderr, "probe %s: get_candidate_nbest_index(%u) failed\n",
                        label, i);
                return 1;
            }
            if ((int)idx > proven)
                proven = idx;
            printf("probe:%s cand[%u]=%s/%u/%s\n", label, i, type_name(type),
                   (unsigned)idx, text ? text : "(null)");
        } else {
            printf("probe:%s cand[%u]=%s/-/%s\n", label, i, type_name(type),
                   text ? text : "(null)");
        }
    }
    /* The window proves the rows it carries; no NBEST row means the
     * lookup produced none, and no index may be asked. */
    return print_sentences(s, inst, label, proven);
}

/* Phase B keystroke: re-parse the FULL accumulated buffer — both
 * libraries' parse_more replaces the composition with the passed string
 * (the frontend contract: the whole preedit buffer is re-sent every
 * keystroke), so a one-byte call would clobber the composition. The
 * parse return is a compared ABI surface, printed here; the probes below
 * carry the rest. */
static int type_step(const struct syms *s, pinyin_instance_t *inst,
                     const char *label, const char *buffer, size_t cursor) {
    size_t parsed = s->parse(inst, buffer);
    printf("probe:%s parsed=%zu\n", label, parsed);
    return probe_at(s, inst, label, cursor);
}

/* Phase C: one decoded-continuation training round. Parse "nihao", choose
 * 你 for "ni", re-decode so 好 stays decoded (no second choose), train.
 * Returns 1 on any failure; the caller routes it through the cleanup. */
static int train_decoded_continuation(const struct syms *s,
                                      pinyin_instance_t *inst, int round) {
    char pre[32], post[32];
    snprintf(pre, sizeof(pre), "core-r%d-pre", round);
    snprintf(post, sizeof(post), "core-r%d-post", round);

    if (parse_expect(s, inst, "nihao", 5))
        return 1;
    if (probe_at(s, inst, pre, 0))
        return 1;

    lookup_candidate_t *ni = find_by_text(s, inst, "\xe4\xbd\xa0" /* 你 */);
    if (!ni) {
        fprintf(stderr, "round %d: candidate 你 not offered\n", round);
        return 1;
    }
    int cursor = s->choose(inst, 0, ni);
    if (cursor < 0) {
        fprintf(stderr, "round %d: choose 你 failed\n", round);
        return 1;
    }
    printf("round:%d cursor=%d\n", round, cursor);

    /* The constrained re-decode: 好 remains decoded, never chosen. */
    if (probe_at(s, inst, post, (size_t)cursor))
        return 1;

    bool trained = s->train(inst, 0);
    printf("round:%d train=%d\n", round, (int)trained);
    if (!trained) {
        fprintf(stderr, "round %d: pinyin_train failed\n", round);
        return 1;
    }
    if (!s->reset(inst)) {
        fprintf(stderr, "round %d: reset failed\n", round);
        return 1;
    }
    return 0;
}

/* ── Main ─────────────────────────────────────────────────────────────── */

int main(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "Usage: %s <path-to-so> <systemdir>\n", argv[0]);
        return 1;
    }

    int rounds = 3;
    if (parse_rounds_env("LIVETYPING_ROUNDS", &rounds))
        return 1;
    /* Opt-in: the backspace-after-choose phase (LIVETYPING_BACKSPACE=1).
     * Its divergence is the recorded parse-survival divergence
     * (upstream-divergences.md: upstream's constraints survive every
     * re-parse, the engine continues only an extending one), so it must
     * not run in the default diff — that one gates the closed L-classes
     * and stays green. */
    const char *flag = getenv("LIVETYPING_BACKSPACE");
    int backspace_phase = flag && flag[0] == '1' && flag[1] == '\0';
    if (flag && !backspace_phase) {
        fprintf(stderr, "LIVETYPING_BACKSPACE must be 1 or unset, got \"%s\"\n", flag);
        return 1;
    }

    void *handle = dlopen(argv[1], RTLD_NOW);
    if (!handle) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }
    struct syms s;
    memset(&s, 0, sizeof(s));
    resolve_all(handle, &s);
    resolve_g_free();

    char userdir[] = "/tmp/livetypingdiff-user-XXXXXX";
    if (!mkdtemp(userdir)) {
        perror("mkdtemp");
        return 1;
    }

    pinyin_context_t *ctx = s.init(argv[2], userdir);
    if (!ctx) {
        fprintf(stderr, "pinyin_init failed (real-unigram systemdir required)\n");
        goto fail;
    }
    if (!s.set_options(ctx, PARITY_OPTIONS)) {
        fprintf(stderr, "pinyin_set_options(0x%x) failed\n", (unsigned)PARITY_OPTIONS);
        goto fail_ctx;
    }
    pinyin_instance_t *inst = s.alloc(ctx);
    if (!inst) {
        fprintf(stderr, "pinyin_alloc_instance failed\n");
        goto fail_ctx;
    }

    /* Phase A — baseline: choose-free control on an empty user store. */
    if (parse_expect(&s, inst, "nihao", 5) ||
        probe_at(&s, inst, "baseline", 0) ||
        !s.reset(inst)) {
        fprintf(stderr, "baseline probe failed\n");
        goto fail_inst;
    }

    /* Phase B0 — terminal choose: choosing 你 for the WHOLE input "ni"
     * leaves nothing to decode. The oracle still walks the constrained
     * full matrix and emits the forced phrase as rows; the engine's
     * remaining-input model has nothing. */
    {
        if (parse_expect(&s, inst, "ni", 2)) {
            fprintf(stderr, "term-choose: pre-choose parse failed\n");
            goto fail_inst;
        }
        if (!s.sentence(inst) || !s.guess(inst, 0, DEFAULT_SORT)) {
            fprintf(stderr, "term-choose: pre-choose decode failed\n");
            goto fail_inst;
        }
        lookup_candidate_t *ni0 = find_by_text(&s, inst, "\xe4\xbd\xa0" /* 你 */);
        if (!ni0) {
            fprintf(stderr, "term-choose: candidate 你 not offered\n");
            goto fail_inst;
        }
        int cursor0 = s.choose(inst, 0, ni0);
        if (cursor0 < 0) {
            fprintf(stderr, "term-choose: choose 你 failed\n");
            goto fail_inst;
        }
        printf("term:cursor=%d\n", cursor0);
        if (probe_at(&s, inst, "term-after-choose", (size_t)cursor0)) {
            fprintf(stderr, "term-choose: after-choose probe failed\n");
            goto fail_inst;
        }
        if (!s.reset(inst)) {
            fprintf(stderr, "term-choose: reset failed\n");
            goto fail_inst;
        }
    }

    /* Phase B — live typing after a choose over remaining input. A NORMAL
     * choose trains nothing (user-store.md §2.2), so this phase leaves
     * the user store empty and the export stays attributable to phase C.
     * The parse after the choose re-sends the whole buffer: upstream's
     * instance-level constraints survive that, the engine's session-level
     * selection does not (parse resets the session). */
    {
        static const char *const steps[] = {
            "nihaos", "nihaosh", "nihaoshi", "nihaoshij",
            "nihaoshiji", "nihaoshijie",
        };
        if (parse_expect(&s, inst, "nihao", 5)) {
            fprintf(stderr, "live-typing: pre-choose parse failed\n");
            goto fail_inst;
        }
        if (!s.sentence(inst) || !s.guess(inst, 0, DEFAULT_SORT)) {
            fprintf(stderr, "live-typing: pre-choose decode failed\n");
            goto fail_inst;
        }
        lookup_candidate_t *ni = find_by_text(&s, inst, "\xe4\xbd\xa0" /* 你 */);
        if (!ni) {
            fprintf(stderr, "live-typing: candidate 你 not offered\n");
            goto fail_inst;
        }
        int cursor = s.choose(inst, 0, ni);
        if (cursor < 0) {
            fprintf(stderr, "live-typing: choose 你 failed\n");
            goto fail_inst;
        }
        printf("live:cursor=%d\n", cursor);
        if (probe_at(&s, inst, "after-choose", (size_t)cursor)) {
            fprintf(stderr, "live-typing: after-choose probe failed\n");
            goto fail_inst;
        }
        char label[32];
        for (size_t i = 0; i < sizeof(steps) / sizeof(steps[0]); i++) {
            snprintf(label, sizeof(label), "type-%s", steps[i] + 5);
            if (type_step(&s, inst, label, steps[i], (size_t)cursor)) {
                fprintf(stderr, "live-typing: step %s failed\n", steps[i]);
                goto fail_inst;
            }
        }
        if (!s.reset(inst)) {
            fprintf(stderr, "live-typing: reset failed\n");
            goto fail_inst;
        }
    }

    /* Phase BP — backspace after a choose (opt-in). The frontend's
     * backspace edits its own buffer and re-parses the SHORTER string,
     * so the probe re-parses down a shrink ladder after choosing 你 for
     * "ni", then re-types past the shrink. No training: the phase leaves
     * the user store untouched. The ladder stops at "ni" — the cursor
     * (2) must never overrun one-past-end, where upstream's
     * _check_offset aborts. */
    if (backspace_phase) {
        static const char *const ladder[] = {
            "nihaoshiji", "nihaoshij", "nihaoshi", "nihaosh",
            "nihaos",     "nihao",     "niha",     "nih",     "ni",
        };
        if (parse_expect(&s, inst, "nihaoshijie", 11)) {
            fprintf(stderr, "backspace: pre-choose parse failed\n");
            goto fail_inst;
        }
        if (!s.sentence(inst) || !s.guess(inst, 0, DEFAULT_SORT)) {
            fprintf(stderr, "backspace: pre-choose decode failed\n");
            goto fail_inst;
        }
        lookup_candidate_t *bp_ni = find_by_text(&s, inst, "\xe4\xbd\xa0" /* 你 */);
        if (!bp_ni) {
            fprintf(stderr, "backspace: candidate 你 not offered\n");
            goto fail_inst;
        }
        int bp_cursor = s.choose(inst, 0, bp_ni);
        if (bp_cursor < 0) {
            fprintf(stderr, "backspace: choose 你 failed\n");
            goto fail_inst;
        }
        printf("bp:cursor=%d\n", bp_cursor);
        /* The ladder's shortest rung is "ni" (2 bytes): a cursor past it
         * would drive pinyin_guess_candidates beyond one-past-end, where
         * upstream's _check_offset aborts — a SIGABRT instead of a
         * reported divergence. Fail the run loudly if the oracle ever
         * answers a larger cursor for this choose. */
        if (bp_cursor > 2) {
            fprintf(stderr,
                    "backspace: cursor %d overruns the shortest rung \"ni\" "
                    "(2); refusing to drive the oracle past one-past-end\n",
                    bp_cursor);
            goto fail_inst;
        }
        char label[32];
        for (size_t i = 0; i < sizeof(ladder) / sizeof(ladder[0]); i++) {
            snprintf(label, sizeof(label), "bp-%s", ladder[i]);
            if (type_step(&s, inst, label, ladder[i], (size_t)bp_cursor)) {
                fprintf(stderr, "backspace: shrink %s failed\n", ladder[i]);
                goto fail_inst;
            }
        }
        /* Re-type past the shrink: the composition re-extends. */
        if (type_step(&s, inst, "bp-retype", "nihaoshijie", (size_t)bp_cursor)) {
            fprintf(stderr, "backspace: retype failed\n");
            goto fail_inst;
        }
        if (!s.reset(inst)) {
            fprintf(stderr, "backspace: reset failed\n");
            goto fail_inst;
        }
    }

    /* Phase C — decoded-continuation training rounds. */
    for (int round = 1; round <= rounds; round++) {
        if (train_decoded_continuation(&s, inst, round)) {
            fprintf(stderr, "train round %d failed\n", round);
            goto fail_inst;
        }
    }

    /* Phase D — the landed user state, one export per process. */
    {
        export_iterator_t *iter = s.begin_phrases(ctx, 7 /* USER_DICTIONARY */);
        if (iter) {
            while (s.has_next(iter)) {
                gchar *phrase = NULL;
                gchar *pinyin = NULL;
                gint count = -1;
                if (!s.get_next(iter, &phrase, &pinyin, &count))
                    break;
                printf("phrase: %s|%s|%d\n", phrase ? phrase : "(null)",
                       pinyin ? pinyin : "(null)", (int)count);
                g_free_fn(phrase);
                g_free_fn(pinyin);
            }
            s.end_phrases(iter);
        }
        bigram_export_iterator_t *biter = s.begin_bigram(ctx);
        if (biter) {
            while (s.bigram_has_next(biter)) {
                gchar *phrase = NULL;
                gchar *pinyin = NULL;
                gint count = -1;
                /* Upstream's bigram get_next fills the out-params and
                 * returns whether MORE rows follow, not whether the call
                 * succeeded (user-store.md §9, mirrored from train-diff):
                 * print the filled row either way, and stop the loop when
                 * it reports no more. */
                bool more = s.bigram_get_next(biter, &phrase, &pinyin, &count);
                printf("bigram: %s|%s|%d\n", phrase ? phrase : "(null)",
                       pinyin ? pinyin : "(null)", (int)count);
                g_free_fn(phrase);
                g_free_fn(pinyin);
                if (!more)
                    break;
            }
            s.end_bigram(biter);
        }
    }

    s.free_instance(inst);
    s.fini(ctx);
    dlclose(handle);
    rm_rf(userdir);
    return 0;

    /* Shared cleanup: no exit path leaks the mkdtemp userdir. */
fail_inst:
    s.free_instance(inst);
fail_ctx:
    s.fini(ctx);
    dlclose(handle);
fail:
    rm_rf(userdir);
    return 1;
}
