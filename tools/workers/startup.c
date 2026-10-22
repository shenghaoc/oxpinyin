/*
 * startup.c — native startup worker (tooling layer 5 §7.2, §7.3, §7.7, §7.8).
 *
 * Runs #842's units U1–U6 against one libpinyin or libzhuyin shared object
 * using only libc and the loader. The Python controller owns profiles, memory
 * reads, digests, statistics and reports; none of it runs in this process.
 *
 * Usage:
 *   startup MODE FACADE SO DATA USER BIND ARG
 *     MODE    verify | measure | stop | probe | persist
 *     FACADE  pinyin | zhuyin
 *     BIND    now | lazy
 *     ARG     verify:  witness file, or - for none
 *             measure: comma-separated handshake points, or - for none
 *             stop:    the stop point
 *             probe:   hostile input index, 0-11
 *             persist: -
 *   Stop and handshake points, in order: start, lib_load, init, config,
 *   alloc, first_candidate, end.
 *
 * Environment:
 *   OXPINYIN_STARTUP_EXPORTS   the facade's complete export list,
 *                              comma-separated; U1 resolves every name.
 *   OXPINYIN_STARTUP_HANDSHAKE "WRITE_FD,READ_FD", required when measure
 *                              names handshake points. At each point the
 *                              worker writes "point NAME\n" and blocks until
 *                              it reads "\n", so the parent can read this
 *                              process's memory (§7.8 item 2).
 *
 * Output: one JSON record per line. The "units" record is the canonical
 * U1–U6 record; the controller hashes it (§7.8 item 3), so it carries no
 * time, address or path. Every record is buffered and written only when the
 * run succeeds. process_wall is the controller's: it spans the process,
 * which this worker cannot observe from inside.
 *
 * Exit: 0 success; 1 refusal (missing library, a file that is not a
 * loadable shared library, a missing symbol, a missing input) or a failed
 * unit; 2 bad invocation. On a non-zero exit stdout is empty.
 *
 * Build:
 *   cc -std=gnu11 -Wall -Wextra -Werror -O2 -g -o startup startup.c -ldl
 *   (no -ldl on macOS)
 */
#if defined(__APPLE__)
#define _DARWIN_C_SOURCE
#endif
#define _DEFAULT_SOURCE
#define _XOPEN_SOURCE 700
#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <errno.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <time.h>
#include <unistd.h>
#if !defined(CLOCK_MONOTONIC_RAW)
#define CLOCK_MONOTONIC_RAW CLOCK_MONOTONIC
#endif

/* §7.2: option word, sort word and first input. 0x18a is IS_PINYIN |
 * PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE; 0x1e is
 * SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY. */
#define OPTIONS ((uint32_t)0x18a)
#define SORT ((uint32_t)0x1e)
#define FIRST_INPUT "nihao"
#define NBEST_MATCH_CANDIDATE 1
#define FIRST_TEXT_BYTES 4096
#define MAX_EXPORTS 512
#define MAX_MISSING 512
#define OUTPUT_RESERVE (64 * 1024)

/* §7.3 input sets, in order. */
static const char *const SCRIPT_INPUTS[] = {
    "nihao", "ni", "n", "zh", "zhongguo", "xi'an", "bu'tian", "fan'gan",
    "fangan", "chongke", "caisho", "paolen", "waimenggu", "naoxion",
    "liangniejue", "chuaipengdengzaimiu", "a", "nihao"};
static const char *const CHEWING_INPUTS[] = {"su3cl3", "ji3", "5j/ "};
static const char *const HOSTILE_INPUTS[] = {
    "", "'", "''", "'ni", "ni'", "nihao'", "zzzz", "12345", "ni hao", "NIHAO",
    "\xe4\xbd\xa0\xe5\xa5\xbd",
    "nihaonihaonihaonihaonihaonihaonihaonihaonihaonihaonihaonihaonihaonihaonihao"};
#define COUNT(array) (sizeof (array) / sizeof (array)[0])

/* The save-and-reopen scenario (#842 child.rs persist_rows, PERSIST_INPUTS
 * at a204f9c0). The negative count is a row the library may refuse. */
