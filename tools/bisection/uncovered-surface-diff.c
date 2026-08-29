/*
 * uncovered-surface-diff.c — W12 parked live-typing coverage differential
 * driver for the four surfaces no frozen pin gates:
 *
 *   A. deep paging — the frontend's page down walks the candidate array
 *      the library already returned (ibus-libpinyin pages its own
 *      LookupTable, page size 5, PYPConfig.cc:148; the library ABI has no
 *      page calls). Walk pages 0..11 plus the last page, then choose from
 *      a deep page (index 10) and from the very last row.
 *   B. punctuation modes — full/half width and Chinese/English punct
 *      toggles are ibus-frontend state (PYHalfFullConverter.cc,
 *      PYPunctTable.h), NOT expressible through the pinned 2.11.91 C ABI
 *      (nm: no such exports). The ABI punct surface is the punct-table
 *      prediction path: pinyin_guess_predicted_candidates_with_punctuations
 *      prepends PREDICTED_PUNCTUATION rows per prefix token
 *      (pinyin.cpp:2454-2498), plus ASCII punctuation bytes in the
 *      composition (the parse-length surface).
 *   C. FORCE_TONE (1<<6) and DYNAMIC_ADJUST (1<<9) — the two option-bit
 *      classes the corpus never exercised (option-bits.md: FORCE_TONE
 *      absent from the engine, DYNAMIC_ADJUST bit-SET deferred #99).
 *   D. mid-composition cursor moves — the ABI readouts the frontend drives
 *      on Left/Right: pinyin_get_full_pinyin_auxiliary_text at every byte
 *      cursor, pinyin_get_pinyin_offset(cursor), the word-level
 *      left/right pinyin offsets, the candidate window at each moved
 *      cursor, and one mid-buffer choose.
 *   E. raw mid-syllable lookup offsets — pinyin_guess_candidates at every
 *      byte offset of nihao/nihaoshijie, fresh and after guess_sentence:
 *      the empty-matrix-column law (true with the n-best rows alone, no
 *      suffix re-parse) against the pin's search_matrix from that offset.
 *
 * Caller contracts honoured: get_sentence is asked only for proved
 * indices (0, plus NBEST nbest_index rows the window proves, clamped to
 * 2 — a past-the-rows index on a non-empty set aborts upstream,
 * pinyin.cpp:1474); predicted candidates are never passed to choose
 * (pinyin.cpp:2507 asserts); no scheme calls at all (zhuyin 7 / double 30
 * abort upstream).
 *
 * Usage: ./uncovered-surface-diff <path-to-so> <systemdir>
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

/* ── Scalar types (match pinyin.h / glib) ─────────────────────────────── */

typedef uint32_t guint;
typedef int32_t gint;
typedef char gchar;
typedef uint8_t guint8;

#define DEFAULT_SORT ((guint)0x1e) /* SORT_BY_PHRASE_LENGTH|PINYIN|FREQUENCY */
#define PAGE_SIZE ((guint)5)       /* fork default, PYPConfig.cc:148 */

/* The frozen parity option word (docs/findings/option-bits.md):
 * PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE (the
 * word carries no USE_TONE bit). The two libraries' bare defaults
 * differ (the capi instance starts at PINYIN_INCOMPLETE, the oracle at
 * USE_TONE), so the driver sets this word on both sides first — without
 * it a default-config artefact would masquerade as a divergence. */
#define PARITY_OPTIONS ((guint)0x18a)
#define USE_TONE ((guint)1 << 5)
#define FORCE_TONE ((guint)1 << 6)
#define DYNAMIC_ADJUST ((guint)1 << 9)
/* Composite option profiles: the parity word plus one bit each
 * (0x1ca, 0x38a), and USE_TONE|FORCE_TONE (0x60). */
#define PARITY_FORCE_TONE (PARITY_OPTIONS | FORCE_TONE)
#define PARITY_DYNAMIC_ADJUST (PARITY_OPTIONS | DYNAMIC_ADJUST)
#define USE_TONE_FORCE_TONE (USE_TONE | FORCE_TONE)

