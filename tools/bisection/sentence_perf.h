/* Additional bisect workloads (#1348). The old --perf path is independent.
 * Sentence: parse outside the anchor; collect guess_sentence alone.
 * Typing: one reset, then parse/guess_sentence/guess_candidates at offset 0
 * for each accumulated ASCII prefix on the same instance. Observations and
 * cleanup stay outside both anchors. No training or saving is performed. */

struct sentence_perf_step {
    size_t parsed;
    bool sentence;
    bool candidates;
};

__attribute__((noinline)) bool
bisect_sentence_unit(const struct symbols *s, pinyin_instance_t *inst) {
    return s->guess_sentence(inst);
}

__attribute__((noinline)) struct sentence_perf_step
bisect_typing_unit(const struct symbols *s, pinyin_instance_t *inst,
                   const char *input) {
    struct sentence_perf_step step;
    step.parsed = s->parse_full(inst, input);
    step.sentence = s->guess_sentence(inst);
    step.candidates = s->guess_candidates(inst, 0, DEFAULT_SORT);
    return step;
}

/* Full observations, not a count-only checksum. A failed ABI getter fails
 * the worker; NULL strings remain distinguishable from empty strings. */
static void sentence_perf_string(const char *text) {
    if (text)
        perf_json_string(text);
    else
        fputs("null", stdout);
}

static bool sentence_perf_observe(const struct symbols *s,
                                 pinyin_instance_t *inst, const char *input,
                                 struct sentence_perf_step step, bool typing) {
    char *sentence = NULL;
    bool ok = s->get_sentence(inst, 0, &sentence);
    fputs("{\"input\":", stdout);
    perf_json_string(input);
    printf(",\"parsed\":%zu,\"guess_sentence\":%s,\"get_sentence\":%s,\"sentence\":",
           step.parsed, step.sentence ? "true" : "false", ok ? "true" : "false");
    sentence_perf_string(sentence);
    if (sentence)
        g_free_fn(sentence);
    if (!ok)
        return false;
    if (typing) {
        guint count = 0;
        if (!s->get_n_candidate(inst, &count))
            return false;
        printf(",\"guess_candidates\":%s,\"candidates\":[",
               step.candidates ? "true" : "false");
        for (guint i = 0; i < count; ++i) {
            lookup_candidate_t *candidate = NULL;
            const char *text = NULL;
            lookup_candidate_type_t type = NORMAL_CANDIDATE;
            uint8_t nbest = 0;
            if (!s->get_candidate(inst, i, &candidate) || !candidate ||
                !s->get_candidate_string(inst, candidate, &text) ||
                !s->get_candidate_type(inst, candidate, &type))
                return false;
            if (type == NBEST_MATCH_CANDIDATE &&
                !s->get_candidate_nbest_index(inst, candidate, &nbest))
                return false;
            if (i)
                putchar(',');
            printf("{\"type\":%d,\"nbest\":", (int)type);
            if (type == NBEST_MATCH_CANDIDATE)
                printf("%u", (unsigned)nbest);
            else
                fputs("null", stdout);
            fputs(",\"text\":", stdout);
            sentence_perf_string(text);
            putchar('}');
        }
        putchar(']');
    }
    putchar('}');
    return true;
}

static int run_sentence_perf_mode(int argc, char **argv) {
    if (argc != 6) {
        fprintf(stderr, "Usage: %s --perf-sentence|--perf-typing-sequence SO DATA ASCII-PINYIN ITERATIONS\n", argv[0]);
        return 2;
    }
    const char *input = argv[4];
    size_t length = strlen(input);
    if (!length || length > 255 || !*argv[5])
        return 2;
    for (size_t i = 0; i < length; ++i)
        if ((input[i] < 'a' || input[i] > 'z') && input[i] != '\'')
            return 2;
    for (const char *p = argv[5]; *p; ++p)
        if (*p < '0' || *p > '9')
            return 2;
    errno = 0;
    unsigned long iterations = strtoul(argv[5], NULL, 10);
    if (errno || iterations < 1 || iterations > 100000)
        return 2;
    bool typing = strcmp(argv[1], "--perf-typing-sequence") == 0;
    int status = 1;
    char user_dir[] = "/tmp/bisect-sentence-XXXXXX";
    if (!mkdtemp(user_dir))
        return 1;
    void *handle = dlopen(argv[2], RTLD_NOW);
    pinyin_context_t *ctx = NULL;
    pinyin_instance_t *inst = NULL;
    struct symbols s;
    if (!handle) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        goto cleanup;
    }
    int missing = resolve_perf_symbols(handle, &s, false);
    RESOLVE(handle, s, guess_sentence, "pinyin_guess_sentence");
    RESOLVE(handle, s, get_sentence, "pinyin_get_sentence");
    RESOLVE(handle, s, get_candidate, "pinyin_get_candidate");
    RESOLVE(handle, s, get_candidate_string, "pinyin_get_candidate_string");
    RESOLVE(handle, s, get_candidate_type, "pinyin_get_candidate_type");
    RESOLVE(handle, s, get_candidate_nbest_index, "pinyin_get_candidate_nbest_index");
    if (missing)
        goto cleanup;
    resolve_g_free();
    ctx = s.init(argv[3], user_dir);
    if (!ctx || !s.set_options(ctx, DEFAULT_FLAGS))
        goto cleanup;
    inst = s.alloc_instance(ctx);
    if (!inst)
        goto cleanup;
    uint64_t elapsed = 0;
    printf("{\"pid\":%ld,\"workload\":\"%s\",\"iterations\":%lu,\"observations\":[",
           (long)getpid(), typing ? "typing-sequence" : "sentence", iterations);
    for (unsigned long repeat = 0; repeat < iterations; ++repeat) {
        if (!s.reset(inst))
            goto cleanup;
        if (repeat)
            putchar(',');
        putchar('[');
        size_t first = typing ? 1 : length;
        for (size_t n = first; n <= length; ++n) {
            char prefix[256];
            memcpy(prefix, input, n);
            prefix[n] = '\0';
            struct sentence_perf_step step = {0, false, false};
            if (!typing)
                step.parsed = s.parse_full(inst, prefix);
            uint64_t start = now_ns();
            if (typing)
                step = bisect_typing_unit(&s, inst, prefix);
            else
                step.sentence = bisect_sentence_unit(&s, inst);
            uint64_t end = now_ns();
            if (!start || end < start || !step.parsed || !step.sentence ||
                (typing && !step.candidates))
                goto cleanup;
            elapsed += end - start;
            if (n != first)
                putchar(',');
            if (!sentence_perf_observe(&s, inst, prefix, step, typing))
                goto cleanup;
        }
        putchar(']');
    }
    printf("],\"elapsed_ns\":%llu}\n", (unsigned long long)elapsed);
    status = ferror(stdout) ? 1 : 0;
cleanup:
    if (inst)
        s.free_instance(inst);
    if (ctx)
        s.fini(ctx);
    if (handle)
        dlclose(handle);
    perf_remove_user_dir(user_dir);
    if (status)
        fputs("sentence workload failed\n", stderr);
    return status;
}