static const struct { const char *phrase, *reading; int count; } PERSIST_ROWS[] = {
    {"\xe6\xb5\x8b\xe8\xaf\x95", "ce4'shi4", 5},
    {"\xe7\x88\xb1\xe6\xb2\xb3", "ai'he", 20000},
    {"\xe9\x94\x95", "a", 20000},
    {"\xe6\xb5\x8b\xe4\xbe\xa7", "ce4'ce4", -1}};
static const char *const PERSIST_INPUTS[] = {"ceshi", "aihe", "a", "cece", "nihao"};

enum point { START, LIB_LOAD, INIT, CONFIG, ALLOC, FIRST_CANDIDATE, END, POINTS };
static const char *const POINT_NAMES[POINTS] = {
    "start", "lib_load", "init", "config", "alloc", "first_candidate", "end"};

typedef void *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(void *);
typedef void *(*fn_alloc_instance)(void *);
typedef void (*fn_free_instance)(void *);
typedef bool (*fn_set_options)(void *, uint32_t);
typedef size_t (*fn_parse)(void *, const char *);
typedef size_t (*fn_parsed_length)(void *);
typedef bool (*fn_instance_bool)(void *);
typedef bool (*fn_context_bool)(void *);
typedef bool (*fn_guess_candidates)(void *, size_t, uint32_t);
typedef bool (*fn_guess_after)(void *, size_t);
typedef bool (*fn_get_n_candidate)(void *, unsigned *);
typedef bool (*fn_get_candidate)(void *, unsigned, void **);
typedef bool (*fn_get_candidate_type)(void *, void *, int *);
typedef bool (*fn_get_candidate_string)(void *, void *, const char **);
typedef bool (*fn_get_nbest)(void *, void *, uint8_t *);
typedef bool (*fn_get_sentence)(void *, uint8_t, char **);
typedef bool (*fn_zhuyin_sentence)(void *, char **);
typedef bool (*fn_load_library)(void *, uint8_t);
typedef void *(*fn_begin_add)(void *, uint8_t);
typedef bool (*fn_add_phrase)(void *, const char *, const char *, int);
typedef void (*fn_end_add)(void *);
typedef void (*fn_g_free)(void *);

struct api {
    bool zhuyin;
    fn_init init;
    fn_fini fini;
    fn_alloc_instance alloc_instance;
    fn_free_instance free_instance;
    fn_set_options set_options;
    fn_parse parse_full, parse_chewing;
    fn_parsed_length parsed_length;
    fn_instance_bool reset, guess_sentence;
    fn_context_bool save;
    fn_guess_candidates guess_candidates;
    fn_guess_after guess_after;
    fn_get_n_candidate get_n_candidate;
    fn_get_candidate get_candidate;
    fn_get_candidate_type get_candidate_type;
    fn_get_candidate_string get_candidate_string;
    fn_get_nbest get_nbest;
    fn_get_sentence get_sentence;
    fn_zhuyin_sentence zhuyin_sentence;
    fn_load_library load_library;
    fn_begin_add begin_add;
    fn_add_phrase add_phrase;
    fn_end_add end_add;
    fn_g_free free_memory;
};

struct clock_pair { uint64_t wall, cpu; };

/* Everything U1–U5 writes is preallocated, so no unit grows a buffer the
 * worker owns. */
struct units {
    enum point stopped_at;
    size_t exports;
    bool init, options, alloc;
    size_t consumed, parsed;
    bool guessed, counted, read;
    unsigned count;
    int type;
    char text[FIRST_TEXT_BYTES];
    struct clock_pair load_start, first_end;
    struct clock_pair lib_load, init_time, config, alloc_time, first, drop, fini;
    void *context, *instance;
};

static const char *facade;
static void *library;
static struct api api;
static char *export_names[MAX_EXPORTS];
static size_t export_count;
static const char *missing[MAX_MISSING];
static size_t missing_count;
static int handshake_write = -1, handshake_read = -1;
static bool handshake[POINTS];
static struct units units;

/* ── Failure and buffered output ──────────────────────────────────────── */

static _Noreturn void die(const char *format, ...) {
    va_list args;
    va_start(args, format);
    fputs("startup: ", stderr);
    vfprintf(stderr, format, args);
    fputc('\n', stderr);
    va_end(args);
    fflush(stderr);
    _exit(1);
}

