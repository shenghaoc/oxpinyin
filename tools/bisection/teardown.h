/* Native side of contract-diff.py --teardown. Included only by bisect.c.
 * Python provisions private profiles and observes /proc while this child is
 * stopped on a pipe. No interpreter, filesystem inspection or memory reader
 * runs inside the measured ABI calls. Existing bisect modes do not enter here.
 */
#include <limits.h>
#include <sys/resource.h>
#if defined(__linux__)
#include <sys/mman.h>
#include <sys/prctl.h>
#endif
#if defined(__GLIBC__)
#include <malloc.h>
#endif
#if !defined(CLOCK_MONOTONIC_RAW)
#define CLOCK_MONOTONIC_RAW CLOCK_MONOTONIC
#endif
#if defined(OXPINYIN_TEARDOWN_MASSIF) || defined(OXPINYIN_TEARDOWN_MEMCHECK)
#include <valgrind/valgrind.h>
#endif
#ifdef OXPINYIN_TEARDOWN_MEMCHECK
#include <valgrind/memcheck.h>
#endif

#define TD_MAX_INSTANCES 16
#define TD_MAX_CANDIDATES 512
#define TD_TEXT_BYTES 512

/* wall and cpu are clock reads. The rest is one getrusage(RUSAGE_SELF):
 * majflt needed I/O, minflt did not; nvcsw/nivcsw are context switches.
 * user/sys follow the kernel's CPU accounting. Under tick accounting
 * (CONFIG_TICK_CPU_ACCOUNTING) their split is sampled at the tick, so a
 * short interval's user/sys values are quantized; cpu is not. */
struct td_clock { uint64_t wall, cpu, user, sys, minflt, majflt, nvcsw, nivcsw; };
struct td_candidate { int type, nbest; char text[TD_TEXT_BYTES]; };
struct td_observation {
    size_t consumed, parsed;
    bool guessed;
    unsigned count;
    struct td_candidate candidates[TD_MAX_CANDIDATES];
};
struct td_api {
    struct symbols common;
    bool zhuyin;
    bool (*guess_after)(void *, size_t);
    bool (*zhuyin_sentence)(void *, char **);
    bool (*load_library)(void *, uint8_t);
    void (*free_memory)(void *);
};

static uint64_t td_timeval_ns(struct timeval value) {
    return (uint64_t)value.tv_sec * 1000000000ULL + (uint64_t)value.tv_usec * 1000ULL;
}

static struct td_clock td_now(void) {
    struct timespec wall, cpu;
    if (clock_gettime(CLOCK_MONOTONIC_RAW, &wall) ||
        clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &cpu)) {
        perror("teardown clock");
        exit(1);
    }
    struct rusage usage;
    if (getrusage(RUSAGE_SELF, &usage)) { perror("teardown rusage"); exit(1); }
    return (struct td_clock){
        .wall = (uint64_t)wall.tv_sec * 1000000000ULL + (uint64_t)wall.tv_nsec,
        .cpu = (uint64_t)cpu.tv_sec * 1000000000ULL + (uint64_t)cpu.tv_nsec,
        .user = td_timeval_ns(usage.ru_utime), .sys = td_timeval_ns(usage.ru_stime),
        .minflt = (uint64_t)usage.ru_minflt, .majflt = (uint64_t)usage.ru_majflt,
        .nvcsw = (uint64_t)usage.ru_nvcsw, .nivcsw = (uint64_t)usage.ru_nivcsw};
}

static struct td_clock td_elapsed(struct td_clock start) {
    struct td_clock end = td_now();
    return (struct td_clock){end.wall - start.wall, end.cpu - start.cpu, end.user - start.user,
                             end.sys - start.sys, end.minflt - start.minflt, end.majflt - start.majflt,
                             end.nvcsw - start.nvcsw, end.nivcsw - start.nivcsw};
}

static void td_add(struct td_clock *sum, struct td_clock value) {
    sum->wall += value.wall; sum->cpu += value.cpu; sum->user += value.user; sum->sys += value.sys;
    sum->minflt += value.minflt; sum->majflt += value.majflt;
    sum->nvcsw += value.nvcsw; sum->nivcsw += value.nivcsw;
}

/* Stable, separately selectable Callgrind collection boundaries. All native
 * preparation is outside these functions. The wrapper's exclusive cost is
 * reported separately from the subject's inclusive call edge. */
__attribute__((noinline))
void bisect_teardown_free_unit(fn_pinyin_free_instance call, void *instance) {
    call(instance);
    __asm__ volatile ("" ::: "memory");
}

__attribute__((noinline))
void bisect_teardown_fini_unit(fn_pinyin_fini call, void *context) {
    call(context);
    __asm__ volatile ("" ::: "memory");
}

static void *td_symbol(void *library, const char *facade, const char *suffix) {
    char name[128];
    if (snprintf(name, sizeof name, "%s_%s", facade, suffix) >= (int)sizeof name)
        exit(2);
    dlerror();
    void *symbol = dlsym(library, name);
    const char *error = dlerror();
    if (error || !symbol) {
        fprintf(stderr, "teardown missing symbol %s: %s\n", name, error ? error : "NULL");
        exit(1);
    }
    return symbol;
}