/* ── Function pointer types ───────────────────────────────────────────── */

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef pinyin_instance_t *(*fn_alloc)(pinyin_context_t *);
typedef void (*fn_free_instance)(pinyin_instance_t *);
typedef size_t (*fn_parse)(pinyin_instance_t *, const char *);
typedef bool (*fn_sentence)(pinyin_instance_t *);
typedef bool (*fn_guess)(pinyin_instance_t *, size_t, guint);
typedef bool (*fn_predict_punct)(pinyin_instance_t *, const char *);
typedef bool (*fn_getn)(pinyin_instance_t *, guint *);
typedef bool (*fn_getc)(pinyin_instance_t *, guint, lookup_candidate_t **);
typedef bool (*fn_gettype)(pinyin_instance_t *, lookup_candidate_t *, int *);
typedef bool (*fn_getstr)(pinyin_instance_t *, lookup_candidate_t *, const gchar **);
typedef bool (*fn_getnbest)(pinyin_instance_t *, lookup_candidate_t *, guint8 *);
typedef bool (*fn_get_sentence)(pinyin_instance_t *, guint8, gchar **);
typedef int (*fn_choose)(pinyin_instance_t *, size_t, lookup_candidate_t *);
typedef bool (*fn_reset)(pinyin_instance_t *);
typedef bool (*fn_set_options)(pinyin_context_t *, guint);
typedef bool (*fn_get_pinyin_offset)(pinyin_instance_t *, size_t, size_t *);
typedef bool (*fn_get_left_offset)(pinyin_instance_t *, size_t, size_t *);
typedef bool (*fn_get_right_offset)(pinyin_instance_t *, size_t, size_t *);
typedef bool (*fn_get_aux_text)(pinyin_instance_t *, size_t, gchar **);