static _Noreturn void usage(const char *reason) {
    fprintf(stderr, "startup: %s\n"
            "usage: startup verify|measure|stop|probe|persist pinyin|zhuyin "
            "SO DATA USER now|lazy ARG\n", reason);
    fflush(stderr);
    _exit(2);
}

static struct { char *data; size_t length, capacity; } out;

static void reserve(size_t extra) {
    if (out.capacity - out.length > extra) return;
    size_t capacity = out.capacity ? out.capacity : OUTPUT_RESERVE;
    while (capacity - out.length <= extra) {
        if (capacity > SIZE_MAX / 2) die("output too large");
        capacity *= 2;
    }
    char *data = realloc(out.data, capacity);
    if (!data) die("output allocation failed");
    out.data = data;
    out.capacity = capacity;
}

__attribute__((format(printf, 1, 2)))
static void emit(const char *format, ...) {
    va_list args, copy;
    va_start(args, format);
    va_copy(copy, args);
    int needed = vsnprintf(NULL, 0, format, copy);
    va_end(copy);
    if (needed < 0) die("output formatting failed");
    reserve((size_t)needed + 1);
    vsnprintf(out.data + out.length, out.capacity - out.length, format, args);
    va_end(args);
    out.length += (size_t)needed;
}

/* Bytes at or above 0x80 pass through: the libraries answer in UTF-8. */
static void emit_string(const char *text) {
    if (!text) { emit("null"); return; }
    emit("\"");
    for (const unsigned char *p = (const unsigned char *)text; *p; p++) {
        if (*p == '"' || *p == '\\') emit("\\%c", *p);
        else if (*p < 0x20) emit("\\u%04x", *p);
        else emit("%c", *p);
    }
    emit("\"");
}

static void emit_bool(const char *name, bool value) {
    emit(",\"%s\":%s", name, value ? "true" : "false");
}

static _Noreturn void finish(void) {
    for (size_t written = 0; written < out.length;) {
        ssize_t n = write(STDOUT_FILENO, out.data + written, out.length - written);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) die("stdout write failed");
        written += (size_t)n;
    }
    fflush(stderr);
    /* No library destructor runs after the measured units; #842 never
     * unloads the library either. */
    _exit(0);
}

/* ── Clock and handshake ──────────────────────────────────────────────── */

static struct clock_pair now(void) {
    struct timespec wall, cpu;
    if (clock_gettime(CLOCK_MONOTONIC_RAW, &wall) ||
        clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &cpu)) die("clock failed");
    return (struct clock_pair){
        (uint64_t)wall.tv_sec * 1000000000ULL + (uint64_t)wall.tv_nsec,
        (uint64_t)cpu.tv_sec * 1000000000ULL + (uint64_t)cpu.tv_nsec};
}

static struct clock_pair since(struct clock_pair start) {
    struct clock_pair end = now();
    return (struct clock_pair){end.wall - start.wall, end.cpu - start.cpu};
}

static struct clock_pair between(struct clock_pair start, struct clock_pair end) {
    return (struct clock_pair){end.wall - start.wall, end.cpu - start.cpu};
}

static void reach(enum point point) {
    if (!handshake[point]) return;
    char message[64];
    int length = snprintf(message, sizeof message, "point %s\n", POINT_NAMES[point]);
    for (int written = 0; written < length;) {
        ssize_t n = write(handshake_write, message + written, (size_t)(length - written));
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) die("handshake write failed at %s", POINT_NAMES[point]);
        written += (int)n;
    }
    char reply;
    ssize_t n;
    do n = read(handshake_read, &reply, 1); while (n < 0 && errno == EINTR);
    if (n != 1 || reply != '\n') die("handshake acknowledgement lost at %s", POINT_NAMES[point]);
}

/* ── Units ────────────────────────────────────────────────────────────── */

static void *lookup(const char *name) {
    dlerror();
    void *symbol = dlsym(library, name);
    if (dlerror() || !symbol) {
        if (missing_count < MAX_MISSING) missing[missing_count++] = name;
        return NULL;
    }
    return symbol;
}

/* The worker's api table, prepared before U1. A name in the export list
 * takes that lookup's result; any other name is looked up once on its own. */
static struct required { void **slot; char name[96]; long export_index; } required[48];
static size_t required_count;
static void *export_values[MAX_EXPORTS];

