/*
 * bisect.c — three-subject ABI bisection fixture.
 *
 * Loads a pinyin shared object (libpinyin.so or libpinyin_capi.so) via
 * dlopen and runs its measurement modes: --perf, --perf-sentence,
 * --perf-typing-sequence and --teardown.
 *
 * Usage:
 *   ./bisect --perf <path-to-so> <systemdir>   # perf/RAM JSON line
 *
 * Build:
 *   gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o bisect bisect.c -ldl
 */

/* _DEFAULT_SOURCE alongside _POSIX_C_SOURCE: the RSS diagnostic mode calls
 * malloc_trim(3) and mallinfo2(3), glibc extensions that a bare
 * _POSIX_C_SOURCE definition hides; syscall(2) backs the perf_event_open
 * wrapper for the hardware instruction counter. Darwin exposes mkdtemp(3)
 * and getline(3) only through _DARWIN_C_SOURCE. */
#if defined(__APPLE__)
#define _DARWIN_C_SOURCE
#endif
#define _DEFAULT_SOURCE
#define _XOPEN_SOURCE 700
#define _POSIX_C_SOURCE 200809L
#if !defined(_WIN32)
#include <dlfcn.h>
#endif
#include <errno.h>
#if !defined(_WIN32)
#include <ftw.h>
#endif
#include <stdbool.h>
#include <stdint.h>
#if defined(__GLIBC__)
#include <malloc.h>
#endif
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#if !defined(_WIN32)
#include <sys/ioctl.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>
#else
#include <windows.h>
#include <psapi.h>
#endif
#if defined(__APPLE__)
#include <libproc.h>
#include <mach-o/loader.h>
#include <mach/mach_time.h>
#include <sys/resource.h>
#endif
#if defined(__linux__)
#include <linux/perf_event.h>
#include <sys/syscall.h>
#else
/* The hardware counter is Linux-only; the call sites stay compiled but are
 * unreachable because perf_open_instruction_counter() always returns -1. */
#define PERF_EVENT_IOC_ENABLE 0
#define PERF_EVENT_IOC_DISABLE 0
#define PERF_EVENT_IOC_RESET 0
#endif

#include "driver-common.h"

/* ── Opaque handle types (match pinyin.h) ─────────────────────────────── */

typedef void pinyin_context_t;
typedef void pinyin_instance_t;
typedef void lookup_candidate_t;
typedef void ChewingKeyRest;
typedef void import_iterator_t;
typedef void export_iterator_t;
typedef void bigram_export_iterator_t;

/* ── Scalar types (match pinyin.h / glib) ─────────────────────────────── */

typedef uint32_t pinyin_option_t;
typedef uint32_t guint;
typedef int32_t  gint;
typedef char     gchar;
typedef uint32_t phrase_token_t;

/* ── Frontend call profile (matches tools/capture/capture.c) ──────────── */

#define IS_PINYIN         (1u << 1)
#define PINYIN_INCOMPLETE (1u << 3)
#define USE_DIVIDED_TABLE (1u << 7)
#define USE_RESPLIT_TABLE (1u << 8)

/* F-A capture profile: IS_PINYIN | PINYIN_INCOMPLETE | USE_DIVIDED_TABLE |
 * USE_RESPLIT_TABLE == 0x0000018a. */
#define DEFAULT_FLAGS \
    ((pinyin_option_t)(IS_PINYIN | PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | \
                       USE_RESPLIT_TABLE))

/* SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY. */
#define DEFAULT_SORT ((guint)0x1e)

/* ── Candidate type enum ──────────────────────────────────────────────── */

typedef enum {
    NBEST_MATCH_CANDIDATE         = 1,
    NORMAL_CANDIDATE              = 2,
    ZOMBIE_CANDIDATE              = 3,
    PREDICTED_BIGRAM_CANDIDATE    = 4,
    PREDICTED_PREFIX_CANDIDATE    = 5,
    ADDON_CANDIDATE               = 6,
    LONGER_CANDIDATE              = 7,
    PREDICTED_PUNCTUATION_CANDIDATE = 8,
} lookup_candidate_type_t;

/* ── Function pointer types for all 51 live symbols ───────────────────── */

/* 1a. Context lifecycle */
typedef pinyin_context_t *  (*fn_pinyin_init)(const char *, const char *);
typedef void                (*fn_pinyin_fini)(pinyin_context_t *);
typedef pinyin_instance_t * (*fn_pinyin_alloc_instance)(pinyin_context_t *);
typedef void                (*fn_pinyin_free_instance)(pinyin_instance_t *);

/* 1b. Configuration */
typedef bool (*fn_pinyin_set_options)(pinyin_context_t *, pinyin_option_t);
typedef bool (*fn_pinyin_set_double_pinyin_scheme)(pinyin_context_t *, int);
typedef bool (*fn_pinyin_set_zhuyin_scheme)(pinyin_context_t *, int);
typedef bool (*fn_pinyin_load_addon_phrase_library)(pinyin_context_t *, uint8_t);
typedef bool (*fn_pinyin_save)(pinyin_context_t *);

/* 1c. Parsing */
typedef size_t   (*fn_pinyin_parse_more_full_pinyins)(pinyin_instance_t *, const char *);
typedef size_t   (*fn_pinyin_parse_more_double_pinyins)(pinyin_instance_t *, const char *);
typedef size_t   (*fn_pinyin_parse_more_chewings)(pinyin_instance_t *, const char *);
typedef size_t   (*fn_pinyin_get_parsed_input_length)(pinyin_instance_t *);
typedef bool (*fn_pinyin_in_chewing_keyboard)(pinyin_instance_t *, char, gchar ***);

/* 1d. Sentence / guess */
typedef bool (*fn_pinyin_guess_sentence)(pinyin_instance_t *);
typedef bool (*fn_pinyin_guess_candidates)(pinyin_instance_t *, size_t, guint);
typedef bool (*fn_pinyin_guess_predicted)(pinyin_instance_t *, const char *);
typedef bool (*fn_pinyin_reset)(pinyin_instance_t *);

/* 1e. Candidate access */
typedef bool (*fn_pinyin_get_n_candidate)(pinyin_instance_t *, guint *);
typedef bool (*fn_pinyin_get_candidate)(pinyin_instance_t *, guint, lookup_candidate_t **);
typedef bool (*fn_pinyin_get_candidate_type)(pinyin_instance_t *, lookup_candidate_t *, lookup_candidate_type_t *);
typedef bool (*fn_pinyin_get_candidate_string)(pinyin_instance_t *, lookup_candidate_t *, const gchar **);
typedef bool (*fn_pinyin_get_candidate_nbest_index)(pinyin_instance_t *, lookup_candidate_t *, uint8_t *);
typedef bool (*fn_pinyin_is_user_candidate)(pinyin_instance_t *, lookup_candidate_t *);
typedef bool (*fn_pinyin_remove_user_candidate)(pinyin_instance_t *, lookup_candidate_t *);

/* 1f. Selection / training */
typedef gint     (*fn_pinyin_choose_candidate)(pinyin_instance_t *, size_t, lookup_candidate_t *);
typedef bool (*fn_pinyin_choose_predicted_candidate)(pinyin_instance_t *, lookup_candidate_t *);
typedef bool (*fn_pinyin_train)(pinyin_instance_t *, uint8_t);

/* 1g. Sentence retrieval */
typedef bool (*fn_pinyin_get_sentence)(pinyin_instance_t *, uint8_t, char **);
typedef bool (*fn_pinyin_get_character_offset)(pinyin_instance_t *, const char *, size_t, size_t *);

/* 1h. Cursor / key rest */
typedef bool (*fn_pinyin_get_pinyin_key_rest)(pinyin_instance_t *, size_t, ChewingKeyRest **);
typedef bool (*fn_pinyin_get_pinyin_key_rest_positions)(pinyin_instance_t *, ChewingKeyRest *, uint16_t *, uint16_t *);
typedef bool (*fn_pinyin_get_pinyin_offset)(pinyin_instance_t *, size_t, size_t *);
typedef bool (*fn_pinyin_get_left_pinyin_offset)(pinyin_instance_t *, size_t, size_t *);
typedef bool (*fn_pinyin_get_right_pinyin_offset)(pinyin_instance_t *, size_t, size_t *);

/* 1i. Auxiliary text */
typedef bool (*fn_pinyin_get_full_pinyin_auxiliary_text)(pinyin_instance_t *, size_t, gchar **);
typedef bool (*fn_pinyin_get_double_pinyin_auxiliary_text)(pinyin_instance_t *, size_t, gchar **);
typedef bool (*fn_pinyin_get_chewing_auxiliary_text)(pinyin_instance_t *, size_t, gchar **);