static void td_resolve(void *library, const char *facade, struct td_api *api) {
    memset(api, 0, sizeof *api);
    api->zhuyin = strcmp(facade, "zhuyin") == 0;
    /* The controller supplies the existing version script's complete export
     * set, so U1 includes resolution of every facade symbol. */
    const char *exports = getenv("OXPINYIN_TEARDOWN_EXPORTS");
    if (!exports || !*exports) {
        fputs("teardown requires OXPINYIN_TEARDOWN_EXPORTS\n", stderr);
        exit(2);
    }
    while (*exports) {
        const char *end = strchr(exports, ',');
        size_t length = end ? (size_t)(end - exports) : strlen(exports);
        char name[128];
        if (!length || length >= sizeof name) exit(2);
        memcpy(name, exports, length);
        name[length] = '\0';
        if (!dlsym(library, name)) {
            fprintf(stderr, "teardown missing export %s\n", name);
            exit(1);
        }
        if (!end) break;
        exports = end + 1;
    }
#define TD_RESOLVE(FIELD, SUFFIX) \
    *(void **)(&api->common.FIELD) = td_symbol(library, facade, SUFFIX)
    TD_RESOLVE(init, "init");
    TD_RESOLVE(fini, "fini");
    TD_RESOLVE(alloc_instance, "alloc_instance");
    TD_RESOLVE(free_instance, "free_instance");
    TD_RESOLVE(set_options, "set_options");
    TD_RESOLVE(parse_full, "parse_more_full_pinyins");
    TD_RESOLVE(parse_chewing, "parse_more_chewings");
    TD_RESOLVE(reset, "reset");
    TD_RESOLVE(guess_sentence, "guess_sentence");
    TD_RESOLVE(get_n_candidate, "get_n_candidate");
    TD_RESOLVE(get_candidate, "get_candidate");
    TD_RESOLVE(get_candidate_type, "get_candidate_type");
    TD_RESOLVE(get_candidate_string, "get_candidate_string");
    TD_RESOLVE(save, "save");
    TD_RESOLVE(begin_add_phrases, "begin_add_phrases");
    TD_RESOLVE(iterator_add_phrase, "iterator_add_phrase");
    TD_RESOLVE(end_add_phrases, "end_add_phrases");
    *(void **)(&api->load_library) = td_symbol(library, facade, "load_phrase_library");
    *(void **)(&api->free_memory) = dlsym(library, "g_free");
    if (!api->free_memory) exit(1);
    if (api->zhuyin) {
        *(void **)(&api->guess_after) = td_symbol(library, facade, "guess_candidates_after_cursor");
        /* zhuyin.h:256-257 at 074a2219 has no nbest-index argument. */
        *(void **)(&api->zhuyin_sentence) = td_symbol(library, facade, "get_sentence");
    } else {
        TD_RESOLVE(get_sentence, "get_sentence");
        TD_RESOLVE(guess_candidates, "guess_candidates");
        TD_RESOLVE(get_parsed_input_length, "get_parsed_input_length");
        TD_RESOLVE(get_candidate_nbest_index, "get_candidate_nbest_index");
        TD_RESOLVE(begin_get_phrases, "begin_get_phrases");
        TD_RESOLVE(iterator_has_next, "iterator_has_next_phrase");
        TD_RESOLVE(iterator_get_next, "iterator_get_next_phrase");
        TD_RESOLVE(end_get_phrases, "end_get_phrases");
    }
#undef TD_RESOLVE
}

static bool td_read_candidate(const struct td_api *api, void *instance,
                              unsigned index, struct td_candidate *out) {
    lookup_candidate_t *candidate = NULL;
    lookup_candidate_type_t type;
    const char *text = NULL;
    if (!api->common.get_candidate(instance, index, &candidate) || !candidate ||
        !api->common.get_candidate_type(instance, candidate, &type) ||
        !api->common.get_candidate_string(instance, candidate, &text) || !text ||
        strlen(text) >= sizeof out->text) return false;
    out->type = (int)type;
    out->nbest = -1;
    if (!api->zhuyin && type == NBEST_MATCH_CANDIDATE) {
        uint8_t nbest = 0;
        if (!api->common.get_candidate_nbest_index(instance, candidate, &nbest)) return false;
        out->nbest = nbest;
    }
    strcpy(out->text, text);
    return true;
}

static bool td_first(const struct td_api *api, void *instance, const char *input,
                     struct td_observation *out) {
    out->consumed = api->common.parse_full(instance, input);
    out->parsed = api->zhuyin ? out->consumed : api->common.get_parsed_input_length(instance);
    out->guessed = api->zhuyin ? api->guess_after(instance, 0) :
        api->common.guess_candidates(instance, 0, DEFAULT_SORT);
    return api->common.get_n_candidate(instance, &out->count) &&
        out->count <= TD_MAX_CANDIDATES &&
        (!out->count || td_read_candidate(api, instance, 0, &out->candidates[0]));
}

static void td_print_observation(const struct td_api *api, const struct td_observation *out) {
    printf("{\"consumed\":%zu,\"parsed\":", out->consumed);
    if (api->zhuyin) printf("null"); else printf("%zu", out->parsed);
    printf(",\"guessed\":%s,\"count\":%u,\"candidates\":[",
           out->guessed ? "true" : "false", out->count);
    for (unsigned i = 0; i < out->count; i++) {
        printf("%s{\"type\":%d,\"nbest\":", i ? "," : "", out->candidates[i].type);
        if (api->zhuyin) printf("null"); else printf("%d", out->candidates[i].nbest);
        printf(",\"text\":");
        perf_json_string(out->candidates[i].text);
        putchar('}');
    }
    printf("]}");
}