static void require(void **slot, const char *prefix, const char *suffix) {
    if (required_count == COUNT(required)) die("too many required symbols");
    struct required *entry = &required[required_count++];
    entry->slot = slot;
    if (snprintf(entry->name, sizeof entry->name, "%s%s", prefix, suffix) >= (int)sizeof entry->name)
        die("symbol name too long");
    entry->export_index = -1;
    for (size_t i = 0; i < export_count; i++)
        if (!strcmp(export_names[i], entry->name)) entry->export_index = (long)i;
}

static void prepare_required(void) {
    char prefix[16];
    snprintf(prefix, sizeof prefix, "%s_", facade);
#define REQUIRE(FIELD, SUFFIX) require((void **)&api.FIELD, prefix, SUFFIX)
    REQUIRE(init, "init");
    REQUIRE(fini, "fini");
    REQUIRE(alloc_instance, "alloc_instance");
    REQUIRE(free_instance, "free_instance");
    REQUIRE(set_options, "set_options");
    REQUIRE(parse_full, "parse_more_full_pinyins");
    REQUIRE(parse_chewing, "parse_more_chewings");
    REQUIRE(reset, "reset");
    REQUIRE(guess_sentence, "guess_sentence");
    REQUIRE(get_n_candidate, "get_n_candidate");
    REQUIRE(get_candidate, "get_candidate");
    REQUIRE(get_candidate_type, "get_candidate_type");
    REQUIRE(get_candidate_string, "get_candidate_string");
    REQUIRE(save, "save");
    REQUIRE(load_library, "load_phrase_library");
    REQUIRE(begin_add, "begin_add_phrases");
    REQUIRE(add_phrase, "iterator_add_phrase");
    REQUIRE(end_add, "end_add_phrases");
    if (api.zhuyin) {
        REQUIRE(guess_after, "guess_candidates_after_cursor");
        /* zhuyin.h:256-257 at 074a2219 has no nbest-index argument. */
        REQUIRE(zhuyin_sentence, "get_sentence");
    } else {
        REQUIRE(guess_candidates, "guess_candidates");
        REQUIRE(parsed_length, "get_parsed_input_length");
        REQUIRE(get_nbest, "get_candidate_nbest_index");
        REQUIRE(get_sentence, "get_sentence");
    }
#undef REQUIRE
    require((void **)&api.free_memory, "", "g_free");
}

/* Separately selectable Callgrind boundaries: each wraps exactly one unit.
 * U1 opens the library and resolves every name exactly once. */
__attribute__((noinline))
void startup_load_unit(const char *path, int flags) {
    library = dlopen(path, flags);
    if (!library) return;
    for (size_t i = 0; i < export_count; i++) export_values[i] = lookup(export_names[i]);
    for (size_t i = 0; i < required_count; i++) {
        struct required *entry = &required[i];
        *entry->slot = entry->export_index >= 0 ? export_values[entry->export_index] :
            lookup(entry->name);
    }
}

__attribute__((noinline))
void *startup_init_unit(const char *data, const char *user) {
    return api.init(data, user);
}

__attribute__((noinline))
bool startup_config_unit(void *context) {
    return api.set_options(context, OPTIONS);
}

__attribute__((noinline))
void *startup_alloc_unit(void *context) {
    return api.alloc_instance(context);
}

/* U5: parse through the copied-out first candidate string. */
__attribute__((noinline))
void startup_first_candidate_unit(void *instance) {
    units.consumed = api.parse_full(instance, FIRST_INPUT);
    units.parsed = api.zhuyin ? units.consumed : api.parsed_length(instance);
    units.guessed = api.zhuyin ? api.guess_after(instance, 0) :
        api.guess_candidates(instance, 0, SORT);
    units.counted = api.get_n_candidate(instance, &units.count);
    if (!units.counted || !units.count) return;
    void *candidate = NULL;
    const char *text = NULL;
    units.read = api.get_candidate(instance, 0, &candidate) && candidate &&
        api.get_candidate_type(instance, candidate, &units.type) &&
        api.get_candidate_string(instance, candidate, &text) && text &&
        strlen(text) < sizeof units.text;
    if (units.read) strcpy(units.text, text);
}