/* 1j. User data */
typedef bool           (*fn_pinyin_mask_out)(pinyin_context_t *, phrase_token_t, phrase_token_t);
typedef bool           (*fn_pinyin_remember_user_input)(pinyin_instance_t *, const char *, gint);
typedef import_iterator_t *(*fn_pinyin_begin_add_phrases)(pinyin_context_t *, uint8_t);
typedef bool           (*fn_pinyin_iterator_add_phrase)(import_iterator_t *, const char *, const char *, gint);
typedef void               (*fn_pinyin_end_add_phrases)(import_iterator_t *);

/* 1k. Phrase / bigram export */
typedef export_iterator_t *(*fn_pinyin_begin_get_phrases)(pinyin_context_t *, guint);
typedef bool           (*fn_pinyin_iterator_has_next_phrase)(export_iterator_t *);
typedef bool           (*fn_pinyin_iterator_get_next_phrase)(export_iterator_t *, gchar **, gchar **, gint *);
typedef void               (*fn_pinyin_end_get_phrases)(export_iterator_t *);
typedef bigram_export_iterator_t *(*fn_pinyin_begin_get_bigram_phrases)(pinyin_context_t *);
typedef bool           (*fn_pinyin_bigram_iterator_has_next_phrase)(bigram_export_iterator_t *);
typedef bool           (*fn_pinyin_bigram_iterator_get_next_phrase)(bigram_export_iterator_t *, gchar **, gchar **, gint *);
typedef void               (*fn_pinyin_end_get_bigram_phrases)(bigram_export_iterator_t *);

/* ── Symbol table ─────────────────────────────────────────────────────── */

struct symbols {
    /* 1a */
    fn_pinyin_init                       init;
    fn_pinyin_fini                       fini;
    fn_pinyin_alloc_instance             alloc_instance;
    fn_pinyin_free_instance              free_instance;
    /* 1b */
    fn_pinyin_set_options                set_options;
    fn_pinyin_set_double_pinyin_scheme   set_double_pinyin_scheme;
    fn_pinyin_set_zhuyin_scheme          set_zhuyin_scheme;
    fn_pinyin_load_addon_phrase_library  load_addon_phrase_library;
    fn_pinyin_save                       save;
    /* 1c */
    fn_pinyin_parse_more_full_pinyins    parse_full;
    fn_pinyin_parse_more_double_pinyins  parse_double;
    fn_pinyin_parse_more_chewings        parse_chewing;
    fn_pinyin_get_parsed_input_length    get_parsed_input_length;
    fn_pinyin_in_chewing_keyboard        in_chewing_keyboard;
    /* 1d */
    fn_pinyin_guess_sentence             guess_sentence;
    fn_pinyin_guess_candidates           guess_candidates;
    fn_pinyin_guess_predicted            guess_predicted;
    fn_pinyin_reset                      reset;
    /* 1e */
    fn_pinyin_get_n_candidate            get_n_candidate;
    fn_pinyin_get_candidate              get_candidate;
    fn_pinyin_get_candidate_type         get_candidate_type;
    fn_pinyin_get_candidate_string       get_candidate_string;
    fn_pinyin_get_candidate_nbest_index  get_candidate_nbest_index;
    fn_pinyin_is_user_candidate          is_user_candidate;
    fn_pinyin_remove_user_candidate      remove_user_candidate;
    /* 1f */
    fn_pinyin_choose_candidate           choose_candidate;
    fn_pinyin_choose_predicted_candidate choose_predicted_candidate;
    fn_pinyin_train                      train;
    /* 1g */
    fn_pinyin_get_sentence               get_sentence;
    fn_pinyin_get_character_offset       get_character_offset;
    /* 1h */
    fn_pinyin_get_pinyin_key_rest            get_pinyin_key_rest;
    fn_pinyin_get_pinyin_key_rest_positions  get_pinyin_key_rest_positions;
    fn_pinyin_get_pinyin_offset              get_pinyin_offset;
    fn_pinyin_get_left_pinyin_offset         get_left_pinyin_offset;
    fn_pinyin_get_right_pinyin_offset        get_right_pinyin_offset;
    /* 1i */
    fn_pinyin_get_full_pinyin_auxiliary_text    get_full_aux;
    fn_pinyin_get_double_pinyin_auxiliary_text  get_double_aux;
    fn_pinyin_get_chewing_auxiliary_text        get_chewing_aux;
    /* 1j */
    fn_pinyin_mask_out                  mask_out;
    fn_pinyin_remember_user_input       remember_user_input;
    fn_pinyin_begin_add_phrases         begin_add_phrases;
    fn_pinyin_iterator_add_phrase       iterator_add_phrase;
    fn_pinyin_end_add_phrases           end_add_phrases;
    /* 1k */
    fn_pinyin_begin_get_phrases              begin_get_phrases;
    fn_pinyin_iterator_has_next_phrase        iterator_has_next;
    fn_pinyin_iterator_get_next_phrase        iterator_get_next;
    fn_pinyin_end_get_phrases                end_get_phrases;
    fn_pinyin_begin_get_bigram_phrases       begin_get_bigram;
    fn_pinyin_bigram_iterator_has_next_phrase bigram_has_next;
    fn_pinyin_bigram_iterator_get_next_phrase bigram_get_next;
    fn_pinyin_end_get_bigram_phrases         end_get_bigram;
};

/* ── Symbol resolution ────────────────────────────────────────────────── */

#ifndef _WIN32
#define RESOLVE(handle, table, field, name) \
    ((table).field = (typeof((table).field))dc_resolve(handle, name, &missing))

/* ── Caller-owned string release ──────────────────────────────────────── */
/* pinyin.h / T0 freeze: strings returned by pinyin_get_sentence and the
 * *_auxiliary_text getters are caller-owned and must be released with
 * g_free().  GLib is not linked here, so resolve g_free from libglib and
 * fall back to free() (correct on Linux, where GLib uses the system
 * allocator). */

typedef void (*fn_g_free)(void *);
typedef void (*fn_g_strfreev)(gchar **);

static fn_g_free g_free_fn;
static fn_g_strfreev g_strfreev_fn;

static void resolve_g_free(void) {
    g_free_fn = (fn_g_free)free;
    g_strfreev_fn = NULL;
    void *glib = dlopen("libglib-2.0.so.0", RTLD_NOW);
    if (glib) {
        fn_g_free sym = (fn_g_free)dlsym(glib, "g_free");
        if (sym)
            g_free_fn = sym;
        g_strfreev_fn = (fn_g_strfreev)dlsym(glib, "g_strfreev");
    }
}

/* ── Performance-baseline inputs (the 20-input keystroke-cycle corpus;
 * crates/oxpinyin-engine/src/session/tests.rs::CYCLE_INPUTS holds the same) ─── */

#endif

static const char *PERF_CYCLE_INPUTS[] = {
    "ni",
    "wo",
    "de",
    "nihao",
    "zhongguo",
    "xian",
    "fangan",
    "xi'an",
    "bu'tian",
    "fan'gan",
    "n",
    "zh",
    "chongke",
    "caisho",
    "paolen",
    "waimenggu",
    "lenglan",
    "naoxion",
    "liangniejue",
    "chuaipengdengzaimiu",
};
static const size_t N_PERF_CYCLE_INPUTS =
    sizeof(PERF_CYCLE_INPUTS) / sizeof(PERF_CYCLE_INPUTS[0]);

/* The only observable effect of the perf loops. Function-pointer calls
 * cannot be folded by the compiler anyway, but this keeps the checksum live
 * and makes the intent explicit. */
static volatile uint64_t perf_sink;

#ifndef _WIN32
static uint64_t now_ns(void) {
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0)
        return 0;
    return (uint64_t)ts.tv_sec * 1000000000ULL + (uint64_t)ts.tv_nsec;
}

/* Process CPU time, read beside each cycle's wall-clock reads. */
static uint64_t cpu_ns(void) {
    struct timespec ts;
    if (clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &ts) != 0)
        return 0;
    return (uint64_t)ts.tv_sec * 1000000000ULL + (uint64_t)ts.tv_nsec;
}

struct perf_memory {
    long rss_kib;
    long hwm_kib;
    long vm_size_kib;
    long rss_anon_kib;
    long rss_file_kib;
};

static void read_perf_memory(struct perf_memory *memory) {
    FILE *status = fopen("/proc/self/status", "r");
    char line[256];

    memory->rss_kib = -1;
    memory->hwm_kib = -1;
    memory->vm_size_kib = -1;
    memory->rss_anon_kib = -1;
    memory->rss_file_kib = -1;
    if (!status)
        return;

    while (fgets(line, sizeof line, status)) {
        unsigned long value = 0;
        if (sscanf(line, "VmRSS: %lu kB", &value) == 1)
            memory->rss_kib = (long)value;
        else if (sscanf(line, "VmHWM: %lu kB", &value) == 1)
            memory->hwm_kib = (long)value;
        else if (sscanf(line, "VmSize: %lu kB", &value) == 1)
            memory->vm_size_kib = (long)value;
        else if (sscanf(line, "RssAnon: %lu kB", &value) == 1)
            memory->rss_anon_kib = (long)value;
        else if (sscanf(line, "RssFile: %lu kB", &value) == 1)
            memory->rss_file_kib = (long)value;
    }
    fclose(status);
}