struct syms {
    fn_init init;
    fn_fini fini;
    fn_alloc alloc;
    fn_free_instance free_instance;
    fn_parse parse;
    fn_sentence sentence;
    fn_guess guess;
    fn_predict_punct predict_punct;
    fn_getn getn;
    fn_getc getc;
    fn_gettype gettype;
    fn_getstr getstr;
    fn_getnbest getnbest;
    fn_get_sentence get_sentence;
    fn_choose choose;
    fn_reset reset;
    fn_set_options set_options;
    fn_get_pinyin_offset get_pinyin_offset;
    fn_get_left_offset get_left_offset;
    fn_get_right_offset get_right_offset;
    fn_get_aux_text get_aux_text;
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
    s->predict_punct = (fn_predict_punct)load(
        "pinyin_guess_predicted_candidates_with_punctuations", handle);
    s->getn = (fn_getn)load("pinyin_get_n_candidate", handle);
    s->getc = (fn_getc)load("pinyin_get_candidate", handle);
    s->gettype = (fn_gettype)load("pinyin_get_candidate_type", handle);
    s->getstr = (fn_getstr)load("pinyin_get_candidate_string", handle);
    s->getnbest = (fn_getnbest)load("pinyin_get_candidate_nbest_index", handle);
    s->get_sentence = (fn_get_sentence)load("pinyin_get_sentence", handle);
    s->choose = (fn_choose)load("pinyin_choose_candidate", handle);
    s->reset = (fn_reset)load("pinyin_reset", handle);
    s->set_options = (fn_set_options)load("pinyin_set_options", handle);
    s->get_pinyin_offset =
        (fn_get_pinyin_offset)load("pinyin_get_pinyin_offset", handle);
    s->get_left_offset =
        (fn_get_left_offset)load("pinyin_get_left_pinyin_offset", handle);
    s->get_right_offset =
        (fn_get_right_offset)load("pinyin_get_right_pinyin_offset", handle);
    s->get_aux_text =
        (fn_get_aux_text)load("pinyin_get_full_pinyin_auxiliary_text", handle);
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

/* ── Helpers (shared shape with live-typing-diff.c) ───────────────────── */

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

/* Prints candidate rows `from`..`to` (exclusive) of the current window as
 * `prefix:row[i]=TYPE/nbest/text`. Returns the largest NBEST nbest_index
 * seen (the get_sentence proof set), 0 when no NBEST row is walked.
 * Accessor failures return -1 so the caller can fail the run. */
static int print_candidate_rows(const struct syms *s, pinyin_instance_t *inst,
                                const char *prefix, guint from, guint to) {
    int proved = 0;
    for (guint i = from; i < to; i++) {
        lookup_candidate_t *cand = NULL;
        if (!s->getc(inst, i, &cand) || !cand) {
            fprintf(stderr, "%s: get_candidate(%u) failed\n", prefix, i);
            return -1;
        }
        int type = 0;
        if (!s->gettype(inst, cand, &type)) {
            fprintf(stderr, "%s: get_candidate_type(%u) failed\n", prefix, i);
            return -1;
        }
        const gchar *text = NULL;
        if (!s->getstr(inst, cand, &text)) {
            fprintf(stderr, "%s: get_candidate_string(%u) failed\n", prefix, i);
            return -1;
        }
        if (type == 1) { /* NBEST: the oracle asserts nbest_index is only
                          asked of NBEST rows */
            guint8 idx = 255;
            if (!s->getnbest(inst, cand, &idx)) {
                fprintf(stderr, "%s: get_candidate_nbest_index(%u) failed\n",
                        prefix, i);
                return -1;
            }
            if ((int)idx > proved)
                proved = (int)idx;
            printf("%s:row[%u]=%s/%u/%s\n", prefix, i, type_name(type),
                   (unsigned)idx, text ? text : "(null)");
        } else {
            printf("%s:row[%u]=%s/-/%s\n", prefix, i, type_name(type),
                   text ? text : "(null)");
        }
    }
    return proved;
}

/* get_sentence discipline: row 0 is safe whenever the guess succeeded
 * (a non-empty nbest set has size >= 1); rows 1..proved are proved by the
 * NBEST nbest_index values the candidate window carries (the corpus
 * discipline, live.rs:551-560). A past-the-rows index on a non-empty set
 * aborts upstream (pinyin.cpp:1474), so nothing unproved is asked. */
static int print_sentences(const struct syms *s, pinyin_instance_t *inst,
                           const char *label, int proved) {
    if (proved > 2)
        proved = 2;
    printf("%s:proved=%d\n", label, proved);
    for (int i = 0; i <= proved; i++) {
        gchar *text = NULL;
        bool got = s->get_sentence(inst, (guint8)i, &text);
        if (got && !text) {
            fprintf(stderr, "probe %s: get_sentence(%d) true with no text\n",
                    label, i);
            return 1;
        }
        printf("%s:sentence[%d]=%s\n", label, i, got ? text : "-");
        /* Caller-owned per the C ABI contract; g_free(NULL) is a no-op. */
        g_free_fn(text);
    }
    return 0;
}

/* guess_sentence → guess_candidates at `offset` → the candidate window
 * `from`..`to` plus the window count → the proved sentence rows, in the
 * frontend's order (updatePinyin then updateCandidates). Returns the
 * proved sentence index, or -1 on accessor failure. The guess retvals are
 * PRINTED, never bailed on: a `false` is a legitimate compared surface.
 * get_sentence discipline: row 0 is safe whenever `guessed` (a non-empty
 * nbest set has size >= 1, and size==0 returns false before the assert);
 * rows 1..proved are proved by the NBEST nbest_index values the window
 * walk just collected (the corpus discipline, live.rs:551-560). */
static int probe_window(const struct syms *s, pinyin_instance_t *inst,
                        const char *label, size_t offset, guint from,
                        guint to) {
    bool guessed = s->sentence(inst);
    printf("%s:guess=%d\n", label, (int)guessed);
    int proved = 0;
    if (s->guess(inst, offset, DEFAULT_SORT)) {
        guint n = 0;
        if (!s->getn(inst, &n)) {
            fprintf(stderr, "probe %s: get_n_candidate failed\n", label);
            return -1;
        }
        printf("%s:n=%u\n", label, n);
        guint hi = to < n ? to : n;
        if (hi < from)
            hi = from;
        proved = print_candidate_rows(s, inst, label, from, hi);
        if (proved < 0)
            return -1;
    } else {
        /* An empty-matrix window: n=0, no rows. A compared surface. */
        printf("%s:n=0\n", label);
    }
    if (guessed) {
        if (print_sentences(s, inst, label, proved))
            return -1;
    } else {
        /* Not proved: a stale non-empty nbest set cannot be ruled out,
         * and only indices >= 1 can abort — ask nothing. */
        printf("%s:proved=none\n", label);
    }
    return proved;
}

/* ── Phase A — deep paging ────────────────────────────────────────────── */

/* One input: parse, decode (frontend order: sentence then window), walk
 * the paged windows (pages 0..11 plus the last page when deeper), then
 * one deep choose. */
static int paging_input(const struct syms *s, pinyin_instance_t *inst,
                        const char *input, int round) {
    char label[72];
    snprintf(label, sizeof(label), "page-r%d-%s", round, input);

    size_t parsed = s->parse(inst, input);
    printf("%s:parsed=%zu\n", label, parsed);

    bool guessed = s->sentence(inst);
    printf("%s:guess=%d\n", label, (int)guessed);

    if (!s->guess(inst, 0, DEFAULT_SORT)) {
        printf("%s:n=0\n", label);
        return 0;
    }
    guint n = 0;
    if (!s->getn(inst, &n)) {
        fprintf(stderr, "%s: get_n_candidate failed\n", label);
        return 1;
    }
    printf("%s:n=%u\n", label, n);

    /* The frontend pages its own LookupTable over this list: page p holds
     * indices 5p..5p+4. Walk the first 12 pages, then the last page. */
    guint walk_end = n < 60 ? n : 60;
    int proved = 0;
    for (guint page = 0; page * PAGE_SIZE < walk_end; page++) {
        guint from = page * PAGE_SIZE;
        guint to = from + PAGE_SIZE;
        if (to > walk_end)
            to = walk_end;
        char page_label[96];
        snprintf(page_label, sizeof(page_label), "%s:page=%u", label, page);
        int p = print_candidate_rows(s, inst, page_label, from, to);
        if (p < 0)
            return 1;
        if (p > proved)
            proved = p;
    }
    if (n > 60) {
        char page_label[96];
        snprintf(page_label, sizeof(page_label), "%s:page=last", label);
        int p = print_candidate_rows(s, inst, page_label, n - PAGE_SIZE, n);
        if (p < 0)
            return 1;
        if (p > proved)
            proved = p;
    }

    /* The decoded sentence, rows proved by the walked NBEST rows. */
    if (guessed) {
        if (print_sentences(s, inst, label, proved))
            return 1;
    } else {
        printf("%s:proved=none\n", label);
    }

    /* Deep choose, round 1: index 10 (page 2 row 0) when offered. */
    guint deep = n > 10 ? 10 : (n > 5 ? 5 : 0);
    lookup_candidate_t *cand = NULL;
    if (s->getc(inst, deep, &cand) && cand) {
        int type = 0;
        s->gettype(inst, cand, &type);
        const gchar *text = NULL;
        s->getstr(inst, cand, &text);
        printf("%s:deep-choose idx=%u type=%s text=%s\n", label, deep,
               type_name(type), text ? text : "(null)");
        int cursor = s->choose(inst, 0, cand);
        printf("%s:cursor=%d\n", label, cursor);
        if (cursor >= 0) {
            char post[64];
            snprintf(post, sizeof(post), "page-r%d-%s-post", round, input);
            if (probe_window(s, inst, post, (size_t)cursor, 0, 8) < 0)
                return 1;
        }
    } else {
        printf("%s:deep-choose skipped (n=%u)\n", label, n);
    }
    return 0;
}

/* Round 2: choose the very LAST row (the deepest possible page). */
static int paging_last_row_choose(const struct syms *s, pinyin_instance_t *inst,
                                  const char *input) {
    char label[72];
    snprintf(label, sizeof(label), "page-last-%s", input);

    size_t parsed = s->parse(inst, input);
    printf("%s:parsed=%zu\n", label, parsed);
    if (!s->guess(inst, 0, DEFAULT_SORT)) {
        printf("%s:n=0\n", label);
        return 0;
    }
    guint n = 0;
    if (!s->getn(inst, &n)) {
        fprintf(stderr, "%s: get_n_candidate failed\n", label);
        return 1;
    }
    printf("%s:n=%u\n", label, n);
    if (n == 0)
        return 0;
    lookup_candidate_t *cand = NULL;
    if (!s->getc(inst, n - 1, &cand) || !cand) {
        fprintf(stderr, "%s: get_candidate(%u) failed\n", label, n - 1);
        return 1;
    }
    int type = 0;
    s->gettype(inst, cand, &type);
    const gchar *text = NULL;
    s->getstr(inst, cand, &text);
    printf("%s:tail-choose idx=%u type=%s text=%s\n", label, n - 1,
           type_name(type), text ? text : "(null)");
    int cursor = s->choose(inst, 0, cand);
    printf("%s:cursor=%d\n", label, cursor);
    return 0;
}

/* ── Phase B — punctuation modes ──────────────────────────────────────── */

/* Punct-table prediction for one prefix: retval, window count, the head
 * rows with types, and every PRED_PUNCT row wherever it sits. */
static int punct_prefix(const struct syms *s, pinyin_instance_t *inst,
                        const char *prefix, const char *tag) {
    bool ok = s->predict_punct(inst, prefix);
    printf("punct-%s:predict=%d\n", tag, (int)ok);
    guint n = 0;
    if (!s->getn(inst, &n)) {
        fprintf(stderr, "punct-%s: get_n_candidate failed\n", tag);
        return 1;
    }
    printf("punct-%s:n=%u\n", tag, n);
    guint head = n < 12 ? n : 12;
    for (guint i = 0; i < head; i++) {
        lookup_candidate_t *cand = NULL;
        if (!s->getc(inst, i, &cand) || !cand) {
            fprintf(stderr, "punct-%s: get_candidate(%u) failed\n", tag, i);
            return 1;
        }
        int type = 0;
        if (!s->gettype(inst, cand, &type)) {
            fprintf(stderr, "punct-%s: get_candidate_type(%u) failed\n", tag, i);
            return 1;
        }
        const gchar *text = NULL;
        if (!s->getstr(inst, cand, &text)) {
            fprintf(stderr, "punct-%s: get_candidate_string(%u) failed\n", tag, i);
            return 1;
        }
        printf("punct-%s:head[%u]=%s/-/%s\n", tag, i, type_name(type),
               text ? text : "(null)");
    }
    /* Every PRED_PUNCT row in the full window, in order. */
    guint seen = 0;
    for (guint i = 0; i < n; i++) {
        lookup_candidate_t *cand = NULL;
        if (!s->getc(inst, i, &cand) || !cand)
            continue;
        int type = 0;
        if (!s->gettype(inst, cand, &type))
            continue;
        if (type != 8)
            continue;
        const gchar *text = NULL;
        if (!s->getstr(inst, cand, &text))
            continue;
        printf("punct-%s:punct[%u]=%s\n", tag, i, text ? text : "(null)");
        seen++;
    }
    printf("punct-%s:punct-n=%u\n", tag, seen);
    return 0;
}

/* ASCII/full-width punctuation bytes in the composition: the parse-length
 * surface (the frontend re-parses the whole buffer; a punct byte stops
 * the pinyin parse). */
static int punct_parse_probe(const struct syms *s, pinyin_instance_t *inst,
                             const char *input, const char *tag) {
    size_t parsed = s->parse(inst, input);
    printf("punctparse-%s:parsed=%zu\n", tag, parsed);
    guint n = 0;
    if (s->guess(inst, 0, DEFAULT_SORT) && s->getn(inst, &n))
        printf("punctparse-%s:n=%u\n", tag, n);
    else
        printf("punctparse-%s:n=0\n", tag);
    return 0;
}

/* ── Phase C — option profiles ────────────────────────────────────────── */

/* One (profile, input) pair: set the option word (context-level, both
 * libraries honour live remasking), parse, window at `offset`, proved
 * sentence rows. */
static int opt_profile_input(const struct syms *s, pinyin_context_t *ctx,
                             pinyin_instance_t *inst, guint word,
                             const char *input, size_t offset) {
    char label[80];
    snprintf(label, sizeof(label), "opt:0x%x-%s@%zu", word, input, offset);
    if (!s->set_options(ctx, word)) {
        fprintf(stderr, "pinyin_set_options(0x%x) failed\n", (unsigned)word);
        return 1;
    }
    size_t parsed = s->parse(inst, input);
    printf("%s:parsed=%zu\n", label, parsed);
    if (probe_window(s, inst, label, offset, 0, 8) < 0)
        return 1;
    return 0;
}

/* ── Phase D — mid-composition cursor moves ───────────────────────────── */

/* Aux text + lookup offset at every byte cursor, word-level left/right
 * offsets at the syllable boundaries, the candidate window at each moved
 * cursor (the frontend's Left/Right arrows: m_cursor±1 then guess at
 * pinyin_offset(m_cursor)), and one mid-buffer choose. */
static int cursor_moves(const struct syms *s, pinyin_instance_t *inst,
                        const char *input) {
    size_t len = strlen(input);
    size_t parsed = s->parse(inst, input);
    printf("cur:parsed=%zu len=%zu\n", parsed, len);

    bool guessed = s->sentence(inst);
    printf("cur:guess=%d\n", (int)guessed);

    /* The cursor table: every byte position of the preedit. */
    for (size_t c = 0; c <= len; c++) {
        gchar *aux = NULL;
        bool aux_ok = s->get_aux_text(inst, c, &aux);
        size_t off = (size_t)-1;
        bool off_ok = s->get_pinyin_offset(inst, c, &off);
        printf("cur:%zu aux=%d:%s off=%d:%zu\n", c, (int)aux_ok,
               aux_ok && aux ? aux : "(null)", (int)off_ok,
               off_ok ? off : (size_t)-1);
        g_free_fn(aux);
    }

    /* Word-level moves at a subset of cursors. The pinned oracle's
     * get_left/right_pinyin_offset run a SECOND _check_offset on the
     * offset they compute (pinyin.cpp:3055/:3090 at the pin) and assert
     * there for tail cursors of this composition (measured: offset 11
     * aborts, pinyin.cpp:2175 — later upstream turned the assert into
     * `return false`, commit 95e3af7 "Fix _check_offset function"). The
     * probes below are the smoke-proved-safe cursors. Offset 8 is fully
     * measurable (get_left(8)=5, get_right(8)=11, both ok), so it is
     * measured with the rest; offset 11 is not probed — its right move
     * aborts, a frontend Ctrl+Right there aborts the pinned library, and
     * that is the pin's landmine, not a divergence to close. */
    static const size_t probes[] = {0, 2, 5, 8};
    for (size_t i = 0; i < sizeof(probes) / sizeof(probes[0]); i++) {
        size_t off = (size_t)-1;
        if (!s->get_pinyin_offset(inst, probes[i], &off)) {
            printf("cur:left-right@%zu off=false\n", probes[i]);
            continue;
        }
        size_t left = (size_t)-1, right = (size_t)-1;
        bool lok = s->get_left_offset(inst, off, &left);
        bool rok = s->get_right_offset(inst, off, &right);
        printf("cur:left-right@%zu off=%zu left=%d:%zu right=%d:%zu\n",
               probes[i], off, (int)lok, lok ? left : (size_t)-1, (int)rok,
               rok ? right : (size_t)-1);
    }

    /* Left-arrow walk: cursor from the tail inward, guessing at each
     * moved cursor's lookup offset (frontend: m_cursor-- then update). */
    for (size_t c = len; c-- > 0;) {
        size_t off = (size_t)-1;
        if (!s->get_pinyin_offset(inst, c, &off)) {
            printf("cur:left@%zu off=false\n", c);
            continue;
        }
        char label[48];
        snprintf(label, sizeof(label), "cur:left@%zu", c);
        if (!s->guess(inst, off, DEFAULT_SORT)) {
            printf("%s:n=0\n", label);
            continue;
        }
        guint n = 0;
        if (!s->getn(inst, &n)) {
            fprintf(stderr, "%s: get_n_candidate failed\n", label);
            return 1;
        }
        printf("%s:n=%u\n", label, n);
        guint hi = n < 4 ? n : 4;
        if (print_candidate_rows(s, inst, label, 0, hi) < 0)
            return 1;
    }

    /* Right-arrow walk: cursor from the head outward. */
    for (size_t c = 0; c < len; c++) {
        size_t off = (size_t)-1;
        if (!s->get_pinyin_offset(inst, c, &off)) {
            printf("cur:right@%zu off=false\n", c);
            continue;
        }
        char label[48];
        snprintf(label, sizeof(label), "cur:right@%zu", c);
        if (!s->guess(inst, off, DEFAULT_SORT)) {
            printf("%s:n=0\n", label);
            continue;
        }
        guint n = 0;
        if (!s->getn(inst, &n)) {
            fprintf(stderr, "%s: get_n_candidate failed\n", label);
            return 1;
        }
        printf("%s:n=%u\n", label, n);
        guint hi = n < 4 ? n : 4;
        if (print_candidate_rows(s, inst, label, 0, hi) < 0)
            return 1;
    }

    /* Mid-buffer choose after a cursor move: move to byte 5 ("nihao|"),
     * guess at its offset, choose row 0. */
    if (len >= 5) {
        size_t off = (size_t)-1;
        bool ok = s->get_pinyin_offset(inst, 5, &off);
        printf("cur:mid off=%d:%zu\n", (int)ok, ok ? off : (size_t)-1);
        if (ok) {
            if (!s->guess(inst, off, DEFAULT_SORT)) {
                printf("cur:mid:n=0\n");
            } else {
                guint n = 0;
                if (!s->getn(inst, &n)) {
                    fprintf(stderr, "cur:mid: get_n_candidate failed\n");
                    return 1;
                }
                printf("cur:mid:n=%u\n", n);
                if (print_candidate_rows(s, inst, "cur:mid", 0, n < 4 ? n : 4) < 0)
                    return 1;
                lookup_candidate_t *cand = NULL;
                if (n > 0 && s->getc(inst, 0, &cand) && cand) {
                    /* Row 0 of a mid-offset window can be a NBEST row;
                     * choose handles both NBEST and NORMAL. */
                    int cursor = s->choose(inst, off, cand);
                    printf("cur:mid:cursor=%d\n", cursor);
                    if (probe_window(s, inst, "cur:mid-post",
                                     cursor >= 0 ? (size_t)cursor : 0, 0, 8) < 0)
                        return 1;
                }
            }
        }
    }
    return 0;
}

/* ── Phase E — raw mid-syllable lookup offsets ────────────────────────── */

/* The empty-matrix-column surface the corpus never varied: guess at every
 * byte offset of the composition, fresh and after guess_sentence. The
 * compared surface per offset is the guess bool, the window count, and the
 * first four rows (rows past the head-4 prefix are not compared — the count
 * pins the window size). Bytes no matrix key starts on answer the pin's
 * empty-column law — true with the n-best rows alone (none fresh), never a
 * suffix re-parse — while syllable starts keep their windows. Apostrophe
 * inputs stay out: one past a lone zero-key column aborts the pinned
 * library (the recorded _check_offset landmine, not a comparable surface). */
static int raw_offset_input(const struct syms *s, pinyin_instance_t *inst,
                            const char *input) {
    char label[72];
    size_t len = s->parse(inst, input);
    snprintf(label, sizeof(label), "raw:%s", input);
    printf("%s:parsed=%zu\n", label, len);

    for (size_t off = 0; off <= len; off++) {
        char off_label[96];
        snprintf(off_label, sizeof(off_label), "raw:%s@%zu", input, off);
        bool ok = s->guess(inst, off, DEFAULT_SORT);
        printf("%s:guess=%d\n", off_label, (int)ok);
        if (!ok)
            continue;
        guint n = 0;
        if (!s->getn(inst, &n)) {
            fprintf(stderr, "%s: get_n_candidate failed\n", off_label);
            return 1;
        }
        printf("%s:n=%u\n", off_label, n);
        guint hi = n < 4 ? n : 4;
        if (print_candidate_rows(s, inst, off_label, 0, hi) < 0)
            return 1;
    }

    bool guessed = s->sentence(inst);
    printf("%s:sentence=%d\n", label, (int)guessed);
    for (size_t off = 0; off <= len; off++) {
        char off_label[96];
        snprintf(off_label, sizeof(off_label), "raw:%s+sent@%zu", input, off);
        bool ok = s->guess(inst, off, DEFAULT_SORT);
        printf("%s:guess=%d\n", off_label, (int)ok);
        if (!ok)
            continue;
        guint n = 0;
        if (!s->getn(inst, &n)) {
            fprintf(stderr, "%s: get_n_candidate failed\n", off_label);
            return 1;
        }
        printf("%s:n=%u\n", off_label, n);
        guint hi = n < 4 ? n : 4;
        if (print_candidate_rows(s, inst, off_label, 0, hi) < 0)
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

    void *handle = dlopen(argv[1], RTLD_NOW);
    if (!handle) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }
    struct syms s;
    memset(&s, 0, sizeof(s));
    resolve_all(handle, &s);
    resolve_g_free();

    char userdir[] = "/tmp/uncoveredsurface-user-XXXXXX";
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
        fprintf(stderr, "pinyin_set_options(0x%x) failed\n",
                (unsigned)PARITY_OPTIONS);
        goto fail_ctx;
    }
    pinyin_instance_t *inst = s.alloc(ctx);
    if (!inst) {
        fprintf(stderr, "pinyin_alloc_instance failed\n");
        goto fail_ctx;
    }

