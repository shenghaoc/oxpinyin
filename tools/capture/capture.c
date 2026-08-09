#include <stdbool.h>

#include <pinyin.h>

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define CAPTURE_SCHEMA "pinyin-capture-v1"
#define MAX_CAPTURED_CANDIDATES 10

static const char *oracle_pin_ref;

#define BASE_FLAGS ((pinyin_option_t)IS_PINYIN)
#define DEFAULT_FLAGS                                                        \
    ((pinyin_option_t)(IS_PINYIN | PINYIN_INCOMPLETE | USE_DIVIDED_TABLE |  \
                       USE_RESPLIT_TABLE))

struct capture_case {
    const char *id;
    const char *input;
    pinyin_option_t flags;
};

struct option_case {
    const char *id;
    const char *input;
    pinyin_option_t flag;
};

static void print_escaped(const char *value) {
    const unsigned char *cursor = (const unsigned char *)value;

    while (*cursor != '\0') {
        switch (*cursor) {
        case '\\':
            fputs("\\\\", stdout);
            break;
        case '\t':
            fputs("\\t", stdout);
            break;
        case '\n':
            fputs("\\n", stdout);
            break;
        case '\r':
            fputs("\\r", stdout);
            break;
        default:
            if (*cursor < 0x20 || *cursor == 0x7f) {
                fprintf(stdout, "\\x%02x", *cursor);
            } else {
                fputc(*cursor, stdout);
            }
            break;
        }
        ++cursor;
    }
}

static void print_hex(const char *value) {
    const unsigned char *cursor = (const unsigned char *)value;

    while (*cursor != '\0') {
        fprintf(stdout, "%02x", *cursor);
        ++cursor;
    }
}

static void free_candidate_strings(gchar **candidates, size_t count) {
    for (size_t index = 0; index < count; ++index) {
        g_free(candidates[index]);
    }
}