static void perf_json_string(const char *text) {
    putchar('"');
    for (const unsigned char *p = (const unsigned char *)text; *p; p++) {
        switch (*p) {
        case '\\': fputs("\\\\", stdout); break;
        case '"':  fputs("\\\"", stdout); break;
        case '\n': fputs("\\n", stdout); break;
        case '\r': fputs("\\r", stdout); break;
        case '\t': fputs("\\t", stdout); break;
        default:
            if (*p < 0x20) {
                printf("\\u%04x", (unsigned)*p);
            } else {
                putchar(*p);
            }
        }
    }
    putchar('"');
}

static void perf_print_memory_body(const struct perf_memory *m) {
    printf("{\"rss_kib\":%ld,\"hwm_kib\":%ld,\"vm_size_kib\":%ld,"
           "\"rss_anon_kib\":%ld,\"rss_file_kib\":%ld}",
           m->rss_kib, m->hwm_kib, m->vm_size_kib,
           m->rss_anon_kib, m->rss_file_kib);
}

static void perf_print_memory(const char *tag, const struct perf_memory *m) {
    printf("\"%s\":", tag);
    perf_print_memory_body(m);
}

/* ── RSS diagnosis instrument (PERF_MODE=rss-diag) ────────────────────
 *
 * Answers, for one process of either engine, where the resident set
 * actually sits: how much of it is anonymous versus file-backed
 * (smaps_rollup), which mappings carry it (/proc/self/maps), how much of
 * it the allocator is merely retaining (malloc_trim(3) plus mallinfo2(3)),
 * and how the split moves between "just initialized" and "after N
 * keystroke cycles".
 *
 * Every reading is taken identically for libpinyin and for oxpinyin from
 * inside the same driver, so the comparison is symmetric by construction.
 * The /proc text dumps are gated on RSS_DIAG_DIR precisely because
 * writing them allocates: a measurement round that reports the trim delta
 * must leave RSS_DIAG_DIR unset, and a round that captures mappings is a
 * separate round. Nothing in this block runs unless PERF_MODE=rss-diag.
 */

struct smaps_rollup {
    long rss_kib;
    long pss_kib;
    long shared_clean_kib;
    long shared_dirty_kib;
    long private_clean_kib;
    long private_dirty_kib;
    long anonymous_kib;
    long file_kib; /* derived: rss - anonymous, per smaps semantics */
};

static void smaps_rollup_reset(struct smaps_rollup *r) {
    r->rss_kib = -1;
    r->pss_kib = -1;
    r->shared_clean_kib = -1;
    r->shared_dirty_kib = -1;
    r->private_clean_kib = -1;
    r->private_dirty_kib = -1;
    r->anonymous_kib = -1;
    r->file_kib = -1;
}

static void read_smaps_rollup(struct smaps_rollup *r) {
    FILE *f = fopen("/proc/self/smaps_rollup", "r");
    char line[256];

    smaps_rollup_reset(r);
    if (!f)
        return;

    while (fgets(line, sizeof line, f)) {
        unsigned long value = 0;
        if (sscanf(line, "Rss: %lu kB", &value) == 1)
            r->rss_kib = (long)value;
        else if (sscanf(line, "Pss: %lu kB", &value) == 1)
            r->pss_kib = (long)value;
        else if (sscanf(line, "Shared_Clean: %lu kB", &value) == 1)
            r->shared_clean_kib = (long)value;
        else if (sscanf(line, "Shared_Dirty: %lu kB", &value) == 1)
            r->shared_dirty_kib = (long)value;
        else if (sscanf(line, "Private_Clean: %lu kB", &value) == 1)
            r->private_clean_kib = (long)value;
        else if (sscanf(line, "Private_Dirty: %lu kB", &value) == 1)
            r->private_dirty_kib = (long)value;
        else if (sscanf(line, "Anonymous: %lu kB", &value) == 1)
            r->anonymous_kib = (long)value;
    }
    fclose(f);
    if (r->rss_kib >= 0 && r->anonymous_kib >= 0)
        r->file_kib = r->rss_kib - r->anonymous_kib;
}

static void print_smaps_rollup(const char *tag, const struct smaps_rollup *r) {
    printf("\"%s\":{\"rss_kib\":%ld,\"pss_kib\":%ld,"
           "\"shared_clean_kib\":%ld,\"shared_dirty_kib\":%ld,"
           "\"private_clean_kib\":%ld,\"private_dirty_kib\":%ld,"
           "\"anonymous_kib\":%ld,\"file_kib\":%ld}",
           tag, r->rss_kib, r->pss_kib, r->shared_clean_kib,
           r->shared_dirty_kib, r->private_clean_kib, r->private_dirty_kib,
           r->anonymous_kib, r->file_kib);
}

struct arena_state {
    long arena;      /* bytes obtained from sbrk (non-mmap heap)      */
    long hblks;      /* count of mmap'd regions                        */
    long hblkhd;     /* bytes in mmap'd regions                        */
    long uordblks;   /* bytes in in-use chunks                         */
    long fordblks;   /* bytes in free chunks retained by the allocator */
    long keepcost;   /* releasable top-of-heap bytes                   */
};

static void read_arena_state(struct arena_state *a) {
#if defined(__GLIBC__) && (__GLIBC__ > 2 || (__GLIBC__ == 2 && __GLIBC_MINOR__ >= 33))
    struct mallinfo2 mi = mallinfo2();

    a->arena = (long)mi.arena;
    a->hblks = (long)mi.hblks;
    a->hblkhd = (long)mi.hblkhd;
    a->uordblks = (long)mi.uordblks;
    a->fordblks = (long)mi.fordblks;
    a->keepcost = (long)mi.keepcost;
#else
    /* Not available on this libc; print_arena_state() emits null. */
    a->arena = a->hblks = a->hblkhd = -1;
    a->uordblks = a->fordblks = a->keepcost = -1;
#endif
}

static void print_arena_state(const char *tag, const struct arena_state *a) {
#if defined(__GLIBC__) && (__GLIBC__ > 2 || (__GLIBC__ == 2 && __GLIBC_MINOR__ >= 33))
    printf("\"%s\":{\"arena\":%ld,\"hblks\":%ld,\"hblkhd\":%ld,"
           "\"uordblks\":%ld,\"fordblks\":%ld,\"keepcost\":%ld}",
           tag, a->arena, a->hblks, a->hblkhd,
           a->uordblks, a->fordblks, a->keepcost);
#else
    (void)a;
    printf("\"%s\":null", tag);
#endif
}

/* Copy a /proc text file verbatim. Returns 0 on success. The destination
 * is <dir>/<tag>-<what>.txt; the caller owns naming. */
static int dump_proc_text(const char *dir, const char *tag, const char *what,
                          const char *src) {
    char path[512];
    FILE *in, *out;
    char buffer[4096];
    size_t n;

    if (snprintf(path, sizeof path, "%s/%s-%s.txt", dir, tag, what)
        >= (int)sizeof path)
        return -1;
    in = fopen(src, "r");
    if (!in)
        return -1;
    out = fopen(path, "w");
    if (!out) {
        fclose(in);
        return -1;
    }
    while ((n = fread(buffer, 1, sizeof buffer, in)) > 0) {
        if (fwrite(buffer, 1, n, out) != n) {
            fclose(in);
            fclose(out);
            return -1;
        }
    }
    fclose(in);
    return fclose(out) == 0 ? 0 : -1;
}

/* glibc's own arena XML, which mallinfo2 cannot express: per-arena free
 * totals, the mmap threshold, and the system-bytes high-water. */
static int dump_malloc_info(const char *dir, const char *tag) {
#if defined(__GLIBC__)
    char path[512];
    FILE *out;

    if (snprintf(path, sizeof path, "%s/%s-mallocinfo.xml", dir, tag)
        >= (int)sizeof path)
        return -1;
    out = fopen(path, "w");
    if (!out)
        return -1;
    if (malloc_info(0, out) != 0) {
        fclose(out);
        return -1;
    }
    return fclose(out) == 0 ? 0 : -1;
#else
    (void)dir;
    (void)tag;
    return -1;
#endif
}

#endif

/* One 20-input cycle. For every input the instance is reset, then each
 * accumulated ASCII prefix is parsed and decoded the way the C++ frontend
 * drives the keystroke path: parse, guess candidates, read the count. */
#if defined(MEASURE_LINKED)
static bool measure_failed;
static uint64_t measure_operations;
#endif