static void td_checkpoint(const char *point, int cycle, bool wait) {
    if (!wait) return;
#ifdef OXPINYIN_TEARDOWN_MASSIF
    if (strcmp(point, "reset")) {
        fprintf(stderr, "TEARDOWN_HEAP %s BEGIN\n", point);
        if (VALGRIND_MONITOR_COMMAND("detailed_snapshot /dev/stderr")) exit(1);
        fprintf(stderr, "TEARDOWN_HEAP %s END\n", point);
    }
#endif
#ifdef OXPINYIN_TEARDOWN_MEMCHECK
    /* Diagnostic builds only. These scans are outside ABI timing and do not
     * enter shipping figures. Count live blocks independently of glibc arenas. */
    if ((!strcmp(point, "M0")) || (cycle == 1 && strcmp(point, "reset")) ||
        (!strcmp(point, "M3") && (cycle == 10 || cycle == 50 || cycle == 100))) {
        unsigned long lost, possible, reachable, suppressed;
        unsigned long lost_blocks, possible_blocks, reachable_blocks, suppressed_blocks;
        fprintf(stderr, "TEARDOWN_LEAK %s %d BEGIN\n", point, cycle);
        VALGRIND_DO_LEAK_CHECK;
        VALGRIND_COUNT_LEAKS(lost, possible, reachable, suppressed);
        VALGRIND_COUNT_LEAK_BLOCKS(lost_blocks, possible_blocks, reachable_blocks, suppressed_blocks);
        printf("{\"heap_live\":true,\"point\":\"%s\",\"cycle\":%d,"
               "\"lost_bytes\":%lu,\"possible_bytes\":%lu,\"reachable_bytes\":%lu,\"suppressed_bytes\":%lu,"
               "\"lost_blocks\":%lu,\"possible_blocks\":%lu,\"reachable_blocks\":%lu,\"suppressed_blocks\":%lu}\n",
               point, cycle, lost, possible, reachable, suppressed,
               lost_blocks, possible_blocks, reachable_blocks, suppressed_blocks);
        fprintf(stderr, "TEARDOWN_LEAK %s %d END\n", point, cycle);
    }
#endif
    /* mallinfo2 is a native, allocation-free glibc read. All formatting and
     * pipe synchronization are outside every timed/profiler ABI interval. */
#if defined(__GLIBC__) && (__GLIBC__ > 2 || (__GLIBC__ == 2 && __GLIBC_MINOR__ >= 33))
    struct mallinfo2 arena = mallinfo2();
    printf("{\"checkpoint\":\"%s\",\"cycle\":%d,\"allocator\":{"
           "\"arena\":%zu,\"uordblks\":%zu,\"fordblks\":%zu,"
           "\"hblkhd\":%zu,\"hblks\":%zu,\"keepcost\":%zu}}\n",
           point, cycle, arena.arena, arena.uordblks, arena.fordblks,
           arena.hblkhd, arena.hblks, arena.keepcost);
#else
    printf("{\"checkpoint\":\"%s\",\"cycle\":%d,\"allocator\":null}\n", point, cycle);
#endif
    if (getchar() != '\n') {
        fputs("teardown checkpoint acknowledgement lost\n", stderr);
        exit(1);
    }
}

static bool td_observe_input(const struct td_api *api, void *instance,
                             const char *input, bool chewing) {
    bool reset = api->common.reset(instance);
    size_t consumed = chewing ? api->common.parse_chewing(instance, input) :
        api->common.parse_full(instance, input);
    size_t parsed = api->zhuyin ? consumed : api->common.get_parsed_input_length(instance);
    bool sentence_ok = api->common.guess_sentence(instance);
    char *sentence = NULL;
    bool sentence_read = sentence_ok && (api->zhuyin ? api->zhuyin_sentence(instance, &sentence) :
                                         api->common.get_sentence(instance, 0, &sentence));
    bool guessed = api->zhuyin ? api->guess_after(instance, 0) :
        api->common.guess_candidates(instance, 0, DEFAULT_SORT);
    unsigned count = 0;
    bool counted = api->common.get_n_candidate(instance, &count);
    printf("{\"transcript\":");
    perf_json_string(input);
    printf(",\"parser\":\"%s\",\"reset\":%s,\"consumed\":%zu,\"parsed\":",
           chewing ? "chewing" : "full", reset ? "true" : "false", consumed);
    if (api->zhuyin) printf("null"); else printf("%zu", parsed);
    printf(",\"sentence_ok\":%s,\"sentence_read\":%s,\"sentence\":",
           sentence_ok ? "true" : "false", sentence_read ? "true" : "false");
    if (sentence) perf_json_string(sentence); else printf("null");
    api->free_memory(sentence);
    printf(",\"guessed\":%s,\"counted\":%s,\"count\":%u,\"candidates\":[",
           guessed ? "true" : "false", counted ? "true" : "false", count);
    unsigned max_rank = 0;
    for (unsigned i = 0; counted && i < count; i++) {
        struct td_candidate candidate;
        memset(&candidate, 0, sizeof candidate);
        if (!td_read_candidate(api, instance, i, &candidate)) return false;
        if (candidate.nbest > (int)max_rank) max_rank = (unsigned)candidate.nbest;
        printf("%s{\"type\":%d,\"nbest\":", i ? "," : "", candidate.type);
        if (api->zhuyin) printf("null"); else printf("%d", candidate.nbest);
        printf(",\"text\":"); perf_json_string(candidate.text); putchar('}');
    }
    printf("],\"sentences\":[");
    for (unsigned rank = 1; sentence_ok && !api->zhuyin && rank <= max_rank && rank <= 2; rank++) {
        sentence = NULL;
        bool ok = api->common.get_sentence(instance, (uint8_t)rank, &sentence);
        printf("%s{\"rank\":%u,\"ok\":%s,\"text\":", rank == 1 ? "" : ",", rank, ok ? "true" : "false");
        if (sentence) perf_json_string(sentence); else printf("null");
        api->free_memory(sentence);
        putchar('}');
    }
    puts("]}");
    return true;
}

static bool td_observe_script(const struct td_api *api, void *instance) {
    static const char *inputs[] = {"nihao", "ni", "n", "zh", "zhongguo", "xi'an", "bu'tian",
        "fan'gan", "fangan", "chongke", "caisho", "paolen", "waimenggu", "naoxion",
        "liangniejue", "chuaipengdengzaimiu", "a", "nihao"};
    for (size_t i = 0; i < sizeof inputs / sizeof inputs[0]; i++)
        if (!td_observe_input(api, instance, inputs[i], false)) return false;
    const char *path = getenv("OXPINYIN_TEARDOWN_INPUTS");
    if (path) {
        FILE *input = fopen(path, "r");
        if (!input) return false;
        char *line = NULL;
        size_t capacity = 0;
        bool ok = true;
        while (getline(&line, &capacity, input) >= 0) {
            line[strcspn(line, "\n")] = '\0';
            if (*line && !td_observe_input(api, instance, line, false)) { ok = false; break; }
        }
        if (ferror(input)) ok = false;
        free(line);
        fclose(input);
        if (!ok) return false;
    }
    if (api->zhuyin) {
        static const char *chewings[] = {"su3cl3", "ji3", "5j/ "};
        for (size_t i = 0; i < sizeof chewings / sizeof chewings[0]; i++)
            if (!td_observe_input(api, instance, chewings[i], true)) return false;
    }
    return true;
}