static bool emit_case(pinyin_context_t *context, const char *family,
                      const struct capture_case *capture) {
    pinyin_instance_t *instance;
    gchar *candidates[MAX_CAPTURED_CANDIDATES] = {NULL};
    guint candidate_total = 0;
    size_t captured_candidates = 0;
    size_t parse_return;
    size_t parsed_length;
    size_t input_length = strlen(capture->input);
    bool candidates_available = false;
    bool first_segment = true;

    if ((capture->flags & DYNAMIC_ADJUST) != 0) {
        fprintf(stderr, "%s: dynamic adjustment must remain disabled\n",
                capture->id);
        return false;
    }
    if (!pinyin_set_options(context, capture->flags)) {
        fprintf(stderr, "%s: pinyin_set_options failed\n", capture->id);
        return false;
    }

    instance = pinyin_alloc_instance(context);
    if (instance == NULL) {
        fprintf(stderr, "%s: pinyin_alloc_instance failed\n", capture->id);
        return false;
    }

    parse_return =
        pinyin_parse_more_full_pinyins(instance, capture->input);
    parsed_length = pinyin_get_parsed_input_length(instance);
    if (parsed_length > input_length) {
        fprintf(stderr, "%s: parsed length exceeds input length\n", capture->id);
        pinyin_free_instance(instance);
        return false;
    }

    if (parsed_length > 0 &&
        pinyin_guess_candidates(
            instance, 0,
            SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY)) {
        if (!pinyin_get_n_candidate(instance, &candidate_total)) {
            fprintf(stderr, "%s: pinyin_get_n_candidate failed\n", capture->id);
            pinyin_free_instance(instance);
            return false;
        }
        candidates_available = true;
        captured_candidates = candidate_total < MAX_CAPTURED_CANDIDATES
                                  ? candidate_total
                                  : MAX_CAPTURED_CANDIDATES;
        for (size_t index = 0; index < captured_candidates; ++index) {
            lookup_candidate_t *candidate = NULL;
            const gchar *candidate_string = NULL;

            if (!pinyin_get_candidate(instance, (guint)index, &candidate) ||
                candidate == NULL ||
                !pinyin_get_candidate_string(instance, candidate,
                                             &candidate_string) ||
                candidate_string == NULL) {
                fprintf(stderr, "%s: candidate %zu inspection failed\n",
                        capture->id, index);
                free_candidate_strings(candidates, index);
                pinyin_free_instance(instance);
                return false;
            }
            candidates[index] = g_strdup(candidate_string);
            if (candidates[index] == NULL) {
                fprintf(stderr, "%s: candidate %zu allocation failed\n",
                        capture->id, index);
                free_candidate_strings(candidates, index);
                pinyin_free_instance(instance);
                return false;
            }
        }
    }

    fputs("schema=" CAPTURE_SCHEMA "\tpin_ref=", stdout);
    print_escaped(oracle_pin_ref);
    fputs("\tfamily=", stdout);
    print_escaped(family);
    fputs("\tcase=", stdout);
    print_escaped(capture->id);
    fputs("\tapi_sequence=pinyin_set_options,pinyin_alloc_instance,"
          "pinyin_parse_more_full_pinyins,pinyin_get_parsed_input_length",
          stdout);
    if (parsed_length > 0) {
        fputs(",pinyin_guess_candidates", stdout);
    }
    if (candidates_available) {
        fputs(",pinyin_get_n_candidate", stdout);
    }
    if (captured_candidates > 0) {
        fputs(",pinyin_get_candidate,pinyin_get_candidate_string", stdout);
    }
    if (parsed_length > 0) {
        fputs(",pinyin_get_pinyin_key,pinyin_get_pinyin_key_rest,"
              "pinyin_get_pinyin_key_rest_positions,"
              "pinyin_get_pinyin_string,pinyin_get_pinyin_is_incomplete",
              stdout);
    }
    fputs(",pinyin_free_instance\tinput=", stdout);
    print_escaped(capture->input);
    fprintf(stdout,
            "\tflags=0x%08x\tparse_return=%zu\tparsed_input_length=%zu"
            "\tsegments=",
            (unsigned int)capture->flags, parse_return, parsed_length);

    for (size_t offset = 0; offset < parsed_length;) {
        ChewingKey *key = NULL;
        ChewingKeyRest *rest = NULL;
        gchar *pinyin = NULL;
        guint16 begin = 0;
        guint16 end = 0;

        if (!pinyin_get_pinyin_key(instance, offset, &key) || key == NULL) {
            ++offset;
            continue;
        }
        if (!pinyin_get_pinyin_key_rest(instance, offset, &rest) || rest == NULL) {
            if (!first_segment) {
                fputc(',', stdout);
            }
            fprintf(stdout, "<missing-rest>@%zu", offset);
            first_segment = false;
            ++offset;
            continue;
        }
        if (!pinyin_get_pinyin_key_rest_positions(instance, rest, &begin, &end)) {
            if (!first_segment) {
                fputc(',', stdout);
            }
            fprintf(stdout, "<missing-position>@%zu", offset);
            first_segment = false;
            ++offset;
            continue;
        }
        if (!pinyin_get_pinyin_string(instance, key, &pinyin) || pinyin == NULL) {
            if (!first_segment) {
                fputc(',', stdout);
            }
            fprintf(stdout, "<missing-pinyin>@%zu", offset);
            first_segment = false;
            ++offset;
            continue;
        }

        if (!first_segment) {
            fputc(',', stdout);
        }
        print_escaped(pinyin);
        fprintf(stdout, "@%u:%u:%s", (unsigned int)begin, (unsigned int)end,
                pinyin_get_pinyin_is_incomplete(instance, key) ? "partial"
                                                                : "complete");
        first_segment = false;
        g_free(pinyin);
        offset = end > offset ? end : offset + 1;
    }

    if (first_segment) {
        fputc('-', stdout);
    }
    fprintf(stdout, "\tcandidate_total=%u\tcandidates_hex=", candidate_total);
    if (captured_candidates == 0) {
        fputc('-', stdout);
    } else {
        for (size_t index = 0; index < captured_candidates; ++index) {
            if (index > 0) {
                fputc(',', stdout);
            }
            print_hex(candidates[index]);
        }
    }
    fputs("\tremainder=", stdout);
    print_escaped(capture->input + parsed_length);
    fputc('\n', stdout);

    free_candidate_strings(candidates, captured_candidates);
    pinyin_free_instance(instance);
    return true;
}

static bool capture_fa(pinyin_context_t *context) {
    static const struct capture_case cases[] = {
        {"valid-single", "ni", DEFAULT_FLAGS},
        {"valid-multiple", "nihao", DEFAULT_FLAGS},
        {"valid-phrase", "zhongguoren", DEFAULT_FLAGS},
        {"robustness-zhuan", "zhuan", DEFAULT_FLAGS},
        {"ambiguous-xian", "xian", DEFAULT_FLAGS},
        {"ambiguous-fangan", "fangan", DEFAULT_FLAGS},
        {"apostrophe-xi-an", "xi'an", DEFAULT_FLAGS},
        {"apostrophe-chang-an", "chang'an", DEFAULT_FLAGS},
        {"incomplete-nih", "nih", DEFAULT_FLAGS},
        {"incomplete-zhongg", "zhongg", DEFAULT_FLAGS},
        {"junk-prefix", "!ni", DEFAULT_FLAGS},
        {"junk-middle", "ni!hao", DEFAULT_FLAGS},
        {"junk-suffix", "ni!", DEFAULT_FLAGS},
        {"empty", "", DEFAULT_FLAGS},
    };
    char *long_input = malloc(4097);

    if (long_input == NULL) {
        fputs("failed to allocate very-long input\n", stderr);
        return false;
    }
    memset(long_input, '!', 4096);
    long_input[4096] = '\0';

    for (size_t index = 0; index < sizeof(cases) / sizeof(cases[0]); ++index) {
        if (!emit_case(context, "F-A", &cases[index])) {
            free(long_input);
            return false;
        }
    }

    const struct capture_case long_case = {
        "very-long-junk-4096", long_input, DEFAULT_FLAGS};
    bool result = emit_case(context, "F-A", &long_case);
    free(long_input);
    return result;
}