static uint64_t run_perf_cycle(const struct symbols *s, pinyin_instance_t *inst) {
    uint64_t checksum = 1469598103934665603ULL;

    for (size_t i = 0; i < N_PERF_CYCLE_INPUTS; i++) {
        const char *input = PERF_CYCLE_INPUTS[i];
        size_t length = strlen(input);
        char prefix[256];

        if (length + 1 > sizeof prefix)
            return checksum;
        if (!s->reset(inst)) {
#if defined(MEASURE_LINKED)
            measure_failed = true;
#endif
            return checksum;
        }

        for (size_t j = 0; j < length; j++) {
            prefix[j] = input[j];
            prefix[j + 1] = '\0';

            size_t consumed = s->parse_full(inst, prefix);
            bool guessed = s->guess_candidates(inst, 0, DEFAULT_SORT);
            guint candidate_count = 0;
            bool counted = s->get_n_candidate(inst, &candidate_count);
#if defined(MEASURE_LINKED)
            measure_operations++;
            if (!consumed || !guessed || !counted) measure_failed = true;
#endif

            checksum ^= (uint64_t)consumed;
            checksum ^= guessed ? 0x9e3779b97f4a7c15ULL : 0xbf58476d1ce4e5b9ULL;
            checksum ^= counted ? (uint64_t)candidate_count : 0x94d049bb133111ebULL;
            checksum = (checksum << 7) | (checksum >> 57);
        }
    }
    return checksum;
}

#ifndef _WIN32
/* One timed unit: `repeats` back-to-back corpus passes over the frozen
 * 20-input cycle above. PERF_REPEATS scales the amount of work inside the
 * timed region without touching the corpus, so input structure — every
 * string, every prefix length, every reset — is identical at every size.
 * At the default of 1 this returns run_perf_cycle's own checksum
 * unchanged, so the default workload is the pre-knob workload exactly. */
static uint64_t run_perf_unit(const struct symbols *s, pinyin_instance_t *inst,
                              int repeats) {
    uint64_t checksum = 0;

    for (int r = 0; r < repeats; r++)
        checksum ^= run_perf_cycle(s, inst);
    return checksum;
}

/* Callgrind collection anchors.
 *
 * `run_perf_unit` is static and inlinable, and it runs for every cycle
 * index including 0 — the cold one. Neither property suits
 * `--toggle-collect`, which needs a symbol that survives -O2 and that
 * brackets only the steady cycles. These two `noinline`, externally
 * visible forwarders give that: cycle 0 goes through the cold anchor,
 * cycles 1..n-1 through the steady one, so
 *
 *   valgrind --tool=callgrind --collect-atstart=no \
 *            --toggle-collect=bisect_perf_steady_unit
 *
 * counts exactly the steady keystroke cycles. Process start, dlopen,
 * `pinyin_init`, `pinyin_alloc_instance` and the cold pass all stay
 * outside the count.
 *
 * They live in the harness, not in either library, so the toggled region
 * is identical for every `.so` the harness is pointed at — which is the
 * whole reason a single harness driving both engines through one C ABI
 * gives directly comparable totals.
 *
 * Cost when not under valgrind: one non-inlined call per cycle, against
 * 123 keystroke steps per corpus pass. Identical in both cells. */
uint64_t bisect_perf_cold_unit(const struct symbols *s, pinyin_instance_t *inst,
                               int repeats);
uint64_t bisect_perf_steady_unit(const struct symbols *s, pinyin_instance_t *inst,
                                 int repeats);

__attribute__((noinline)) uint64_t
bisect_perf_cold_unit(const struct symbols *s, pinyin_instance_t *inst,
                      int repeats) {
    return run_perf_unit(s, inst, repeats);
}

__attribute__((noinline)) uint64_t
bisect_perf_steady_unit(const struct symbols *s, pinyin_instance_t *inst,
                        int repeats) {
    return run_perf_unit(s, inst, repeats);
}

static int perf_env_count(const char *name, int fallback) {
    const char *value = getenv(name);
    char *end = NULL;
    long parsed;

    if (!value || !*value)
        return fallback;
    parsed = strtol(value, &end, 10);
    if (!end || *end || parsed < 1 || parsed > 100000)
        return fallback;
    return (int)parsed;
}

/* Hardware instruction counter for the malloc-interception distortion
 * measurement (just measure-callgrind-differential stage 5). Callgrind links
 * vg_replace_malloc into every tool, so its Ir counts valgrind's
 * simplified allocator where a native run pays glibc's; a hardware
 * instructions counter over the same bracketed steady region measures
 * the real thing, and the two engine ratios' difference is the measured
 * distortion.
 *
 * Opened only when PERF_HW=instructions is set: without it no counter
 * exists and the timed paths are untouched. exclude_kernel=1 keeps the
 * counter usable under perf_event_paranoid=2; a container whose SELinux
 * label or seccomp profile denies perf_event_open gets a diagnostic and
 * an hw_errno field, never a failed run. */
static long perf_open_instruction_counter(void) {
#if defined(__linux__)
    struct perf_event_attr attr;

    memset(&attr, 0, sizeof attr);
    attr.size = (uint32_t)sizeof attr;
    attr.type = PERF_TYPE_HARDWARE;
    attr.config = PERF_COUNT_HW_INSTRUCTIONS;
    attr.disabled = 1;
    attr.exclude_kernel = 1;
    attr.exclude_hv = 1;
    return syscall(SYS_perf_event_open, &attr, 0, -1, -1, 0);
#else
    errno = ENOSYS;
    return -1;
#endif
}

/* zhuyin.h:343-344 at 074a2219: the after-cursor entry takes no sort
 * argument. Adapt it to the existing cycle instead of calling through an
 * incompatible function-pointer type. Only the harness owns this adapter. */
static struct {
    bool (*guess_after_cursor)(pinyin_instance_t *, size_t);
} perf_zhuyin;

static bool perf_zhuyin_guess(pinyin_instance_t *instance, size_t offset,
                             guint sort) {
    (void)sort;
    return perf_zhuyin.guess_after_cursor(instance, offset);
}

static int resolve_perf_symbols(void *handle, struct symbols *s, bool zhuyin) {
    int missing = 0;

    memset(s, 0, sizeof *s);
#define PERF_RESOLVE(field, suffix) do { \
    char name[96]; \
    snprintf(name, sizeof name, "%s_%s", zhuyin ? "zhuyin" : "pinyin", suffix); \
    RESOLVE(handle, *s, field, name); \
} while (0)
    PERF_RESOLVE(init, "init");
    PERF_RESOLVE(fini, "fini");
    PERF_RESOLVE(alloc_instance, "alloc_instance");
    PERF_RESOLVE(free_instance, "free_instance");
    PERF_RESOLVE(set_options, "set_options");
    PERF_RESOLVE(parse_full, "parse_more_full_pinyins");
    if (zhuyin) {
        RESOLVE(handle, perf_zhuyin, guess_after_cursor,
                "zhuyin_guess_candidates_after_cursor");
        s->guess_candidates = perf_zhuyin_guess;
    } else {
        PERF_RESOLVE(guess_candidates, "guess_candidates");
    }
    PERF_RESOLVE(get_n_candidate, "get_n_candidate");
    PERF_RESOLVE(reset, "reset");
#undef PERF_RESOLVE
    return missing;
}

static int perf_remove_entry(const char *path, const struct stat *st,
                            int type, struct FTW *ftw) {
    (void)st;
    (void)type;
    (void)ftw;
    return remove(path);
}

static void perf_remove_user_dir(char *user_dir) {
    const char *supplied = getenv("PERF_USER_DIR");
    if (supplied && *supplied && !strcmp(supplied, user_dir)) return;
    /* Best-effort cleanup of our own mkdtemp directory. */
    (void)nftw(user_dir, perf_remove_entry, 16, FTW_DEPTH | FTW_PHYS);
}

static bool user_dir_template(char *path, size_t capacity, const char *prefix) {
    const char *temp_root = getenv("TMPDIR");
    if (temp_root == NULL || *temp_root == '\0')
        temp_root = "/tmp";
    int length = snprintf(path, capacity, "%s/%s-XXXXXX", temp_root, prefix);
    if (length < 0 || (size_t)length >= capacity) {
        fprintf(stderr, "fatal: TMPDIR path is too long\n");
        return false;
    }
    return true;
}