static bool td_populate(const struct td_api *api, void *context, const char *path) {
    FILE *input = fopen(path, "r");
    if (!input) { perror("teardown rows"); return false; }
    char *line = NULL;
    size_t capacity = 0;
    void *iterator = NULL;
    unsigned previous = 0, count = 0;
    bool valid = true;
    while (getline(&line, &capacity, input) >= 0) {
        char *tab1 = strchr(line, '\t');
        char *tab2 = tab1 ? strchr(tab1 + 1, '\t') : NULL;
        char *tab3 = tab2 ? strchr(tab2 + 1, '\t') : NULL;
        if (!tab3) { valid = false; break; }
        *tab1 = *tab2 = *tab3 = '\0';
        char *end;
        unsigned long library = strtoul(line, &end, 10);
        if (*end || library < 1 || library > 4) { valid = false; break; }
        long frequency = strtol(tab3 + 1, &end, 10);
        if ((*end && strcmp(end, "\n")) || frequency < 1 || frequency > INT_MAX) {
            valid = false; break;
        }
        if (previous != library) {
            if (iterator) api->common.end_add_phrases(iterator);
            iterator = NULL;
            (void)api->load_library(context, (uint8_t)library);
            iterator = api->common.begin_add_phrases(context, (uint8_t)library);
            if (!iterator) { valid = false; break; }
            previous = (unsigned)library;
        }
        if (!api->common.iterator_add_phrase(iterator, tab1 + 1, tab2 + 1, (int)frequency)) {
            fprintf(stderr, "teardown %s phrase import rejected row %u\n",
                    api->zhuyin ? "zhuyin" : "pinyin", count + 1);
            valid = false; break;
        }
        count++;
    }
    if (ferror(input)) valid = false;
    if (iterator) api->common.end_add_phrases(iterator);
    free(line);
    fclose(input);
    return valid && count > 0;
}

static int td_export(const struct td_api *api, void *context) {
    resolve_g_free();
    for (unsigned library = 1; library <= 4; library++) {
        (void)api->load_library(context, (uint8_t)library);
        void *iterator = api->common.begin_get_phrases(context, library);
        if (!iterator) return 1;
        while (api->common.iterator_has_next(iterator)) {
            char *phrase = NULL, *pinyin = NULL;
            int count = 0;
            if (!api->common.iterator_get_next(iterator, &phrase, &pinyin, &count) ||
                !phrase || !pinyin) return 1;
            printf("%u\t%s\t%s\t%d\n", library, phrase, pinyin, count);
            g_free_fn(phrase);
            g_free_fn(pinyin);
        }
        api->common.end_get_phrases(iterator);
    }
    return 0;
}

static void td_print_time(const char *name, struct td_clock value, bool comma) {
    printf("%s\"%s\":{\"wall_ns\":%llu,\"cpu_ns\":%llu,\"user_ns\":%llu,\"sys_ns\":%llu,"
           "\"minflt\":%llu,\"majflt\":%llu,\"nvcsw\":%llu,\"nivcsw\":%llu}", comma ? "," : "",
           name, (unsigned long long)value.wall, (unsigned long long)value.cpu,
           (unsigned long long)value.user, (unsigned long long)value.sys,
           (unsigned long long)value.minflt, (unsigned long long)value.majflt,
           (unsigned long long)value.nvcsw, (unsigned long long)value.nivcsw);
}

/* Diagnostic init probes (#1090), only in `init` mode, selected by
 * OXPINYIN_TEARDOWN_INIT_PROBE: `count` (perf counters), `sample:<ns>`
 * (cpu-clock IPs with kernel call chains), `faults` (the IP and address of
 * every minor and major page fault, with every mmap, in order) or `trace`
 * (begin/end writes on OXPINYIN_TEARDOWN_MARKER_FD for an external syscall
 * tracer). Opening,
 * mapping, reading and printing stay outside the init bracket. The enable and
 * disable calls adjoin it; `count` reports their cost from an empty bracket.
 * An event the kernel refuses is reported with its errno, never as zero. */
enum td_probe_kind { TD_PROBE_NONE, TD_PROBE_COUNT, TD_PROBE_SAMPLE, TD_PROBE_FAULTS, TD_PROBE_TRACE };
#define TD_PROBE_EVENTS 11
#define TD_RING_PAGES 64
#define TD_SAMPLE_STACK 48

struct td_probe {
    enum td_probe_kind kind;
    int marker, fds[TD_PROBE_EVENTS], errors[TD_PROBE_EVENTS];
    bool user_only[TD_PROBE_EVENTS];
    uint64_t period, ids[2], bracket[TD_PROBE_EVENTS][3];
    void *ring;
    size_t ring_bytes;
};

#if defined(__linux__)
static const struct { const char *name; uint32_t type; uint64_t config; } td_events[TD_PROBE_EVENTS] = {
    {"task-clock", PERF_TYPE_SOFTWARE, PERF_COUNT_SW_TASK_CLOCK},
    {"page-faults", PERF_TYPE_SOFTWARE, PERF_COUNT_SW_PAGE_FAULTS},
    {"minor-faults", PERF_TYPE_SOFTWARE, PERF_COUNT_SW_PAGE_FAULTS_MIN},
    {"major-faults", PERF_TYPE_SOFTWARE, PERF_COUNT_SW_PAGE_FAULTS_MAJ},
    {"context-switches", PERF_TYPE_SOFTWARE, PERF_COUNT_SW_CONTEXT_SWITCHES},
    {"cpu-migrations", PERF_TYPE_SOFTWARE, PERF_COUNT_SW_CPU_MIGRATIONS},
    {"cycles", PERF_TYPE_HARDWARE, PERF_COUNT_HW_CPU_CYCLES},
    {"instructions", PERF_TYPE_HARDWARE, PERF_COUNT_HW_INSTRUCTIONS},
    {"branch-misses", PERF_TYPE_HARDWARE, PERF_COUNT_HW_BRANCH_MISSES},
    {"L1-icache-load-misses", PERF_TYPE_HW_CACHE, PERF_COUNT_HW_CACHE_L1I |
        (PERF_COUNT_HW_CACHE_OP_READ << 8) | (PERF_COUNT_HW_CACHE_RESULT_MISS << 16)},
    {"iTLB-load-misses", PERF_TYPE_HW_CACHE, PERF_COUNT_HW_CACHE_ITLB |
        (PERF_COUNT_HW_CACHE_OP_READ << 8) | (PERF_COUNT_HW_CACHE_RESULT_MISS << 16)},
};