__attribute__((noinline))
void startup_drop_instance_unit(void *instance) {
    api.free_instance(instance);
    __asm__ volatile ("" ::: "memory");
}

__attribute__((noinline))
void startup_fini_unit(void *context) {
    api.fini(context);
    __asm__ volatile ("" ::: "memory");
}

static void emit_candidates(void *instance, unsigned count, unsigned *max_rank);

/* U1–U6 in order, stopping after `stop`. `after_units` reads the first
 * candidate state between U5 and U6, untimed; verify mode only. */
static void run_units(const char *path, int flags, const char *data, const char *user,
                      enum point stop, bool after_units) {
    units.stopped_at = START;
    reach(START);
    if (stop == START) return;

    units.load_start = now();
    startup_load_unit(path, flags);
    units.lib_load = since(units.load_start);
    if (!library) die("dlopen: %s", dlerror());
    if (missing_count) {
        for (size_t i = 0; i < missing_count; i++)
            fprintf(stderr, "startup: missing symbol %s\n", missing[i]);
        die("%zu required symbols are missing", missing_count);
    }
    units.exports = export_count;
    units.stopped_at = LIB_LOAD;
    reach(LIB_LOAD);
    if (stop == LIB_LOAD) return;

    struct clock_pair start = now();
    units.context = startup_init_unit(data, user);
    units.init_time = since(start);
    units.init = units.context != NULL;
    if (!units.init) die("init returned NULL");
    units.stopped_at = INIT;
    reach(INIT);
    if (stop == INIT) return;

    start = now();
    units.options = startup_config_unit(units.context);
    units.config = since(start);
    if (!units.options) die("set_options returned false");
    units.stopped_at = CONFIG;
    reach(CONFIG);
    if (stop == CONFIG) return;

    start = now();
    units.instance = startup_alloc_unit(units.context);
    units.alloc_time = since(start);
    units.alloc = units.instance != NULL;
    if (!units.alloc) die("alloc_instance returned NULL");
    units.stopped_at = ALLOC;
    reach(ALLOC);
    if (stop == ALLOC) return;

    start = now();
    startup_first_candidate_unit(units.instance);
    units.first_end = now();
    units.first = between(start, units.first_end);
    if (units.consumed != 5 || units.parsed != 5 || !units.guessed || !units.counted ||
        !units.count || !units.read || !units.text[0])
        die("first candidate contract failed");
    units.stopped_at = FIRST_CANDIDATE;
    reach(FIRST_CANDIDATE);
    if (stop == FIRST_CANDIDATE) return;

    if (after_units) {
        emit("{\"startup\":\"after_units\",\"counted\":%s,\"count\":%u,\"candidates\":",
             units.counted ? "true" : "false", units.count);
        unsigned max_rank = 0;
        emit_candidates(units.instance, units.count, &max_rank);
        emit("}\n");
    }

    start = now();
    startup_drop_instance_unit(units.instance);
    units.drop = since(start);
    start = now();
    startup_fini_unit(units.context);
    units.fini = since(start);
    units.instance = units.context = NULL;
    units.stopped_at = END;
    reach(END);
}

/* The canonical record: only answers, in unit order, up to the stop. */
static void emit_units(void) {
    emit("{\"startup\":\"units\",\"facade\":\"%s\",\"stopped_at\":\"%s\"",
         facade, POINT_NAMES[units.stopped_at]);
    if (units.stopped_at >= LIB_LOAD) emit(",\"exports\":%zu", units.exports);
    if (units.stopped_at >= INIT) emit_bool("init", units.init);
    if (units.stopped_at >= CONFIG) emit_bool("options", units.options);
    if (units.stopped_at >= ALLOC) emit_bool("alloc", units.alloc);
    if (units.stopped_at >= FIRST_CANDIDATE) {
        emit(",\"consumed\":%zu,\"parsed\":", units.consumed);
        if (api.zhuyin) emit("null"); else emit("%zu", units.parsed);
        emit_bool("guessed", units.guessed);
        emit_bool("counted", units.counted);
        emit(",\"count\":%u,\"type\":%d,\"text\":", units.count, units.type);
        emit_string(units.text);
    }
    if (units.stopped_at == END) emit_bool("teardown", true);
    emit("}\n");
}