static int run_perf_mode(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr,
                "Usage: %s --perf <path-to-so> <systemdir>\n"
                "  Environment: PERF_BACKEND (label), PERF_FACADE\n"
                "    (pinyin|zhuyin, default pinyin), PERF_MODE\n"
                "    (speed|ram-init|ram-cycle|rss-diag), PERF_CYCLES\n"
                "    (default 8), PERF_REPEATS (corpus passes per cycle,\n"
                "    default 1), PERF_WORKLOAD_CHECKSUM (emit the cycle\n"
                "    return/count fingerprint when non-empty). rss-diag\n"
                "    additionally reports\n"
                "    smaps_rollup and mallinfo2 at init and after the\n"
                "    cycles, calls malloc_trim(0) and re-reads both;\n"
                "    RSS_DIAG_DIR (with RSS_DIAG_TAG) also writes the\n"
                "    /proc/self/maps, /proc/self/smaps and malloc_info\n"
                "    dumps, which\n"
                "    allocate and so belong in their own round.\n",
                argv[0]);
        return 1;
    }

    const char *so_path    = argv[2];
    const char *system_dir = argv[3];

    const char *backend    = getenv("PERF_BACKEND");
    const char *mode       = getenv("PERF_MODE");
    const char *facade     = getenv("PERF_FACADE");
    const char *checksum_env = getenv("PERF_WORKLOAD_CHECKSUM");
    int cycles             = perf_env_count("PERF_CYCLES", 8);
    int repeats            = perf_env_count("PERF_REPEATS", 1);

    if (!backend)
        backend = so_path;
    if (!mode)
        mode = "speed";
    if (!facade)
        facade = "pinyin";
    if (strcmp(facade, "pinyin") != 0 && strcmp(facade, "zhuyin") != 0) {
        fprintf(stderr, "fatal: PERF_FACADE must be pinyin or zhuyin\n");
        return 1;
    }

    /* RSS diagnosis. The /proc text dumps are opt-in on RSS_DIAG_DIR
     * because writing them allocates: a round that reports the
     * malloc_trim delta must run with RSS_DIAG_DIR unset. */
    const bool rss_diag = strcmp(mode, "rss-diag") == 0;
    const char *diag_dir = getenv("RSS_DIAG_DIR");
    const char *diag_tag = getenv("RSS_DIAG_TAG");

    if (diag_tag == NULL || *diag_tag == '\0')
        diag_tag = "run";
    if (!rss_diag)
        diag_dir = NULL;

    char user_dir[4096];
    const char *supplied_user = getenv("PERF_USER_DIR");
    if (supplied_user && *supplied_user) {
        struct stat info;
        if (snprintf(user_dir, sizeof user_dir, "%s", supplied_user) >= (int)sizeof user_dir ||
            stat(user_dir, &info) || !S_ISDIR(info.st_mode)) {
            fputs("PERF_USER_DIR must name an existing private profile directory\n", stderr);
            return 1;
        }
    } else {
        if (!user_dir_template(user_dir, sizeof user_dir, "bisect-perf")) return 1;
        if (!mkdtemp(user_dir)) {
            perror("mkdtemp");
            return 1;
        }
    }

    void *handle = dlopen(so_path, RTLD_NOW);
    if (!handle) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        perf_remove_user_dir(user_dir);
        return 1;
    }

    struct symbols sym;
    int missing = resolve_perf_symbols(handle, &sym, strcmp(facade, "zhuyin") == 0);
    if (missing > 0) {
        fprintf(stderr, "fatal: %d perf symbols missing\n", missing);
        dlclose(handle);
        perf_remove_user_dir(user_dir);
        return 1;
    }

    uint64_t init_start = now_ns();
    pinyin_context_t *ctx = sym.init(system_dir, user_dir);
    uint64_t init_end = now_ns();
    if (!ctx) {
        fprintf(stderr, "fatal: pinyin_init returned NULL\n");
        dlclose(handle);
        perf_remove_user_dir(user_dir);
        return 1;
    }

    (void)sym.set_options(ctx, DEFAULT_FLAGS);
    uint64_t alloc_start = now_ns();
    pinyin_instance_t *inst = sym.alloc_instance(ctx);
    uint64_t alloc_end = now_ns();
    if (!inst) {
        fprintf(stderr, "fatal: alloc_instance returned NULL\n");
        sym.fini(ctx);
        dlclose(handle);
        perf_remove_user_dir(user_dir);
        return 1;
    }

    struct perf_memory after_init;
    struct smaps_rollup rollup_init;
    struct arena_state arena_init;

    read_perf_memory(&after_init);
    if (rss_diag) {
        read_smaps_rollup(&rollup_init);
        read_arena_state(&arena_init);
        if (diag_dir) {
            (void)dump_proc_text(diag_dir, diag_tag, "maps-init",
                                 "/proc/self/maps");
            (void)dump_proc_text(diag_dir, diag_tag, "smaps-init",
                                 "/proc/self/smaps");
            (void)dump_proc_text(diag_dir, diag_tag, "smaps-rollup-init",
                                 "/proc/self/smaps_rollup");
            (void)dump_malloc_info(diag_dir, diag_tag);
        }
    }

    printf("{\"pid\":%ld,\"backend\":", (long)getpid());
    perf_json_string(backend);
    printf(",\"so\":");
    perf_json_string(so_path);
    printf(",\"systemdir\":");
    perf_json_string(system_dir);
    printf(",\"mode\":");
    perf_json_string(mode);
    printf(",\"repeats\":%d", repeats);
    printf(",\"init_ns\":%llu,\"alloc_ns\":%llu,",
           (unsigned long long)(init_end - init_start),
           (unsigned long long)(alloc_end - alloc_start));
    perf_print_memory("after_init", &after_init);
    if (rss_diag) {
        putchar(',');
        print_smaps_rollup("rollup_init", &rollup_init);
        putchar(',');
        print_arena_state("arena_init", &arena_init);
    }

    if (strcmp(mode, "ram-init") != 0) {
        uint64_t *cycle_ns = calloc((size_t)cycles, sizeof *cycle_ns);
        uint64_t *cycle_cpu_ns = calloc((size_t)cycles, sizeof *cycle_cpu_ns);
        struct perf_memory after_first;
        struct perf_memory after_last;
        if (!cycle_ns || !cycle_cpu_ns) {
            free(cycle_ns);
            free(cycle_cpu_ns);
            fprintf(stderr, "fatal: calloc cycle_ns failed\n");
            sym.free_instance(inst);
            sym.fini(ctx);
            dlclose(handle);
            perf_remove_user_dir(user_dir);
            return 1;
        }

        /* Callgrind boundary for the hardware instruction counter: cycles
         * 1..n-1, enabled and disabled around the steady anchor call, so
         * the counted region is the one the callgrind toggle collects. */
        int hw_wanted = getenv("PERF_HW") != NULL;
        int hw_fd = -1;
        int hw_errno = 0;
        uint64_t hw_instructions = 0;
        int hw_cycles = 0;
        uint64_t workload_checksum = 1469598103934665603ULL;

        if (hw_wanted) {
            hw_fd = (int)perf_open_instruction_counter();
            if (hw_fd < 0) {
                hw_errno = errno;
                fprintf(stderr,
                        "perf_event_open: %s (errno %d) — this run "
                        "carries no hardware instruction counts\n",
                        strerror(hw_errno), hw_errno);
            }
        }

        for (int i = 0; i < cycles; i++) {
            uint64_t sink_before = perf_sink;
            if (i > 0 && hw_fd >= 0)
                (void)ioctl(hw_fd, PERF_EVENT_IOC_ENABLE, 0);
            /* The CPU reads sit outside the wall-clock reads, so wall time is
             * unchanged; with PERF_HW the counter also sees their user half. */
            uint64_t cpu_start = cpu_ns();
            uint64_t start = now_ns();
            /* Cycle 0 is the cold one and goes through its own anchor, so a
             * callgrind toggle on the steady anchor never sees it. */
            perf_sink ^= (i == 0)
                             ? bisect_perf_cold_unit(&sym, inst, repeats)
                             : bisect_perf_steady_unit(&sym, inst, repeats);
            uint64_t end = now_ns();
            uint64_t cpu_end = cpu_ns();
            /* Outside the timer: retain the existing cycle's return/count
             * fingerprint without XOR-cancelling an even number of cycles.
             * Full observable equivalence still belongs to contract-diff. */
            workload_checksum ^= sink_before ^ perf_sink;
            workload_checksum *= 1099511628211ULL;
            if (i > 0 && hw_fd >= 0) {
                uint64_t counted = 0;

                (void)ioctl(hw_fd, PERF_EVENT_IOC_DISABLE, 0);
                if (read(hw_fd, &counted, sizeof counted) == (ssize_t)sizeof counted) {
                    hw_instructions += counted;
                    hw_cycles++;
                }
                /* read() does not reset the counter and enable/disable do
                 * not zero it either: without this RESET every later read
                 * would return the running total again and the sum below
                 * would double-count earlier cycles. */
                (void)ioctl(hw_fd, PERF_EVENT_IOC_RESET, 0);
            }
            cycle_ns[i] = end - start;
            cycle_cpu_ns[i] = cpu_end - cpu_start;
            if (i == 0)
                read_perf_memory(&after_first);
        }
        read_perf_memory(&after_last);
        if (checksum_env && *checksum_env)
            printf(",\"workload_checksum\":\"%016llx\"",
                   (unsigned long long)workload_checksum);

        /* The decisive experiment. Read the post-cycle state, then ask
         * glibc to return everything it is merely retaining, then read
         * the same state again. A gap that collapses here was allocator
         * retention driven by transient churn; a gap that survives is
         * held in live structures or in file-backed pages. */
        struct smaps_rollup rollup_cycle;
        struct arena_state arena_cycle;
        struct perf_memory after_trim;
        struct smaps_rollup rollup_trim;
        struct arena_state arena_trim;
#if defined(__GLIBC__)
        int trim_released = -1;
#endif

        if (rss_diag) {
            read_smaps_rollup(&rollup_cycle);
            read_arena_state(&arena_cycle);
            if (diag_dir) {
                (void)dump_proc_text(diag_dir, diag_tag, "maps-cycle",
                                     "/proc/self/maps");
                (void)dump_proc_text(diag_dir, diag_tag, "smaps-cycle",
                                     "/proc/self/smaps");
                (void)dump_proc_text(diag_dir, diag_tag,
                                     "smaps-rollup-cycle",
                                     "/proc/self/smaps_rollup");
            }
#if defined(__GLIBC__)
            trim_released = malloc_trim(0);
#endif
            read_perf_memory(&after_trim);
            read_smaps_rollup(&rollup_trim);
            read_arena_state(&arena_trim);
            if (diag_dir) {
                (void)dump_proc_text(diag_dir, diag_tag, "maps-trim",
                                     "/proc/self/maps");
                (void)dump_proc_text(diag_dir, diag_tag, "smaps-trim",
                                     "/proc/self/smaps");
                (void)dump_proc_text(diag_dir, diag_tag,
                                     "smaps-rollup-trim",
                                     "/proc/self/smaps_rollup");
            }
        }

        printf(",\"cycles_ns\":[");
        for (int i = 0; i < cycles; i++)
            printf("%s%llu", i ? "," : "", (unsigned long long)cycle_ns[i]);
        printf("],\"cycles_cpu_ns\":[");
        for (int i = 0; i < cycles; i++)
            printf("%s%llu", i ? "," : "", (unsigned long long)cycle_cpu_ns[i]);
        printf("],\"after_first\":");
        perf_print_memory_body(&after_first);
        printf(",\"after_last\":");
        perf_print_memory_body(&after_last);
        if (rss_diag) {
            putchar(',');
            print_smaps_rollup("rollup_cycle", &rollup_cycle);
            putchar(',');
            print_arena_state("arena_cycle", &arena_cycle);
#if defined(__GLIBC__)
            printf(",\"trim_released\":%d", trim_released);
#else
            printf(",\"trim_released\":null");
#endif
            printf(",\"after_trim\":");
            perf_print_memory_body(&after_trim);
            putchar(',');
            print_smaps_rollup("rollup_trim", &rollup_trim);
            putchar(',');
            print_arena_state("arena_trim", &arena_trim);
        }
        if (hw_fd >= 0) {
            printf(",\"steady_hw_cycles\":%d"
                   ",\"steady_hw_instructions\":%llu",
                   hw_cycles, (unsigned long long)hw_instructions);
            close(hw_fd);
        } else if (hw_wanted) {
            printf(",\"hw_errno\":%d", hw_errno);
        }
        free(cycle_ns);
        free(cycle_cpu_ns);
    }

    printf("}\n");

    sym.free_instance(inst);
    sym.fini(ctx);
    dlclose(handle);
    perf_remove_user_dir(user_dir);
    return 0;
}