/* Kernel-inclusive first. A kernel that refuses that for this user
 * (perf_event_paranoid) gets one user-only retry, reported as such. */
static int td_perf_open(struct perf_event_attr *attr, bool *user_only, int *error) {
    *user_only = false;
    int fd = (int)syscall(SYS_perf_event_open, attr, 0, -1, -1, PERF_FLAG_FD_CLOEXEC);
    if (fd < 0 && (errno == EACCES || errno == EPERM)) {
        attr->exclude_kernel = 1;
        *user_only = true;
        fd = (int)syscall(SYS_perf_event_open, attr, 0, -1, -1, PERF_FLAG_FD_CLOEXEC);
    }
    *error = fd < 0 ? errno : 0;
    return fd;
}

static void td_probe_toggle(const struct td_probe *probe, bool on) {
    if ((probe->kind == TD_PROBE_COUNT || probe->kind == TD_PROBE_SAMPLE || probe->kind == TD_PROBE_FAULTS) &&
        prctl(on ? PR_TASK_PERF_EVENTS_ENABLE : PR_TASK_PERF_EVENTS_DISABLE, 0, 0, 0, 0)) {
        perror("teardown perf toggle");
        exit(1);
    }
}

static void td_probe_read(const struct td_probe *probe, uint64_t values[TD_PROBE_EVENTS][3]) {
    for (unsigned i = 0; i < TD_PROBE_EVENTS; i++) {
        memset(values[i], 0, sizeof values[i]);
        if (probe->fds[i] >= 0 && read(probe->fds[i], values[i], sizeof values[i]) != (ssize_t)sizeof values[i]) {
            perror("teardown perf read");
            exit(1);
        }
    }
}
#else
static void td_probe_toggle(const struct td_probe *probe, bool on) { (void)probe; (void)on; }
#endif

static void td_probe_open(struct td_probe *probe, bool init_mode) {
    memset(probe, 0, sizeof *probe);
    probe->marker = -1;
    for (unsigned i = 0; i < TD_PROBE_EVENTS; i++) probe->fds[i] = -1;
    const char *value = getenv("OXPINYIN_TEARDOWN_INIT_PROBE");
    if (!value || !*value) return;
    if (!init_mode) {
        fputs("teardown init probes require init mode\n", stderr);
        exit(2);
    }
    char *end;
    if (!strcmp(value, "count")) probe->kind = TD_PROBE_COUNT;
    else if (!strcmp(value, "trace")) probe->kind = TD_PROBE_TRACE;
    else if (!strcmp(value, "faults")) probe->kind = TD_PROBE_FAULTS;
    else if (!strncmp(value, "sample:", 7)) {
        probe->kind = TD_PROBE_SAMPLE;
        probe->period = strtoull(value + 7, &end, 10);
        if (*end || probe->period < 10000) exit(2);
    } else exit(2);
    if (probe->kind == TD_PROBE_TRACE) {
        const char *marker = getenv("OXPINYIN_TEARDOWN_MARKER_FD");
        long fd = marker ? strtol(marker, &end, 10) : -1;
        if (!marker || *end || fd < 3 || fd > INT_MAX) exit(2);
        probe->marker = (int)fd;
        return;
    }
#if defined(__linux__)
    struct perf_event_attr attr;
    if (probe->kind == TD_PROBE_COUNT) {
        for (unsigned i = 0; i < TD_PROBE_EVENTS; i++) {
            memset(&attr, 0, sizeof attr);
            attr.size = (uint32_t)sizeof attr;
            attr.type = td_events[i].type;
            attr.config = td_events[i].config;
            attr.disabled = 1;
            attr.exclude_hv = 1;
            attr.read_format = PERF_FORMAT_TOTAL_TIME_ENABLED | PERF_FORMAT_TOTAL_TIME_RUNNING;
            probe->fds[i] = td_perf_open(&attr, &probe->user_only[i], &probe->errors[i]);
        }
        return;
    }
    if (probe->kind == TD_PROBE_FAULTS) {
        /* Every fault is a sample. Both events write to one ring, so faults
         * and the minor event's mmap records keep their order. */
        static const uint64_t faults[2] = {PERF_COUNT_SW_PAGE_FAULTS_MIN, PERF_COUNT_SW_PAGE_FAULTS_MAJ};
        for (unsigned i = 0; i < 2; i++) {
            memset(&attr, 0, sizeof attr);
            attr.size = (uint32_t)sizeof attr;
            attr.type = PERF_TYPE_SOFTWARE;
            attr.config = faults[i];
            attr.sample_period = 1;
            attr.sample_type = PERF_SAMPLE_IDENTIFIER | PERF_SAMPLE_IP | PERF_SAMPLE_ADDR;
            attr.mmap = attr.mmap_data = attr.mmap2 = i == 0;
            attr.disabled = 1;
            attr.exclude_hv = 1;
            probe->fds[i] = td_perf_open(&attr, &probe->user_only[i], &probe->errors[i]);
            if (probe->fds[i] < 0) return;
            if (ioctl(probe->fds[i], PERF_EVENT_IOC_ID, &probe->ids[i])) { perror("teardown perf id"); exit(1); }
        }
    } else {
        memset(&attr, 0, sizeof attr);
        attr.size = (uint32_t)sizeof attr;
        attr.type = PERF_TYPE_SOFTWARE;
        attr.config = PERF_COUNT_SW_CPU_CLOCK;
        attr.sample_period = probe->period;
        attr.sample_type = PERF_SAMPLE_IP | PERF_SAMPLE_CALLCHAIN;
        attr.exclude_callchain_user = 1;
        attr.sample_max_stack = TD_SAMPLE_STACK;
        attr.disabled = 1;
        attr.exclude_hv = 1;
        probe->fds[0] = td_perf_open(&attr, &probe->user_only[0], &probe->errors[0]);
        if (probe->fds[0] < 0) return;
    }
    long page = sysconf(_SC_PAGESIZE);
    probe->ring_bytes = (size_t)page * (TD_RING_PAGES + 1);
    probe->ring = mmap(NULL, probe->ring_bytes, PROT_READ | PROT_WRITE, MAP_SHARED, probe->fds[0], 0);
    if (probe->ring == MAP_FAILED) { perror("teardown perf ring"); exit(1); }
    if (probe->kind == TD_PROBE_FAULTS && ioctl(probe->fds[1], PERF_EVENT_IOC_SET_OUTPUT, probe->fds[0])) {
        perror("teardown perf output");
        exit(1);
    }
#else
    if (probe->kind != TD_PROBE_TRACE) {
        for (unsigned i = 0; i < TD_PROBE_EVENTS; i++) probe->errors[i] = ENOSYS;
    }
#endif
}