static void emit_clock(const char *name, struct clock_pair value) {
    emit(",\"%s\":{\"wall_ns\":%llu,\"cpu_ns\":%llu}", name,
         (unsigned long long)value.wall, (unsigned long long)value.cpu);
}

/* §7.5 timing metrics, except the controller's process_wall. */
static void emit_times(void) {
    struct clock_pair setup = {
        units.lib_load.wall + units.init_time.wall + units.config.wall +
            units.alloc_time.wall + units.first.wall,
        units.lib_load.cpu + units.init_time.cpu + units.config.cpu +
            units.alloc_time.cpu + units.first.cpu};
    emit("{\"startup\":\"times\"");
    emit_clock("lib_load", units.lib_load);
    emit_clock("init", units.init_time);
    emit_clock("config", units.config);
    emit_clock("alloc", units.alloc_time);
    emit_clock("first_candidate_phase", units.first);
    emit_clock("ttfc", between(units.load_start, units.first_end));
    emit_clock("setup_plus_first_candidate", setup);
    emit_clock("drop_instance", units.drop);
    emit_clock("fini", units.fini);
    emit("}\n");
}

/* ── Untimed observation (§7.3) ───────────────────────────────────────── */

/* A candidate the list claims but the library cannot read is recorded as
 * none, as #842 did (observe.rs, observe_script). */
static void emit_candidates(void *instance, unsigned count, unsigned *max_rank) {
    emit("[");
    for (unsigned i = 0; i < count; i++) {
        void *candidate = NULL;
        int type = 0;
        const char *text = NULL;
        emit("%s{\"index\":%u", i ? "," : "", i);
        if (!api.get_candidate(instance, i, &candidate) || !candidate ||
            !api.get_candidate_type(instance, candidate, &type) ||
            !api.get_candidate_string(instance, candidate, &text) || !text) {
            emit(",\"none\":true}");
            continue;
        }
        emit(",\"type\":%d,\"nbest\":", type);
        if (api.zhuyin) {
            emit("\"not available on this facade\"");
        } else if (type == NBEST_MATCH_CANDIDATE) {
            uint8_t rank = 0;
            if (api.get_nbest(instance, candidate, &rank)) {
                emit("%u", rank);
                if (rank > *max_rank) *max_rank = rank;
            } else {
                emit("null");
            }
        } else {
            emit("null");
        }
        emit(",\"text\":");
        emit_string(text);
        emit("}");
    }
    emit("]");
}

static void observe(const char *kind, void *instance, const char *input, bool chewing) {
    bool reset = api.reset(instance);
    size_t consumed = chewing ? api.parse_chewing(instance, input) :
        api.parse_full(instance, input);
    emit("{\"startup\":\"%s\",\"input\":", kind);
    emit_string(input);
    emit(",\"parser\":\"%s\"", chewing ? "chewing" : "full");
    emit_bool("reset", reset);
    emit(",\"consumed\":%zu,\"parsed\":", consumed);
    if (api.zhuyin) emit("null"); else emit("%zu", api.parsed_length(instance));
    bool sentence_ok = api.guess_sentence(instance);
    emit_bool("sentence_ok", sentence_ok);
    if (sentence_ok) {
        char *sentence = NULL;
        bool read = api.zhuyin ? api.zhuyin_sentence(instance, &sentence) :
            api.get_sentence(instance, 0, &sentence);
        emit_bool("sentence_read", read);
        emit(",\"sentence\":");
        emit_string(read ? sentence : NULL);
        if (sentence) api.free_memory(sentence);
    }
    bool guessed = api.zhuyin ? api.guess_after(instance, 0) :
        api.guess_candidates(instance, 0, SORT);
    unsigned count = 0;
    bool counted = api.get_n_candidate(instance, &count);
    emit_bool("guessed", guessed);
    emit_bool("counted", counted);
    emit(",\"count\":%u,\"candidates\":", counted ? count : 0);
    unsigned max_rank = 0;
    emit_candidates(instance, counted ? count : 0, &max_rank);
    emit(",\"sentences\":[");
    for (unsigned rank = 1; sentence_ok && !api.zhuyin && rank <= max_rank && rank <= 2; rank++) {
        char *sentence = NULL;
        bool ok = api.get_sentence(instance, (uint8_t)rank, &sentence);
        emit("%s{\"rank\":%u", rank == 1 ? "" : ",", rank);
        emit_bool("ok", ok);
        emit(",\"text\":");
        emit_string(ok ? sentence : NULL);
        emit("}");
        if (sentence) api.free_memory(sentence);
    }
    emit("]}\n");
}

