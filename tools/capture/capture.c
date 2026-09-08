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

/* ── ZH: the zhuyin parse battery (tests/test_zhuyin.cpp input class) ── */

struct zhuyin_case {
    const char *id;
    const char *input;
    ZhuyinScheme scheme;
    pinyin_option_t flags;
};

/*
 * Drives one zhuyin keystroke line through the pinyin facade's chewing
 * batch entry — the same seam oxpinyin's facade replays through
 * ZhuyinParser (use_tone and allow_incomplete read off the option word,
 * force_tone never set on this facade). Per key, both renderings are
 * captured: the pinyin string (tone digit appended by the oracle for a
 * non-zero tone) and the zhuyin string (tone mark for tones 2..5), plus
 * the keystroke span. The upstream test_zhuyin.cpp battery is
 * stdin-line driven with no committed corpus, so the inputs here are
 * authored in its shape: valid syllables with and without tone keys,
 * tone-first junk, in-keyboard junk, empty, and a very-long line.
 */
static bool emit_zhuyin_case(pinyin_context_t *context,
                             const struct zhuyin_case *zc) {
    pinyin_instance_t *instance;
    size_t parse_return;
    size_t parsed_length;
    size_t input_length = strlen(zc->input);
    bool first_segment = true;
    bool first_zhuyin = true;

    if ((zc->flags & DYNAMIC_ADJUST) != 0) {
        fprintf(stderr, "%s: dynamic adjustment must remain disabled\n",
                zc->id);
        return false;
    }
    if (!pinyin_set_options(context, zc->flags)) {
        fprintf(stderr, "%s: pinyin_set_options failed\n", zc->id);
        return false;
    }
    if (!pinyin_set_zhuyin_scheme(context, zc->scheme)) {
        fprintf(stderr, "%s: pinyin_set_zhuyin_scheme failed\n", zc->id);
        return false;
    }

    instance = pinyin_alloc_instance(context);
    if (instance == NULL) {
        fprintf(stderr, "%s: pinyin_alloc_instance failed\n", zc->id);
        return false;
    }

    parse_return = pinyin_parse_more_chewings(instance, zc->input);
    parsed_length = pinyin_get_parsed_input_length(instance);
    if (parsed_length > input_length) {
        fprintf(stderr, "%s: parsed length exceeds input length\n", zc->id);
        pinyin_free_instance(instance);
        return false;
    }

    fputs("schema=" CAPTURE_SCHEMA "\tpin_ref=", stdout);
    print_escaped(oracle_pin_ref);
    fputs("\tfamily=ZH\tcase=", stdout);
    print_escaped(zc->id);
    fputs("\tapi_sequence=pinyin_set_options,pinyin_set_zhuyin_scheme,"
          "pinyin_alloc_instance,pinyin_parse_more_chewings,"
          "pinyin_get_parsed_input_length",
          stdout);
    if (parsed_length > 0) {
        fputs(",pinyin_get_pinyin_key,pinyin_get_pinyin_key_rest,"
              "pinyin_get_pinyin_key_rest_positions,"
              "pinyin_get_pinyin_string,pinyin_get_zhuyin_string",
              stdout);
    }
    fputs(",pinyin_free_instance\tinput=", stdout);
    print_escaped(zc->input);
    fprintf(stdout,
            "\tflags=0x%08x\tscheme=%d\tparse_return=%zu"
            "\tparsed_input_length=%zu\tsegments=",
            (unsigned int)zc->flags, (int)zc->scheme, parse_return,
            parsed_length);

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
        if (!pinyin_get_pinyin_key_rest(instance, offset, &rest) ||
            rest == NULL) {
            if (!first_segment) {
                fputc(',', stdout);
            }
            fprintf(stdout, "<missing-rest>@%zu", offset);
            first_segment = false;
            ++offset;
            continue;
        }
        if (!pinyin_get_pinyin_key_rest_positions(instance, rest, &begin,
                                                  &end)) {
            if (!first_segment) {
                fputc(',', stdout);
            }
            fprintf(stdout, "<missing-position>@%zu", offset);
            first_segment = false;
            ++offset;
            continue;
        }
        if (!pinyin_get_pinyin_string(instance, key, &pinyin) ||
            pinyin == NULL) {
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
        fprintf(stdout, "@%u:%u", (unsigned int)begin, (unsigned int)end);
        first_segment = false;
        g_free(pinyin);
        offset = end > offset ? end : offset + 1;
    }

    if (first_segment) {
        fputc('-', stdout);
    }

    /* The zhuyin renderings, one per parsed key, same walk order. */
    fputs("\tzhuyin=", stdout);
    for (size_t offset = 0; offset < parsed_length;) {
        ChewingKey *key = NULL;
        ChewingKeyRest *rest = NULL;
        gchar *zhuyin = NULL;
        guint16 begin = 0;
        guint16 end = 0;

        if (!pinyin_get_pinyin_key(instance, offset, &key) || key == NULL) {
            ++offset;
            continue;
        }
        if (!pinyin_get_pinyin_key_rest(instance, offset, &rest) ||
            rest == NULL ||
            !pinyin_get_pinyin_key_rest_positions(instance, rest, &begin,
                                                  &end)) {
            ++offset;
            continue;
        }
        if (!pinyin_get_zhuyin_string(instance, key, &zhuyin) ||
            zhuyin == NULL) {
            if (!first_zhuyin) {
                fputc(',', stdout);
            }
            fputs("<missing-zhuyin>", stdout);
            first_zhuyin = false;
            ++offset;
            continue;
        }

        if (!first_zhuyin) {
            fputc(',', stdout);
        }
        print_escaped(zhuyin);
        first_zhuyin = false;
        g_free(zhuyin);
        offset = end > offset ? end : offset + 1;
    }
    if (first_zhuyin) {
        fputc('-', stdout);
    }

    fputs("\tremainder=", stdout);
    print_escaped(zc->input + parsed_length);
    fputc('\n', stdout);

    pinyin_free_instance(instance);
    return true;
}