/* Measure the enable/clock/disable bracket around nothing, once per sample,
 * immediately before the init bracket it calibrates. */
static void td_probe_calibrate(struct td_probe *probe) {
#if defined(__linux__)
    if (probe->kind != TD_PROBE_COUNT) return;
    td_probe_toggle(probe, true);
    struct td_clock start = td_now();
    (void)td_elapsed(start);
    td_probe_toggle(probe, false);
    td_probe_read(probe, probe->bracket);
#else
    (void)probe;
#endif
}

static void td_probe_mark(const struct td_probe *probe, const char *marker) {
    if (probe->kind == TD_PROBE_TRACE && write(probe->marker, marker, 1) != 1) {
        perror("teardown trace marker");
        exit(1);
    }
}

#if defined(__linux__)
static void td_ring_copy(void *out, const unsigned char *data, uint64_t size, uint64_t offset, size_t bytes) {
    for (size_t copied = 0; copied < bytes;) {
        uint64_t at = (offset + copied) % size;
        size_t run = (size_t)(size - at) < bytes - copied ? (size_t)(size - at) : bytes - copied;
        memcpy((unsigned char *)out + copied, data + at, run);
        copied += run;
    }
}
#endif

/* One JSON line after the I1 checkpoint. Values are raw totals of the init
 * bracket minus the empty calibration bracket; the latter is reported too. */
static void td_probe_report(const struct td_probe *probe) {
    if (probe->kind == TD_PROBE_NONE || probe->kind == TD_PROBE_TRACE) return;
    if (probe->kind == TD_PROBE_COUNT) {
        uint64_t total[TD_PROBE_EVENTS][3] = {{0}};
#if defined(__linux__)
        td_probe_read(probe, total);
#endif
        printf("{\"init_probe\":\"count\",\"events\":[");
        for (unsigned i = 0; i < TD_PROBE_EVENTS; i++) {
#if defined(__linux__)
            printf("%s{\"name\":\"%s\",\"errno\":%d,\"user_only\":%s", i ? "," : "", td_events[i].name,
                   probe->errors[i], probe->user_only[i] ? "true" : "false");
#else
            printf("%s{\"name\":null,\"errno\":%d,\"user_only\":false", i ? "," : "", probe->errors[i]);
#endif
            if (probe->fds[i] >= 0)
                printf(",\"value\":%llu,\"enabled_ns\":%llu,\"running_ns\":%llu,\"bracket\":%llu,"
                       "\"bracket_enabled_ns\":%llu,\"bracket_running_ns\":%llu}",
                       (unsigned long long)(total[i][0] - probe->bracket[i][0]),
                       (unsigned long long)(total[i][1] - probe->bracket[i][1]),
                       (unsigned long long)(total[i][2] - probe->bracket[i][2]),
                       (unsigned long long)probe->bracket[i][0],
                       (unsigned long long)probe->bracket[i][1],
                       (unsigned long long)probe->bracket[i][2]);
            else putchar('}');
        }
        puts("]}");
        return;
    }
    bool faults = probe->kind == TD_PROBE_FAULTS;
    int error = probe->errors[0] ? probe->errors[0] : faults ? probe->errors[1] : 0;
    printf("{\"init_probe\":\"%s\",\"period_ns\":%llu,\"errno\":%d,\"user_only\":%s",
           faults ? "faults" : "sample", (unsigned long long)probe->period, error,
           probe->user_only[0] || probe->user_only[1] ? "true" : "false");
#if defined(__linux__)
    if (!error) {
        struct perf_event_mmap_page *meta = probe->ring;
        long page = sysconf(_SC_PAGESIZE);
        const unsigned char *data = (const unsigned char *)probe->ring + page;
        uint64_t size = (uint64_t)probe->ring_bytes - (uint64_t)page;
        uint64_t head = __atomic_load_n(&meta->data_head, __ATOMIC_ACQUIRE), tail = meta->data_tail;
        /* An MMAP2 record carries a path of up to PATH_MAX bytes. */
        uint64_t lost = 0, throttled = 0, record[(PATH_MAX + 128) / 8];
        bool valid = head - tail <= size, first = true;
        printf(",\"samples\":[");
        while (valid && tail < head) {
            struct perf_event_header header;
            td_ring_copy(&header, data, size, tail, sizeof header);
            if (header.size < sizeof header || header.size > sizeof record || header.size % 8 ||
                tail + header.size > head) {
                valid = false;
                break;
            }
            td_ring_copy(record, data, size, tail, header.size);
            tail += header.size;
            uint64_t *words = record + 1, count = (header.size - sizeof header) / 8;
            bool kernel = (header.misc & PERF_RECORD_MISC_CPUMODE_MASK) == PERF_RECORD_MISC_KERNEL;
            if (faults && header.type == PERF_RECORD_SAMPLE) {
                /* IDENTIFIER, IP, ADDR. */
                if (count < 3 || (words[0] != probe->ids[0] && words[0] != probe->ids[1])) { valid = false; break; }
                printf("%s{\"fault\":\"%s\",\"kernel\":%s,\"ip\":%llu,\"addr\":%llu}", first ? "" : ",",
                       words[0] == probe->ids[0] ? "minor" : "major", kernel ? "true" : "false",
                       (unsigned long long)words[1], (unsigned long long)words[2]);
                first = false;
            } else if (faults && header.type == PERF_RECORD_MMAP2) {
                /* pid, tid; addr, len, pgoff; maj, min; ino, generation; prot, flags; path. */
                const unsigned char *body = (const unsigned char *)words;
                size_t bytes = header.size - sizeof header;
                if (bytes <= 64 || !memchr(body + 64, '\0', bytes - 64)) { valid = false; break; }
                uint32_t protection, flags;
                memcpy(&protection, body + 56, 4);
                memcpy(&flags, body + 60, 4);
                printf("%s{\"mmap\":%llu,\"len\":%llu,\"pgoff\":%llu,\"prot\":%u,\"flags\":%u,\"file\":",
                       first ? "" : ",", (unsigned long long)words[1], (unsigned long long)words[2],
                       (unsigned long long)words[3], protection, flags);
                perf_json_string((const char *)body + 64);
                putchar('}');
                first = false;
            } else if (header.type == PERF_RECORD_SAMPLE) {
                if (count < 2 || words[1] > count - 2) { valid = false; break; }
                printf("%s{\"kernel\":%s,\"ip\":%llu,\"chain\":[", first ? "" : ",", kernel ? "true" : "false",
                       (unsigned long long)words[0]);
                bool comma = false;
                for (uint64_t j = 0; j < words[1]; j++) {
                    if (words[2 + j] >= (uint64_t)PERF_CONTEXT_MAX) continue;
                    printf("%s%llu", comma ? "," : "", (unsigned long long)words[2 + j]);
                    comma = true;
                }
                printf("]}");
                first = false;
            } else if (header.type == PERF_RECORD_LOST && count >= 2) {
                lost += words[1];
            } else if (header.type == PERF_RECORD_THROTTLE) {
                throttled++;
            }
        }
        printf("],\"lost\":%llu,\"throttled\":%llu,\"valid\":%s",
               (unsigned long long)lost, (unsigned long long)throttled, valid ? "true" : "false");
    }
#endif
    puts("}");
}