/* A fresh context and instance, so no transcript input reaches the U5
 * instance (§7.7). */
static void *open_fresh(const char *data, const char *user, void **context) {
    *context = api.init(data, user);
    if (!*context) die("fresh init returned NULL");
    if (!api.set_options(*context, OPTIONS)) die("fresh set_options returned false");
    void *instance = api.alloc_instance(*context);
    if (!instance) die("fresh alloc_instance returned NULL");
    return instance;
}

static void transcript(const char *data, const char *user, const char *witness) {
    FILE *inputs = NULL;
    if (witness) {
        inputs = fopen(witness, "r");
        if (!inputs) die("witness file %s: %s", witness, strerror(errno));
    }
    void *context;
    void *instance = open_fresh(data, user, &context);
    for (size_t i = 0; i < COUNT(SCRIPT_INPUTS); i++)
        observe("transcript", instance, SCRIPT_INPUTS[i], false);
    if (inputs) {
        char *line = NULL;
        size_t capacity = 0;
        while (getline(&line, &capacity, inputs) >= 0) {
            line[strcspn(line, "\n")] = '\0';
            if (*line) observe("transcript", instance, line, false);
        }
        bool failed = ferror(inputs);
        free(line);
        fclose(inputs);
        if (failed) die("witness file %s: read failed", witness);
    }
    if (api.zhuyin)
        for (size_t i = 0; i < COUNT(CHEWING_INPUTS); i++)
            observe("transcript", instance, CHEWING_INPUTS[i], true);
    api.free_instance(instance);
    api.fini(context);
}

static void persist_op(const char *op, const char *value) {
    emit("{\"startup\":\"persist\",\"op\":\"%s\",\"value\":%s}\n", op, value);
}

/* #842 child.rs persist at a204f9c0: import, save, close, reopen, observe. */
static void persist(const char *data, const char *user) {
    void *context = api.init(data, user);
    persist_op("init", context ? "true" : "false");
    if (!context) die("persist init returned NULL");
    persist_op("set_options", api.set_options(context, OPTIONS) ? "true" : "false");
    persist_op("load_phrase_library", api.load_library(context, 1) ? "true" : "false");
    void *iterator = api.begin_add(context, 1);
    if (!iterator) die("begin_add_phrases returned NULL");
    emit("{\"startup\":\"persist\",\"op\":\"add_phrases\",\"value\":[");
    for (size_t i = 0; i < COUNT(PERSIST_ROWS); i++)
        emit("%s%s", i ? "," : "", api.add_phrase(iterator, PERSIST_ROWS[i].phrase,
             PERSIST_ROWS[i].reading, PERSIST_ROWS[i].count) ? "true" : "false");
    emit("]}\n");
    api.end_add(iterator);
    bool saved = api.save(context);
    persist_op("save", saved ? "true" : "false");
    /* The pin returns false only for an unmodified store. */
    if (!saved) die("save returned false after a modifying import");
    api.fini(context);
    persist_op("fini", "true");
    void *instance = open_fresh(data, user, &context);
    persist_op("reopen", "true");
    for (size_t i = 0; i < COUNT(PERSIST_INPUTS); i++)
        observe("persist", instance, PERSIST_INPUTS[i], false);
    api.free_instance(instance);
    api.fini(context);
}

/* ── Invocation ───────────────────────────────────────────────────────── */

static bool point_named(const char *name, size_t length, enum point *out_point) {
    for (int i = 0; i < POINTS; i++) {
        if (strlen(POINT_NAMES[i]) == length && !strncmp(POINT_NAMES[i], name, length)) {
            *out_point = (enum point)i;
            return true;
        }
    }
    return false;
}