#endif

/* Native measurement uses normal linkage; the legacy pin drivers retain dlopen. */
#if defined(MEASURE_LINKED)
#ifdef _WIN32
#define MEASURE_IMPORT __declspec(dllimport)
#else
#define MEASURE_IMPORT extern
#endif
MEASURE_IMPORT pinyin_context_t *pinyin_init(const char *, const char *);
MEASURE_IMPORT void pinyin_fini(pinyin_context_t *);
MEASURE_IMPORT pinyin_instance_t *pinyin_alloc_instance(pinyin_context_t *);
MEASURE_IMPORT void pinyin_free_instance(pinyin_instance_t *);
MEASURE_IMPORT bool pinyin_set_options(pinyin_context_t *, pinyin_option_t);
MEASURE_IMPORT size_t pinyin_parse_more_full_pinyins(pinyin_instance_t *, const char *);
MEASURE_IMPORT bool pinyin_guess_candidates(pinyin_instance_t *, size_t, guint);
MEASURE_IMPORT bool pinyin_get_n_candidate(pinyin_instance_t *, guint *);
MEASURE_IMPORT bool pinyin_reset(pinyin_instance_t *);

#ifndef _WIN32
static bool measure_image(const char *path, const char *expected_uuid) {
    Dl_info info;
    char actual[4096], expected[4096], uuid[33];
    if (!dladdr((void *)pinyin_init, &info) || !info.dli_fname ||
        !realpath(info.dli_fname, actual) || !realpath(path, expected) ||
        strcmp(actual, expected)) return false;
    const struct mach_header_64 *header = info.dli_fbase;
    if (header->magic != MH_MAGIC_64) return false;
    const unsigned char *cursor = (const unsigned char *)(header + 1);
    const unsigned char *end = cursor + header->sizeofcmds;
    for (uint32_t i = 0; i < header->ncmds; i++) {
        if ((size_t)(end - cursor) < sizeof(struct load_command)) return false;
        const struct load_command *command = (const void *)cursor;
        if (command->cmdsize < sizeof *command || command->cmdsize > (size_t)(end - cursor)) return false;
        if (command->cmd == LC_UUID) {
            if (command->cmdsize != sizeof(struct uuid_command)) return false;
            const struct uuid_command *id = (const void *)cursor;
            for (int j = 0; j < 16; j++) snprintf(uuid + 2*j, 3, "%02x", id->uuid[j]);
            return !strcmp(uuid, expected_uuid);
        }
        cursor += command->cmdsize;
    }
    return false;
}

static double measure_cpu(const struct rusage *usage) {
    return (double)usage->ru_utime.tv_sec + (double)usage->ru_stime.tv_sec +
        ((double)usage->ru_utime.tv_usec + (double)usage->ru_stime.tv_usec) / 1000000.0;
}

static int run_measure(int argc, char **argv) {
    if (argc != 5 || !measure_image(argv[2], argv[3])) {
        fputs("measurement: wrong loaded image or UUID\n", stderr);
        return 1;
    }
    const char *setting = getenv("PERF_CYCLES");
    char *end = NULL;
    long cycles = setting ? strtol(setting, &end, 10) : 0;
    if (!setting || !*setting || *end || cycles < 1 || cycles > 100000) {
        fputs("measurement: invalid cycle count\n", stderr);
        return 1;
    }
    struct symbols sym = {.init=pinyin_init, .fini=pinyin_fini,
        .alloc_instance=pinyin_alloc_instance, .free_instance=pinyin_free_instance,
        .set_options=pinyin_set_options, .parse_full=pinyin_parse_more_full_pinyins,
        .guess_candidates=pinyin_guess_candidates, .get_n_candidate=pinyin_get_n_candidate,
        .reset=pinyin_reset};
    char user[4096];
    if (!user_dir_template(user, sizeof user, "bisect-measure") || !mkdtemp(user)) return 1;
    pinyin_context_t *ctx = sym.init(argv[4], user);
    if (!ctx) { perf_remove_user_dir(user); return 1; }
    pinyin_instance_t *instance = sym.alloc_instance(ctx);
    if (!instance || !sym.set_options(ctx, DEFAULT_FLAGS)) {
        if (instance) sym.free_instance(instance);
        sym.fini(ctx);
        perf_remove_user_dir(user);
        return 1;
    }
    uint64_t expected = 0, fingerprint = 1469598103934665603ULL;
    for (size_t i = 0; i < N_PERF_CYCLE_INPUTS; i++) expected += strlen(PERF_CYCLE_INPUTS[i]);
    expected *= (uint64_t)cycles;
    struct rusage before, after;
    /* The kernel's own per-process counters, read around the same loop. */
    struct rusage_info_v4 entered, left;
    mach_timebase_info_data_t timebase;
    bool captured = mach_timebase_info(&timebase) == KERN_SUCCESS && timebase.denom != 0;
    captured = proc_pid_rusage(getpid(), RUSAGE_INFO_V4, (rusage_info_t *)&entered) == 0 && captured;
    captured = getrusage(RUSAGE_SELF, &before) == 0 && captured;
    uint64_t start = now_ns();
    for (long i = 0; i < cycles; i++) {
        fingerprint ^= run_perf_cycle(&sym, instance);
        fingerprint *= 1099511628211ULL;
    }
    uint64_t stop = now_ns();
    captured = getrusage(RUSAGE_SELF, &after) == 0 && captured;
    captured = proc_pid_rusage(getpid(), RUSAGE_INFO_V4, (rusage_info_t *)&left) == 0 && captured;
    sym.free_instance(instance);
    sym.fini(ctx);
    struct rusage_info_v4 memory;
    captured = proc_pid_rusage(getpid(), RUSAGE_INFO_V4, (rusage_info_t *)&memory) == 0 && captured;
    perf_remove_user_dir(user);
    double cpu = captured ? measure_cpu(&after) - measure_cpu(&before) : 0;
    /* ri_runnable_time is in Mach absolute time units: on a core or waiting for one.
     * The kernel advances it at scheduling events only, so a short loop may read 0. */
    double runnable = captured ? (double)(left.ri_runnable_time - entered.ri_runnable_time)
                                 * timebase.numer / timebase.denom / 1e9 : 0;
    if (!captured || !start || stop <= start || cpu <= 0 ||
        left.ri_instructions <= entered.ri_instructions || left.ri_cycles <= entered.ri_cycles ||
        !memory.ri_lifetime_max_phys_footprint || measure_failed || measure_operations != expected) {
        fputs("measurement: incomplete workload or broken required capture\n", stderr);
        return 1;
    }
    printf("{\"pid\":%d,\"operations\":%llu,\"cycles\":%ld,"
           "\"fingerprint\":\"%016llx\",\"wall_seconds\":%.9f,"
           "\"cpu_seconds\":%.9f,\"loop_instructions\":%llu,\"loop_cpu_cycles\":%llu,"
           "\"loop_runnable_seconds\":%.9f,\"lifetime_max_phys_footprint_bytes\":%llu}\n",
           getpid(), (unsigned long long)measure_operations, cycles,
           (unsigned long long)fingerprint, (double)(stop-start)/1e9, cpu,
           (unsigned long long)(left.ri_instructions - entered.ri_instructions),
           (unsigned long long)(left.ri_cycles - entered.ri_cycles), runnable,
           (unsigned long long)memory.ri_lifetime_max_phys_footprint);
    return 0;
}
#endif