static bool capture_zh(pinyin_context_t *context) {
    /* The authored battery: standard-layout keystrokes first at the
     * three option words that reach the parser (plain, USE_TONE,
     * ZHUYIN_INCOMPLETE), then every other parseable keyboard on the
     * same physical keystrokes, then the robustness tail. ZHUYIN_
     * STANDARD_DVORAK (7) is upstream's setter abort and is excluded
     * on purpose. */
    static const struct zhuyin_case cases[] = {
        {"std-nihao", "su3cl3", ZHUYIN_STANDARD, DEFAULT_FLAGS},
        {"std-nihao-tone", "su3cl3", ZHUYIN_STANDARD,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"std-nihao-incomplete", "su3cl3", ZHUYIN_STANDARD,
         (pinyin_option_t)(DEFAULT_FLAGS | ZHUYIN_INCOMPLETE)},
        {"std-zhongguo", "5j/ej86", ZHUYIN_STANDARD,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"std-single", "su3", ZHUYIN_STANDARD,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"std-zero-tone", "sucl", ZHUYIN_STANDARD,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"std-tone-first", "3su3cl3", ZHUYIN_STANDARD,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"std-bare-tone", "3", ZHUYIN_STANDARD,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"std-junk-prefix", "!su3cl3", ZHUYIN_STANDARD,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"std-junk-middle", "su3!cl3", ZHUYIN_STANDARD,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"std-empty", "", ZHUYIN_STANDARD,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"hsu-nihao", "su3cl3", ZHUYIN_HSU,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"hsu-junk", "!su3cl3", ZHUYIN_HSU,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"ibm-nihao", "su3cl3", ZHUYIN_IBM,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"ibm-junk", "!su3cl3", ZHUYIN_IBM,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"ginyieh-nihao", "su3cl3", ZHUYIN_GINYIEH,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"ginyieh-junk", "!su3cl3", ZHUYIN_GINYIEH,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"eten-nihao", "su3cl3", ZHUYIN_ETEN,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"eten-junk", "!su3cl3", ZHUYIN_ETEN,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"eten26-nihao", "su3cl3", ZHUYIN_ETEN26,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"eten26-junk", "!su3cl3", ZHUYIN_ETEN26,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"hsu-dvorak-nihao", "su3cl3", ZHUYIN_HSU_DVORAK,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"hsu-dvorak-junk", "!su3cl3", ZHUYIN_HSU_DVORAK,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"dachen-cp26-nihao", "su3cl3", ZHUYIN_DACHEN_CP26,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
        {"dachen-cp26-junk", "!su3cl3", ZHUYIN_DACHEN_CP26,
         (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)},
    };
    char *long_input = malloc(4097);

    if (long_input == NULL) {
        fputs("failed to allocate very-long input\n", stderr);
        return false;
    }
    memset(long_input, '!', 4096);
    long_input[4096] = '\0';

    for (size_t index = 0; index < sizeof(cases) / sizeof(cases[0]);
         ++index) {
        if (!emit_zhuyin_case(context, &cases[index])) {
            free(long_input);
            return false;
        }
    }

    const struct zhuyin_case long_case = {
        "std-very-long-junk-4096", long_input, ZHUYIN_STANDARD,
        (pinyin_option_t)(DEFAULT_FLAGS | USE_TONE)};
    bool result = emit_zhuyin_case(context, &long_case);
    free(long_input);
    return result;
}

int main(int argc, char **argv) {
    pinyin_context_t *context;
    bool result;

    if (argc != 5) {
        fprintf(stderr,
                "usage: %s F-A|F-C|ZH SYSTEM_DATA_DIR FRESH_USER_DIR PIN_REF\n",
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
    } else if (strcmp(argv[1], "ZH") == 0) {
        result = capture_zh(context);
    } else {
        fprintf(stderr, "unknown fixture family: %s\n", argv[1]);
        result = false;
    }

    pinyin_fini(context);
    return result ? 0 : 1;
}