/* Leaves a closed probe of kind NONE, so a later cycle never reuses it. */
static void td_probe_close(struct td_probe *probe) {
#if defined(__linux__)
    if (probe->ring && munmap(probe->ring, probe->ring_bytes)) { perror("teardown perf ring"); exit(1); }
    for (unsigned i = 0; i < TD_PROBE_EVENTS; i++)
        if (probe->fds[i] >= 0) close(probe->fds[i]);
#endif
    memset(probe, 0, sizeof *probe);
    probe->marker = -1;
    for (unsigned i = 0; i < TD_PROBE_EVENTS; i++) probe->fds[i] = -1;
}

static int run_teardown_mode(int argc, char **argv) {
    struct rlimit no_core = {0, 0};
    if (setrlimit(RLIMIT_CORE, &no_core)) { perror("teardown core limit"); return 1; }
    if (argc != 12 || (strcmp(argv[2], "pinyin") && strcmp(argv[2], "zhuyin"))) {
        fputs("usage: bisect --teardown pinyin|zhuyin SO DATA USER "
              "time|memory|init|export|populate|observe SAVE INSTANCES CYCLES "
              "first-candidate|dirty|multi ROWS_OR_DASH\n", stderr);
        return 2;
    }
    const char *mode = argv[6];
    bool wait = strcmp(mode, "memory") == 0;
    /* `init` is `time` with pauses after load (M0) and after init (I1) for the
     * parent's memory and page-cache reads, plus the optional init probe. */
    bool init_mode = strcmp(mode, "init") == 0;
    bool save = strcmp(argv[7], "1") == 0;
    bool populate = strcmp(mode, "populate") == 0;
    bool exporting = strcmp(mode, "export") == 0;
    bool observing = strcmp(mode, "observe") == 0;
    if (!wait && !init_mode && !populate && !exporting && !observing && strcmp(mode, "time")) return 2;
    if (strcmp(argv[7], "0") && !save) return 2;
    char *end;
    long instances = strtol(argv[8], &end, 10);
    if (*end || instances < 1 || instances > TD_MAX_INSTANCES) return 2;
    long cycles = strtol(argv[9], &end, 10);
    if (*end || cycles < 1 || cycles > 100 || (!wait && cycles != 1)) return 2;
    /* argv[10] is a named scenario: first-candidate, dirty, or multi. */
    bool dirty = strcmp(argv[10], "dirty") == 0;
    bool multi = strcmp(argv[10], "multi") == 0;
    if (!dirty && !multi && strcmp(argv[10], "first-candidate")) return 2;
    if (multi && instances != 2) return 2;
    if (save && !dirty && !populate) return 2;
    setvbuf(stdout, NULL, _IONBF, 0);
    setvbuf(stdin, NULL, _IONBF, 0);
    struct td_probe probe;
    td_probe_open(&probe, init_mode);
    if (wait || init_mode) {
        /* Fault the pipe/stdio wait path before load and M0, rather than
         * racing its first-use residency against the parent's smaps read. */
        puts("{\"teardown\":\"ready\"}");
        if (getchar() != '\n') return 1;
    }
    /* Fixed observation storage is faulted before M0; copying observations
     * cannot add a retained controller buffer to M1-M0. */
    struct td_observation observations[instances];
    memset(observations, 0, sizeof observations);
    struct td_clock start = td_now();
    void *handle = dlopen(argv[3], RTLD_NOW | RTLD_LOCAL);
    if (!handle) { fprintf(stderr, "teardown dlopen: %s\n", dlerror()); return 1; }
    struct td_api api;
    td_resolve(handle, argv[2], &api);
    struct td_clock load = td_elapsed(start);
    td_checkpoint("M0", 0, wait || init_mode);
    for (int cycle = 1; cycle <= cycles; cycle++) {
        void *allocated[TD_MAX_INSTANCES] = {0};
        struct td_clock beginning = td_now();
        td_probe_calibrate(&probe);
        td_probe_toggle(&probe, true);
        start = td_now();
        td_probe_mark(&probe, "B");
        void *context = api.common.init(argv[4], argv[5]);
        td_probe_mark(&probe, "E");
        struct td_clock init = td_elapsed(start);
        td_probe_toggle(&probe, false);
        if (!context) { fputs("teardown init returned NULL\n", stderr); return 1; }
        td_checkpoint("I1", cycle, init_mode);
        td_probe_report(&probe);
        td_probe_close(&probe);
        if (exporting) {
            int status = api.zhuyin ? 2 : td_export(&api, context);
            api.common.fini(context);
            dlclose(handle);
            return status;
        }
        if (populate) {
            bool ok = td_populate(&api, context, argv[11]);
            bool saved = ok && api.common.save(context);
            api.common.fini(context);
            dlclose(handle);
            if (saved) puts("{\"populated\":true,\"saved\":true}");
            return saved ? 0 : 1;
        }
        start = td_now();
        bool options = api.common.set_options(context, DEFAULT_FLAGS);
        struct td_clock config = td_elapsed(start);
        if (!options) { fputs("teardown set_options returned false\n", stderr); return 1; }
        struct td_clock alloc = {0}, first = {0}, frees[TD_MAX_INSTANCES], all_frees = {0};
        for (int i = 0; i < instances; i++) {
            start = td_now();
            allocated[i] = api.common.alloc_instance(context);
            struct td_clock duration = td_elapsed(start);
            td_add(&alloc, duration);
            if (!allocated[i]) { fputs("teardown alloc returned NULL\n", stderr); return 1; }
            start = td_now();
            bool valid = td_first(&api, allocated[i], "nihao", &observations[i]);
            duration = td_elapsed(start);
            td_add(&first, duration);
            if (!valid || (!observing && (observations[i].consumed != 5 || observations[i].parsed != 5 ||
                !observations[i].guessed || !observations[i].count || !observations[i].candidates[0].text[0]))) {
                fputs("teardown first candidate contract failed\n", stderr); return 1;
            }
            for (unsigned j = 1; j < observations[i].count; j++) {
                if (!td_read_candidate(&api, allocated[i], j, &observations[i].candidates[j])) return 1;
            }
        }
        if (observing && !td_observe_script(&api, allocated[0])) return 1;
        if (dirty && !td_populate(&api, context, argv[11])) return 1;
        bool saved = false;
        struct td_clock saving = {0};
        if (save) {
            start = td_now(); saved = api.common.save(context); saving = td_elapsed(start);
            if (!saved) { fputs("teardown named save returned false\n", stderr); return 1; }
        }
        td_checkpoint("M1", cycle, wait);
        for (int i = 0; i < instances; i++) {
            start = td_now();
            bisect_teardown_free_unit(api.common.free_instance, allocated[i]);
            frees[i] = td_elapsed(start);
            td_add(&all_frees, frees[i]);
            allocated[i] = NULL;
            if (multi && i == 0) {
                struct td_observation remaining;
                memset(&remaining, 0, sizeof remaining);
                if (!td_first(&api, allocated[1], "nihao", &remaining) ||
                    remaining.count != observations[1].count ||
                    strcmp(remaining.candidates[0].text, observations[1].candidates[0].text)) {
                    fputs("teardown surviving instance failed\n", stderr); return 1;
                }
            }
        }
        td_checkpoint("M2", cycle, wait);
        start = td_now();
        bisect_teardown_fini_unit(api.common.fini, context);
        struct td_clock fini = td_elapsed(start);
        struct td_clock lifecycle = td_elapsed(beginning);
        td_checkpoint("M3", cycle, wait);
        printf("{\"teardown\":\"sample\",\"cycle\":%d,\"options\":true,\"saved\":", cycle);
        if (save) printf("%s", saved ? "true" : "false"); else printf("null");
        printf(",\"observations\":[");
        for (int i = 0; i < instances; i++) {
            if (i) putchar(',');
            td_print_observation(&api, &observations[i]);
        }
        printf("],\"times\":{");
        td_print_time("load_ns", load, false);
        td_print_time("init_ns", init, true);
        td_print_time("config_ns", config, true);
        td_print_time("alloc_ns", alloc, true);
        td_print_time("first_candidate_ns", first, true);
        td_print_time("lifecycle_ns", lifecycle, true);
        if (save) td_print_time("save_ns", saving, true);
        td_print_time("free_all_instances_ns", all_frees, true);
        td_print_time("fini_ns", fini, true);
        struct td_clock teardown = all_frees;
        td_add(&teardown, fini);
        td_print_time("teardown_total_ns", teardown, true);
        printf("},\"free_instance_ns\":[");
        for (int i = 0; i < instances; i++) {
            printf("%s{", i ? "," : "");
            td_print_time("call", frees[i], false);
            putchar('}');
        }
        printf("]}\n");
        /* The next-cycle M0 handshake lets the parent restore exactly the
         * same starting profile after fini, with the library still loaded. */
        if (cycle < cycles) td_checkpoint("reset", cycle, wait);
    }
    start = td_now();
    int closed = dlclose(handle);
    struct td_clock closing = td_elapsed(start);
    printf("{\"teardown\":\"closed\",\"status\":%d,\"times\":{", closed);
    td_print_time("dlclose_ns", closing, false);
    puts("}}");
    return closed ? 1 : 0;
}