    /* Phase A — deep paging. */
    {
        static const char *const inputs[] = {"shi", "yi", "ji", "nihao"};
        int round = 0;
        for (size_t i = 0; i < sizeof(inputs) / sizeof(inputs[0]); i++) {
            if (paging_input(&s, inst, inputs[i], round))
                goto fail_inst;
            if (!s.reset(inst)) {
                fprintf(stderr, "paging reset failed\n");
                goto fail_inst;
            }
        }
        for (size_t i = 0; i < sizeof(inputs) / sizeof(inputs[0]); i++) {
            if (paging_last_row_choose(&s, inst, inputs[i]))
                goto fail_inst;
            if (!s.reset(inst)) {
                fprintf(stderr, "paging reset failed\n");
                goto fail_inst;
            }
        }
    }

    /* Phase B — punctuation modes. */
    {
        /* 好häo / 的 / 一 / 你 / 中国 / 我 / 是 / 了: a spread over the
         * punct table's token rows (full table: 272 tokens). */
        if (punct_prefix(&s, inst, "\xe5\xa5\xbd" /* 好 */, "hao") ||
            punct_prefix(&s, inst, "\xe7\x9a\x84" /* 的 */, "de") ||
            punct_prefix(&s, inst, "\xe4\xb8\x80" /* 一 */, "yi") ||
            punct_prefix(&s, inst, "\xe4\xbd\xa0" /* 你 */, "ni") ||
            punct_prefix(&s, inst,
                         "\xe4\xb8\xad\xe5\x9b\xbd" /* 中国 */, "zhongguo") ||
            punct_prefix(&s, inst, "\xe6\x88\x91" /* 我 */, "wo") ||
            punct_prefix(&s, inst, "\xe6\x98\xaf" /* 是 */, "shi") ||
            punct_prefix(&s, inst, "\xe4\xba\x86" /* 了 */, "le"))
            goto fail_inst;
        if (!s.reset(inst)) {
            fprintf(stderr, "punct reset failed\n");
            goto fail_inst;
        }
        /* Punctuation bytes in the composition: ASCII half-width
         * (, ' space), a tone digit mid-input under USE_TONE, and the
         * full-width ，: the parse stops where pinyin stops. */
        if (punct_parse_probe(&s, inst, "nihao,", "comma-tail") ||
            punct_parse_probe(&s, inst, "ni,hao", "comma-mid") ||
            punct_parse_probe(&s, inst, "ni'hao", "apos-mid") ||
            punct_parse_probe(&s, inst, "ni hao", "space-mid") ||
            punct_parse_probe(&s, inst, "ni2hao", "tone-mid") ||
            punct_parse_probe(&s, inst,
                              "\xef\xbc\x8cnihao" /* ，nihao */, "fullwidth"))
            goto fail_inst;
        if (!s.reset(inst)) {
            fprintf(stderr, "punct reset failed\n");
            goto fail_inst;
        }
    }