static bool capture_fc(pinyin_context_t *context) {
    static const struct option_case cases[] = {
        {"pinyin-incomplete", "nih", PINYIN_INCOMPLETE},
        {"use-tone", "ni3", USE_TONE},
        {"force-tone", "ni", FORCE_TONE},
        {"divided-table", "xian", USE_DIVIDED_TABLE},
        {"resplit-table", "fangan", USE_RESPLIT_TABLE},
        {"amb-c-ch", "cang", PINYIN_AMB_C_CH},
        {"amb-s-sh", "sang", PINYIN_AMB_S_SH},
        {"amb-z-zh", "zang", PINYIN_AMB_Z_ZH},
        {"amb-f-h", "fang", PINYIN_AMB_F_H},
        {"amb-g-k", "gang", PINYIN_AMB_G_K},
        {"amb-l-n", "lan", PINYIN_AMB_L_N},
        {"amb-l-r", "lan", PINYIN_AMB_L_R},
        {"amb-an-ang", "lan", PINYIN_AMB_AN_ANG},
        {"amb-en-eng", "sen", PINYIN_AMB_EN_ENG},
        {"amb-in-ing", "lin", PINYIN_AMB_IN_ING},
        {"correct-gn-ng", "zhegn", PINYIN_CORRECT_GN_NG},
        {"correct-mg-ng", "zhemg", PINYIN_CORRECT_MG_NG},
        {"correct-iou-iu", "liou", PINYIN_CORRECT_IOU_IU},
        {"correct-uei-ui", "shuei", PINYIN_CORRECT_UEI_UI},
        {"correct-uen-un", "luen", PINYIN_CORRECT_UEN_UN},
        {"correct-ue-ve", "lue", PINYIN_CORRECT_UE_VE},
        {"correct-v-u", "jv", PINYIN_CORRECT_V_U},
        {"correct-on-ong", "don", PINYIN_CORRECT_ON_ONG},
    };

    for (size_t index = 0; index < sizeof(cases) / sizeof(cases[0]); ++index) {
        char off_id[64];
        char on_id[64];
        int off_length = snprintf(off_id, sizeof(off_id), "%s-off", cases[index].id);
        int on_length = snprintf(on_id, sizeof(on_id), "%s-on", cases[index].id);
        if (off_length < 0 || (size_t)off_length >= sizeof(off_id) ||
            on_length < 0 || (size_t)on_length >= sizeof(on_id)) {
            fputs("F-C case identifier exceeds buffer\n", stderr);
            return false;
        }

        const struct capture_case baseline = {
            off_id, cases[index].input, BASE_FLAGS};
        const struct capture_case enabled = {
            on_id, cases[index].input, BASE_FLAGS | cases[index].flag};
        if (!emit_case(context, "F-C", &baseline) ||
            !emit_case(context, "F-C", &enabled)) {
            return false;
        }
    }
    return true;
}

int main(int argc, char **argv) {
    pinyin_context_t *context;
    bool result;

    if (argc != 5) {
        fprintf(stderr,
                "usage: %s F-A|F-C SYSTEM_DATA_DIR FRESH_USER_DIR PIN_REF\n",
                argv[0]);
        return 2;
    }
    if (argv[4][0] == '\0') {
        fputs("pin ref must not be empty\n", stderr);
        return 2;
    }
    oracle_pin_ref = argv[4];

    context = pinyin_init(argv[2], argv[3]);
    if (context == NULL) {
        fputs("pinyin_init failed\n", stderr);
        return 1;
    }

    if (strcmp(argv[1], "F-A") == 0) {
        result = capture_fa(context);
    } else if (strcmp(argv[1], "F-C") == 0) {
        result = capture_fc(context);
    } else {
        fprintf(stderr, "unknown fixture family: %s\n", argv[1]);
        result = false;
    }

    pinyin_fini(context);
    return result ? 0 : 1;
}