#ifdef _WIN32
/* Windows uses the same linked API table and typing loop. */
struct measure_modules { unsigned count; char path[128][2048]; };
static struct measure_modules modules_before, modules_after;
static bool measure_wide(const char *text, wchar_t *out, int capacity) {
    return MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, text, -1, out, capacity) != 0;
}
static bool measure_module_path(HMODULE module, char *out, size_t capacity) {
    wchar_t path[32768];
    DWORD n = GetModuleFileNameW(module, path, 32768);
    return n && n < 32768 && WideCharToMultiByte(CP_UTF8, 0, path, -1, out,
                                               (int)capacity, NULL, NULL) != 0;
}
static bool measure_image_windows(const char *expected, HMODULE *product) {
    wchar_t wanted[32768], actual[32768];
    DWORD n;
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                           GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                           (LPCWSTR)(uintptr_t)pinyin_init, product) ||
        !measure_wide(expected, wanted, 32768)) return false;
    n = GetModuleFileNameW(*product, actual, 32768);
    if (!n || n == 32768) return false;
    HANDLE a = CreateFileW(wanted, 0, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                          NULL, OPEN_EXISTING, 0, NULL);
    HANDLE b = CreateFileW(actual, 0, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                          NULL, OPEN_EXISTING, 0, NULL);
    BY_HANDLE_FILE_INFORMATION ai, bi;
    bool ok = a != INVALID_HANDLE_VALUE && b != INVALID_HANDLE_VALUE &&
        GetFileInformationByHandle(a, &ai) && GetFileInformationByHandle(b, &bi) &&
        ai.dwVolumeSerialNumber == bi.dwVolumeSerialNumber &&
        ai.nFileIndexHigh == bi.nFileIndexHigh && ai.nFileIndexLow == bi.nFileIndexLow;
    if (a != INVALID_HANDLE_VALUE) CloseHandle(a);
    if (b != INVALID_HANDLE_VALUE) CloseHandle(b);
    return ok;
}
/* The product's dependency set: every module outside the Windows system
 * directory, plus the C runtime wherever it loads from. Other system modules
 * follow the host, not the arm: the loader maps apphelp.dll only on the first
 * launch of a new executable path. */