    /* Phase C — option profiles. */
    {
        /* Control: the parity word must hold (validates the harness). */
        if (opt_profile_input(&s, ctx, inst, PARITY_OPTIONS, "nihao", 0) ||
            opt_profile_input(&s, ctx, inst, PARITY_OPTIONS, "nihao", 2))
            goto fail_inst;
        if (!s.reset(inst)) {
            fprintf(stderr, "opt reset failed\n");
            goto fail_inst;
        }
        /* DYNAMIC_ADJUST bit-SET: the bigram term folds into candidate
         * frequency (option-bits.md, deferred #99 on the engine side). */
        if (opt_profile_input(&s, ctx, inst, PARITY_DYNAMIC_ADJUST, "nihao", 0) ||
            opt_profile_input(&s, ctx, inst, PARITY_DYNAMIC_ADJUST, "nihao", 2) ||
            opt_profile_input(&s, ctx, inst, PARITY_DYNAMIC_ADJUST, "nihaoshijie", 5))
            goto fail_inst;
        if (!s.reset(inst)) {
            fprintf(stderr, "opt reset failed\n");
            goto fail_inst;
        }
        /* FORCE_TONE: upstream rejects toneless syllables under
         * USE_TONE|FORCE_TONE (pinyin_parser2.cpp:186); the engine has
         * no FORCE_TONE handling at all. */
        static const struct {
            guint word;
            const char *input;
        } tone_inputs[] = {
            {PARITY_FORCE_TONE, "ni3hao3"},  {PARITY_FORCE_TONE, "nihao"},  {PARITY_FORCE_TONE, "zai4"},
            {PARITY_FORCE_TONE, "zhuang4"},  {PARITY_FORCE_TONE, "zai6"},   {PARITY_FORCE_TONE, "ni3"},
            {PARITY_FORCE_TONE, "shi4jie4"}, {USE_TONE_FORCE_TONE, "ni3hao3"}, {USE_TONE_FORCE_TONE, "nihao"},
            {USE_TONE_FORCE_TONE, "zai4"},      {USE_TONE_FORCE_TONE, "zai6"},
        };
        for (size_t i = 0; i < sizeof(tone_inputs) / sizeof(tone_inputs[0]); i++) {
            if (opt_profile_input(&s, ctx, inst, tone_inputs[i].word,
                                  tone_inputs[i].input, 0))
                goto fail_inst;
            if (!s.reset(inst)) {
                fprintf(stderr, "opt reset failed\n");
                goto fail_inst;
            }
        }
        /* Restore the parity word for later phases. */
        if (!s.set_options(ctx, PARITY_OPTIONS)) {
            fprintf(stderr, "pinyin_set_options(0x%x) restore failed\n",
                    (unsigned)PARITY_OPTIONS);
            goto fail_inst;
        }
    }

    /* Phase D — mid-composition cursor moves. */
    {
        if (cursor_moves(&s, inst, "nihaoshijie"))
            goto fail_inst;
        if (!s.reset(inst)) {
            fprintf(stderr, "cursor reset failed\n");
            goto fail_inst;
        }
    }

    /* Phase E — raw mid-syllable lookup offsets. Each input resets: a
     * prior input's guess_sentence leaves n-best rows that the pin keeps
     * across a reparse, which would masquerade as a divergence. */
    {
        static const char *const inputs[] = {"nihao", "nihaoshijie"};
        for (size_t i = 0; i < sizeof(inputs) / sizeof(inputs[0]); i++) {
            if (raw_offset_input(&s, inst, inputs[i]))
                goto fail_inst;
            if (!s.reset(inst)) {
                fprintf(stderr, "raw offset reset failed\n");
                goto fail_inst;
            }
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