static void parse_exports(void) {
    const char *value = getenv("OXPINYIN_STARTUP_EXPORTS");
    if (!value || !*value) usage("OXPINYIN_STARTUP_EXPORTS is required");
    char *copy = strdup(value);
    if (!copy) die("export list allocation failed");
    for (char *name = strtok(copy, ","); name; name = strtok(NULL, ",")) {
        if (export_count == MAX_EXPORTS) usage("too many exports");
        for (size_t i = 0; i < export_count; i++)
            if (!strcmp(export_names[i], name)) usage("repeated export");
        export_names[export_count++] = name;
    }
    if (!export_count) usage("OXPINYIN_STARTUP_EXPORTS names no export");
}

static void parse_handshake(const char *list) {
    if (!*list) usage("empty handshake argument; use - for none");
    if (!strcmp(list, "-")) return;
    for (const char *p = list; *p;) {
        size_t length = strcspn(p, ",");
        enum point point;
        if (!point_named(p, length, &point)) usage("unknown handshake point");
        if (handshake[point]) usage("repeated handshake point");
        handshake[point] = true;
        p += length;
        if (*p == ',') { p++; if (!*p) usage("empty handshake point"); }
    }
    const char *fds = getenv("OXPINYIN_STARTUP_HANDSHAKE");
    char *end;
    if (!fds) usage("handshake points need OXPINYIN_STARTUP_HANDSHAKE");
    long w = strtol(fds, &end, 10);
    if (end == fds || *end != ',') usage("OXPINYIN_STARTUP_HANDSHAKE is not WRITE_FD,READ_FD");
    const char *second = end + 1;
    long r = strtol(second, &end, 10);
    if (end == second || *end || w < 0 || r < 0 || w > INT32_MAX || r > INT32_MAX)
        usage("OXPINYIN_STARTUP_HANDSHAKE is not WRITE_FD,READ_FD");
    handshake_write = (int)w;
    handshake_read = (int)r;
}

int main(int argc, char **argv) {
    struct rlimit no_core = {0, 0};
    if (setrlimit(RLIMIT_CORE, &no_core)) die("core limit: %s", strerror(errno));
    if (argc != 8) usage("expected seven arguments");
    const char *mode = argv[1], *path = argv[3], *data = argv[4], *user = argv[5];
    const char *bind = argv[6], *arg = argv[7];
    facade = argv[2];
    if (strcmp(facade, "pinyin") && strcmp(facade, "zhuyin")) usage("unknown facade");
    api.zhuyin = !strcmp(facade, "zhuyin");
    int flags = RTLD_LOCAL;
    if (!strcmp(bind, "now")) flags |= RTLD_NOW;
    else if (!strcmp(bind, "lazy")) flags |= RTLD_LAZY;
    else usage("unknown bind");

    enum point stop = END;
    long probe = -1;
    const char *witness = NULL;
    if (!strcmp(mode, "verify")) {
        if (strcmp(arg, "-")) witness = arg;
    } else if (!strcmp(mode, "measure")) {
        parse_handshake(arg);
    } else if (!strcmp(mode, "stop")) {
        if (!point_named(arg, strlen(arg), &stop)) usage("unknown stop point");
    } else if (!strcmp(mode, "probe")) {
        char *end;
        probe = strtol(arg, &end, 10);
        if (end == arg || *end || probe < 0 || probe >= (long)COUNT(HOSTILE_INPUTS))
            usage("probe index out of range");
    } else if (!strcmp(mode, "persist")) {
        if (strcmp(arg, "-")) usage("persist takes -");
    } else {
        usage("unknown mode");
    }
    parse_exports();
    prepare_required();
    /* Fault the output buffer before U1, so no unit grows it. */
    reserve(OUTPUT_RESERVE / 2);
    memset(out.data, 0, out.capacity);

    emit("{\"startup\":\"run\",\"mode\":\"%s\",\"facade\":\"%s\",\"bind\":\"%s\"}\n",
         mode, facade, bind);
    if (!strcmp(mode, "persist")) {
        run_units(path, flags, data, user, LIB_LOAD, false);
        persist(data, user);
        finish();
    }
    run_units(path, flags, data, user, stop, !strcmp(mode, "verify"));
    emit_units();
    if (!strcmp(mode, "measure")) emit_times();
    if (!strcmp(mode, "verify")) transcript(data, user, witness);
    if (probe >= 0) {
        void *context;
        void *instance = open_fresh(data, user, &context);
        observe("probe", instance, HOSTILE_INPUTS[probe], false);
        api.free_instance(instance);
        api.fini(context);
    }
    finish();
}