static bool measure_dependency_module(HMODULE module) {
    wchar_t path[32768], system[MAX_PATH];
    DWORD n = GetModuleFileNameW(module, path, 32768);
    UINT s = GetSystemDirectoryW(system, MAX_PATH);
    if (!n || n >= 32768 || !s || s >= MAX_PATH) return true; /* Unread paths fail in the caller. */
    const wchar_t *name = wcsrchr(path, L'\\');
    name = name ? name + 1 : path;
    if (!_wcsnicmp(name, L"ucrtbase", 8) || !_wcsnicmp(name, L"vcruntime", 9) ||
        !_wcsnicmp(name, L"msvcp1", 6)) return true;
    return !(n > s && !_wcsnicmp(path, system, s) && path[s] == L'\\');
}
static bool measure_modules_read(struct measure_modules *snapshot, HMODULE product) {
    HMODULE modules[128]; DWORD bytes;
    if (!EnumProcessModules(GetCurrentProcess(), modules, sizeof modules, &bytes) ||
        bytes > sizeof modules) return false;
    snapshot->count = 0;
    for (unsigned i = 0; i < bytes / sizeof(HMODULE); ++i) {
        if (modules[i] == product || modules[i] == GetModuleHandleW(NULL)) continue;
        if (!measure_dependency_module(modules[i])) continue;
        if (!measure_module_path(modules[i], snapshot->path[snapshot->count], 2048)) return false;
        snapshot->count++;
    }
    return snapshot->count != 0;
}
static bool measure_dependencies(const struct measure_modules *snapshot) {
    /* Runner writes the verified preflight module files, one UTF-8 path per line. */
    const char *manifest = getenv("MEASURE_MODULES");
    if (!manifest) return false;
    wchar_t path[32768];
    if (!measure_wide(manifest, path, 32768)) return false;
    FILE *file = _wfopen(path, L"rb");
    if (!file) return false;
    char line[2048]; unsigned count = 0; bool ok = true;
    while (fgets(line, sizeof line, file)) {
        line[strcspn(line, "\r\n")] = 0;
        bool found = false;
        for (unsigned i = 0; i < snapshot->count; ++i)
            if (!_stricmp(line, snapshot->path[i])) found = true;
        if (!found) ok = false;
        count++;
    }
    ok = ok && !ferror(file) && count == snapshot->count;
    fclose(file);
    return ok;
}
static void measure_json_string(const char *text) {
    putchar('"');
    for (const unsigned char *p = (const unsigned char *)text; *p; ++p) {
        if (*p == '"' || *p == '\\') putchar('\\');
        if (*p < 32) printf("\\u%04x", *p); else putchar(*p);
    }
    putchar('"');
}
static void measure_modules_json(const struct measure_modules *snapshot) {
    putchar('[');
    for (unsigned i = 0; i < snapshot->count; ++i) {
        if (i) putchar(',');
        measure_json_string(snapshot->path[i]);
    }
    putchar(']');
}
static bool measure_glib(HMODULE product) {
    /* The IAT binds the product's allocator. Never load a second GLib or use free. */
    const unsigned char *base = (const unsigned char *)product;
    const IMAGE_DOS_HEADER *dos = (const void *)base;
    const IMAGE_NT_HEADERS *nt = (const void *)(base + dos->e_lfanew);
    DWORD rva = nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT].VirtualAddress;
    if (!rva) return false;
    const IMAGE_IMPORT_DESCRIPTOR *imports = (const void *)(base + rva);
    for (; imports->Name; ++imports) {
        const char *name = (const char *)(base + imports->Name);
        if (_strnicmp(name, "glib-2.0", 8)) continue;
        HMODULE glib = GetModuleHandleA(name);
        FARPROC release = glib ? GetProcAddress(glib, "g_free") : NULL;
        if (!release || !imports->OriginalFirstThunk) return false;
        const IMAGE_THUNK_DATA *names = (const void *)(base + imports->OriginalFirstThunk);
        const IMAGE_THUNK_DATA *bound = (const void *)(base + imports->FirstThunk);
        for (; names->u1.AddressOfData; ++names, ++bound) {
            if (IMAGE_SNAP_BY_ORDINAL(names->u1.Ordinal)) continue;
            const IMAGE_IMPORT_BY_NAME *entry = (const void *)(base + names->u1.AddressOfData);
            if (!strcmp((const char *)entry->Name, "g_free"))
                return (uintptr_t)release == (uintptr_t)bound->u1.Function;
        }
    }
    return false;
}
static bool measure_user_dir(wchar_t *wide, char *utf8) {
    const wchar_t *root = _wgetenv(L"TEMP");
    if (!root) return false;
    for (unsigned i = 0; i < 100; ++i) {
        if (_snwprintf_s(wide, 32768, _TRUNCATE, L"%s\\bisect-%lu-%llu-%u", root,
                        GetCurrentProcessId(), GetTickCount64(), i) < 0) return false;
        if (CreateDirectoryW(wide, NULL))
            return WideCharToMultiByte(CP_UTF8, 0, wide, -1, utf8, 32768, NULL, NULL) != 0;
        if (GetLastError() != ERROR_ALREADY_EXISTS) return false;
    }
    return false;
}
static void measure_remove_user(const wchar_t *directory) {
    wchar_t path[32768]; WIN32_FIND_DATAW entry;
    _snwprintf_s(path, 32768, _TRUNCATE, L"%s\\*", directory);
    HANDLE search = FindFirstFileW(path, &entry);
    if (search != INVALID_HANDLE_VALUE) {
        do {
            if (!(entry.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY)) {
                _snwprintf_s(path, 32768, _TRUNCATE, L"%s\\%s", directory, entry.cFileName);
                DeleteFileW(path);
            }
        } while (FindNextFileW(search, &entry));
        FindClose(search);
    }
    RemoveDirectoryW(directory);
}
static uint64_t measure_filetime(FILETIME value) {
    return ((uint64_t)value.dwHighDateTime << 32) | value.dwLowDateTime;
}
static bool measure_cpu_windows(uint64_t *value) {
    FILETIME creation, exit, kernel, user;
    if (!GetProcessTimes(GetCurrentProcess(), &creation, &exit, &kernel, &user)) return false;
    *value = measure_filetime(kernel) + measure_filetime(user);
    return true;
}
static int run_measure(int argc, char **argv) {
    HMODULE product;
    bool preflight = getenv("MEASURE_PREFLIGHT") != NULL;
    if (argc != 5 || !measure_image_windows(argv[2], &product)) {
        fputs("measurement: wrong loaded image\n", stderr); return 1;
    }
    if (!measure_glib(product) || !measure_modules_read(&modules_before, product)) {
        fputs("measurement: product GLib ownership or module capture failed\n", stderr); return 1;
    }
    char image[2048];
    if (!measure_module_path(product, image, sizeof image)) return 1;
    if (preflight) {
        fputs("{\"library\":", stdout); measure_json_string(image);
        fputs(",\"modules\":", stdout); measure_modules_json(&modules_before);
        puts("}"); return 0;
    }
    if (!measure_dependencies(&modules_before)) {
        fputs("measurement: dependency identity mismatch before loop\n", stderr); return 1;
    }
    char *end = NULL; const char *setting = getenv("PERF_CYCLES");
    long cycles = setting ? strtol(setting, &end, 10) : 0;
    if (!setting || !*setting || *end || cycles < 1 || cycles > 100000) return 1;
    struct symbols sym = {.init=pinyin_init, .fini=pinyin_fini,
        .alloc_instance=pinyin_alloc_instance, .free_instance=pinyin_free_instance,
        .set_options=pinyin_set_options, .parse_full=pinyin_parse_more_full_pinyins,
        .guess_candidates=pinyin_guess_candidates, .get_n_candidate=pinyin_get_n_candidate,
        .reset=pinyin_reset};
    wchar_t user_wide[32768]; char user[32768];
    if (!measure_user_dir(user_wide, user)) return 1;
    pinyin_context_t *ctx = sym.init(argv[4], user);
    pinyin_instance_t *instance = ctx ? sym.alloc_instance(ctx) : NULL;
    bool ok = instance && sym.set_options(ctx, DEFAULT_FLAGS);
    uint64_t expected = 0, fingerprint = 1469598103934665603ULL, cpu_start = 0, cpu_end = 0;
    /* Cycles charged to every thread of the process; a count, never converted to seconds. */
    ULONG64 cycles_start = 0, cycles_end = 0;
    for (size_t i = 0; i < N_PERF_CYCLE_INPUTS; ++i) expected += strlen(PERF_CYCLE_INPUTS[i]);
    expected *= (uint64_t)cycles;
    LARGE_INTEGER frequency, start, stop;
    ok = ok && QueryPerformanceFrequency(&frequency) && frequency.QuadPart > 0 &&
        QueryProcessCycleTime(GetCurrentProcess(), &cycles_start) &&
        measure_cpu_windows(&cpu_start) && QueryPerformanceCounter(&start);
    if (ok) {
        for (long i = 0; i < cycles; ++i) {
            fingerprint ^= run_perf_cycle(&sym, instance);
            fingerprint *= 1099511628211ULL;
        }
        ok = QueryPerformanceCounter(&stop) && measure_cpu_windows(&cpu_end) &&
            QueryProcessCycleTime(GetCurrentProcess(), &cycles_end);
    }
    if (instance) sym.free_instance(instance);
    if (ctx) sym.fini(ctx);
    PROCESS_MEMORY_COUNTERS memory;
    ok = ok && GetProcessMemoryInfo(GetCurrentProcess(), &memory, sizeof memory) &&
        measure_modules_read(&modules_after, product) && measure_dependencies(&modules_after);
    measure_remove_user(user_wide);
    if (!ok || stop.QuadPart <= start.QuadPart || cpu_end < cpu_start || cycles_end <= cycles_start ||
        !memory.PeakPagefileUsage || !memory.PeakWorkingSetSize || measure_failed || measure_operations != expected) {
        fputs("measurement: incomplete workload or broken required capture/dependency identity\n", stderr); return 1;
    }
    printf("{\"pid\":%lu,\"operations\":%llu,\"cycles\":%ld,\"fingerprint\":\"%016llx\","
           "\"wall_seconds\":%.9f,\"cpu_seconds\":%.9f,\"loop_cpu_cycles\":%llu,"
           "\"peak_private_commit_bytes\":%llu,"
           "\"peak_working_set_bytes\":%llu,\"library\":",
           GetCurrentProcessId(), (unsigned long long)measure_operations, cycles,
           (unsigned long long)fingerprint, (double)(stop.QuadPart-start.QuadPart)/(double)frequency.QuadPart,
           (double)(cpu_end-cpu_start)/1e7, (unsigned long long)(cycles_end-cycles_start),
           (unsigned long long)memory.PeakPagefileUsage,
           (unsigned long long)memory.PeakWorkingSetSize);
    measure_json_string(image); fputs(",\"modules\":", stdout);
    measure_modules_json(&modules_after); puts("}");
    return 0;
}
#endif

#endif /* MEASURE_LINKED */

/* ── Main ─────────────────────────────────────────────────────────────── */

#ifndef _WIN32
#include "teardown.h"
#include "sentence_perf.h"
#endif

#ifdef _WIN32
static int driver_main(int argc, char **argv) {
#else
int main(int argc, char **argv) {
#endif
#if defined(MEASURE_LINKED)
    if (argc > 1 && !strcmp(argv[1], "--measure")) return run_measure(argc, argv);
#endif
#ifndef _WIN32
    if (argc > 1 && (strcmp(argv[1], "--perf-sentence") == 0 ||
                     strcmp(argv[1], "--perf-typing-sequence") == 0))
        return run_sentence_perf_mode(argc, argv);
    if (argc > 1 && strcmp(argv[1], "--teardown") == 0)
        return run_teardown_mode(argc, argv);
    if (argc > 1 && strcmp(argv[1], "--perf") == 0)
        return run_perf_mode(argc, argv);

    fprintf(stderr,
            "Usage: %s --perf <path-to-so> <systemdir>\n"
            "       %s --perf-sentence|--perf-typing-sequence ...\n"
            "       %s --teardown pinyin|zhuyin SO DATA USER ...\n",
            argv[0], argv[0], argv[0]);
    return 1;
#else
    fputs("Windows supports --measure only\n", stderr);
    return 2;
#endif
}

#ifdef _WIN32
int wmain(int argc, wchar_t **wide_argv) {
    char **argv = calloc((size_t)argc + 1, sizeof *argv);
    if (!argv) return 1;
    int status = 1;
    for (int i = 0; i < argc; ++i) {
        int size = WideCharToMultiByte(CP_UTF8, 0, wide_argv[i], -1, NULL, 0, NULL, NULL);
        if (!size || !(argv[i] = malloc((size_t)size)) ||
            !WideCharToMultiByte(CP_UTF8, 0, wide_argv[i], -1, argv[i], size, NULL, NULL)) goto done;
    }
    status = driver_main(argc, argv);
done:
    for (int i = 0; i < argc; ++i) free(argv[i]);
    free(argv);
    return status;
}
#endif
