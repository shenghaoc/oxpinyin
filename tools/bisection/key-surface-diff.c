/*
 * key-surface-diff.c — Tier-A ABI parity: single-key parsing and the
 * ChewingKey display getters (`crates/oxpinyin-capi/src/keys.rs`).
 *
 * Drives pinyin_parse_full_pinyin / pinyin_parse_double_pinyin /
 * pinyin_parse_chewing over option profiles and scheme tables, logging
 * retval plus the RAW two-byte key — the byte identity of the packed
 * bitfield is the D1 layout verification against the pin — then renders
 * every display getter over a fixed key set, and closes with the
 * pinyin_get_context and addon-unload contracts. The runner executes
 * this binary once per engine (libpinyin_capi.so, the pin-built
 * libpinyin.so) and diffs the logs.
 *
 * Layout self-check: this TU carries its own mirror of the upstream
 * `_ChewingKey` / `_ChewingKeyRest` declarations (chewing_key.h:41-48,
 * :100-104) and static-asserts their sizes; the per-probe two-byte key
 * comparison is the cross-engine check.
 *
 * Usage: ./key-surface-diff <path-to-so> <systemdir>
 */

#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef void pinyin_context_t;
typedef void pinyin_instance_t;
typedef char gchar;

/* Upstream chewing_key.h bitfield mirror. */
typedef struct ChewingKey {
    uint16_t m_initial :5;
    uint16_t m_middle :2;
    uint16_t m_final :5;
    uint16_t m_tone :3;
    uint16_t m_zero_padding :1;
} ChewingKey;

typedef struct ChewingKeyRest {
    uint16_t m_raw_begin;
    uint16_t m_raw_end;
} ChewingKeyRest;

_Static_assert(sizeof(ChewingKey) == 2, "packed chewing key must be 2 bytes");
_Static_assert(sizeof(ChewingKeyRest) == 4, "key rest must be 4 bytes");

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef pinyin_instance_t *(*fn_alloc)(pinyin_context_t *);
typedef void (*fn_free_inst)(pinyin_instance_t *);
typedef bool (*fn_set_options)(pinyin_context_t *, uint32_t);
typedef bool (*fn_set_double)(pinyin_context_t *, int);
typedef bool (*fn_set_zhuyin)(pinyin_context_t *, int);
typedef bool (*fn_parse_full)(pinyin_instance_t *, const char *, ChewingKey *);
typedef bool (*fn_parse_double)(pinyin_instance_t *, const char *, ChewingKey *);
typedef bool (*fn_parse_chewing)(pinyin_instance_t *, const char *, ChewingKey *);
typedef bool (*fn_get_zhuyin)(pinyin_instance_t *, ChewingKey *, gchar **);
typedef bool (*fn_get_pinyin)(pinyin_instance_t *, ChewingKey *, gchar **);
typedef bool (*fn_get_luoma)(pinyin_instance_t *, ChewingKey *, gchar **);
typedef bool (*fn_get_secondary)(pinyin_instance_t *, ChewingKey *, gchar **);
typedef bool (*fn_get_strings)(pinyin_instance_t *, ChewingKey *, gchar **, gchar **);
typedef bool (*fn_is_incomplete)(pinyin_instance_t *, ChewingKey *);
typedef pinyin_context_t *(*fn_get_context)(pinyin_instance_t *);
typedef bool (*fn_unload_addon)(pinyin_context_t *, uint8_t);

static void *must(void *handle, const char *name) {
    void *s = dlsym(handle, name);
    if (!s) {
        fprintf(stderr, "MISSING %s\n", name);
        exit(1);
    }
    return s;
}

static void key_hex(const ChewingKey *key, char out[5]) {
    uint16_t bits = 0;
    memcpy(&bits, key, sizeof(bits));
    snprintf(out, 5, "%04x", bits);
}

static pinyin_context_t *g_context;

static struct {
    fn_set_options set_options;
    fn_set_double set_double;
    fn_set_zhuyin set_zhuyin;
    fn_parse_full parse_full;
    fn_parse_double parse_double;
    fn_parse_chewing parse_chewing;
    fn_get_zhuyin get_zhuyin;
    fn_get_pinyin get_pinyin;
    fn_get_luoma get_luoma;
    fn_get_secondary get_secondary;
    fn_get_strings get_strings;
    fn_is_incomplete is_incomplete;
    pinyin_instance_t *inst;
} api;

/* One full-pinyin probe: retval plus the raw key bytes. The key is
 * pre-filled with 0xAB bytes so "failed parse" behavior is visible
 * (the pin zeroes it; the double/chewing entries leave it). */
static void probe_full(const char *word, const char *input) {
    ChewingKey key;
    memset(&key, 0xAB, sizeof(key));
    bool ok = api.parse_full(api.inst, input, &key);
    char hex[5];
    key_hex(&key, hex);
    printf("full|%s|%s|%d|%s\n", word, input, ok, hex);
}

static void probe_double(int scheme, const char *word, const char *input) {
    api.set_double(g_context, scheme);
    ChewingKey key;
    memset(&key, 0xAB, sizeof(key));
    bool ok = api.parse_double(api.inst, input, &key);
    char hex[5];
    key_hex(&key, hex);
    printf("double|%d|%s|%s|%d|%s\n", scheme, word, input, ok, hex);
}

static void probe_chewing(int scheme, const char *word, const char *input) {
    api.set_zhuyin(g_context, scheme);
    ChewingKey key;
    memset(&key, 0xAB, sizeof(key));
    bool ok = api.parse_chewing(api.inst, input, &key);
    char hex[5];
    key_hex(&key, hex);
    printf("chewing|%d|%s|%s|%d|%s\n", scheme, word, input, ok, hex);
}

/* Renders all six display strings plus is_incomplete over one key. */
static void render_probe(const char *family, const char *input,
                         const ChewingKey *key) {
    ChewingKey mutable_key = *key;
    gchar *out = NULL;
    char hex[5];
    key_hex(key, hex);

    out = NULL;
    bool ok = api.get_zhuyin(api.inst, &mutable_key, &out);
    printf("render|%s|%s|zhuyin|%d|%s\n", family, input, ok, out ? out : "-");
    out = NULL;
    ok = api.get_pinyin(api.inst, &mutable_key, &out);
    printf("render|%s|%s|pinyin|%d|%s\n", family, input, ok, out ? out : "-");
    out = NULL;
    ok = api.get_luoma(api.inst, &mutable_key, &out);
    printf("render|%s|%s|luoma|%d|%s\n", family, input, ok, out ? out : "-");
    out = NULL;
    ok = api.get_secondary(api.inst, &mutable_key, &out);
    printf("render|%s|%s|secondary|%d|%s\n", family, input, ok, out ? out : "-");

    gchar *shengmu = NULL;
    gchar *yunmu = NULL;
    ok = api.get_strings(api.inst, &mutable_key, &shengmu, &yunmu);
    printf("render|%s|%s|shengmu|%d|%s\n", family, input, ok, shengmu ? shengmu : "-");
    printf("render|%s|%s|yunmu|%d|%s\n", family, input, ok, yunmu ? yunmu : "-");
    /* NULL out-params are the pin's skip case: success must hold. */
    ok = api.get_strings(api.inst, &mutable_key, &shengmu, NULL);
    printf("render|%s|%s|shengmu_skip|%d|%s\n", family, input, ok,
           shengmu ? shengmu : "-");

    printf("render|%s|%s|incomplete|%d\n", family, input,
           api.is_incomplete(api.inst, &mutable_key) ? 1 : 0);
}

int main(int argc, char **argv) {
    if (argc != 3) {
        fprintf(stderr, "Usage: %s <so> <systemdir>\n", argv[0]);
        return 1;
    }
    void *h = dlopen(argv[1], RTLD_NOW);
    if (!h) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }
    const char *systemdir = argv[2];

    fn_init init = (fn_init)dlsym(h, "oxpinyin_init_for_fixtures");
    if (!init)
        init = (fn_init)must(h, "pinyin_init");
    fn_fini fini = (fn_fini)must(h, "pinyin_fini");
    fn_alloc alloc = (fn_alloc)must(h, "pinyin_alloc_instance");
    fn_free_inst free_inst = (fn_free_inst)must(h, "pinyin_free_instance");
    fn_get_context get_context = (fn_get_context)must(h, "pinyin_get_context");
    fn_unload_addon unload_addon =
        (fn_unload_addon)must(h, "pinyin_unload_addon_phrase_library");

    api.set_options = (fn_set_options)must(h, "pinyin_set_options");
    api.set_double = (fn_set_double)must(h, "pinyin_set_double_pinyin_scheme");
    api.set_zhuyin = (fn_set_zhuyin)must(h, "pinyin_set_zhuyin_scheme");
    api.parse_full = (fn_parse_full)must(h, "pinyin_parse_full_pinyin");
    api.parse_double = (fn_parse_double)must(h, "pinyin_parse_double_pinyin");
    api.parse_chewing = (fn_parse_chewing)must(h, "pinyin_parse_chewing");
    api.get_zhuyin = (fn_get_zhuyin)must(h, "pinyin_get_zhuyin_string");
    api.get_pinyin = (fn_get_pinyin)must(h, "pinyin_get_pinyin_string");
    api.get_luoma = (fn_get_luoma)must(h, "pinyin_get_luoma_pinyin_string");
    api.get_secondary =
        (fn_get_secondary)must(h, "pinyin_get_secondary_zhuyin_string");
    api.get_strings = (fn_get_strings)must(h, "pinyin_get_pinyin_strings");
    api.is_incomplete =
        (fn_is_incomplete)must(h, "pinyin_get_pinyin_is_incomplete");

    g_context = init(systemdir, "");
    if (!g_context) {
        fprintf(stderr, "pinyin_init failed\n");
        return 1;
    }
    api.inst = alloc(g_context);
    if (!api.inst) {
        fprintf(stderr, "pinyin_alloc_instance failed\n");
        return 1;
    }

    /* layout: the driver-side mirror sizes. */
    printf("layout|sizeof_key=2|sizeof_rest=4\n");

    /* Option profiles: the parity word 0x18a, plus USE_TONE (0x1aa),
     * USE_TONE|FORCE_TONE (0x1ea), and FORCE_TONE alone (0x1ca). */
    const char *full_inputs[] = {
        "ni", "hao", "shi", "jie", "zai", "zhuang", "fangan", "yi", "wu",
        "lv", "b",   "z",   "zh",  "n",   "agn",    "amg",    "jv", "lue",
        "nue", "ni3", "zai4", "zhuang4", "ni6", "zai0", "nihao",
        /* No "ni'hao": the pin ASSERTS on apostrophes inside
         * parse_one_key (pinyin_parser2.cpp:170) and the oracle run
         * would abort; the no-abort `false` is recorded as a
         * divergence in upstream-divergences.md. */
        "nih", "ni hao", "qqq", "x", "", "NI", "nv4",
    };
    const char *words[] = {"0x18a", "0x1aa", "0x1ea", "0x1ca"};
    for (unsigned w = 0; w < sizeof(words) / sizeof(words[0]); ++w) {
        if (!api.set_options(g_context, (uint32_t)strtoul(words[w], NULL, 16))) {
            printf("options|%s|refused\n", words[w]);
            continue;
        }
        for (unsigned i = 0; i < sizeof(full_inputs) / sizeof(full_inputs[0]);
             ++i) {
            probe_full(words[w], full_inputs[i]);
        }
    }

    /* Double pinyin: the six live schemes (30 aborts the pin), two- and
     * three-key probes plus incomplete and garbage. */
    const char *double_inputs[] = {"ni", "ha", "oo", "zhu", "zl",
                                   "ni3", "ha4", "zhuang", "z", "a",
                                   "1a", "xyz", "ni9"};
    for (int scheme = 1; scheme <= 6; ++scheme) {
        for (unsigned w = 0; w < sizeof(words) / sizeof(words[0]); ++w) {
            if (!api.set_options(g_context, (uint32_t)strtoul(words[w], NULL, 16))) {
                continue;
            }
            for (unsigned i = 0; i < sizeof(double_inputs) / sizeof(double_inputs[0]);
                 ++i) {
                probe_double(scheme, words[w], double_inputs[i]);
            }
        }
    }

    /* Chewing: the eight live keyboards (7 aborts the pin), a full
     * single-byte sweep — every keystroke is a one-key probe either
     * engine must agree on — plus STANDARD two- and three-key shapes. */
    const char *chew_inputs[] = {
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m",
        "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z",
        "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", ",", "-", ".",
        "/", ";",
        "18", "183", "1834", "11", "88", "1j", "1j3",
    };
    const int chew_schemes[] = {1, 2, 3, 4, 5, 6, 8, 9};
    for (unsigned s = 0; s < sizeof(chew_schemes) / sizeof(chew_schemes[0]);
         ++s) {
        for (unsigned w = 0; w < sizeof(words) / sizeof(words[0]); ++w) {
            if (!api.set_options(g_context, (uint32_t)strtoul(words[w], NULL, 16))) {
                continue;
            }
            for (unsigned i = 0; i < sizeof(chew_inputs) / sizeof(chew_inputs[0]);
                 ++i) {
                probe_chewing(chew_schemes[s], words[w], chew_inputs[i]);
            }
        }
    }

    /* Display getters over a fixed key set, parsed fresh per key. */
    api.set_options(g_context, 0x18a);
    const char *render_inputs[] = {"ni",   "zhang", "ba",   "b",  "zhong",
                                   "zi",   "ni3",   "hao",  "sh", "e",
                                   "nue",  "jv"};
    for (unsigned i = 0; i < sizeof(render_inputs) / sizeof(render_inputs[0]);
         ++i) {
        ChewingKey key;
        memset(&key, 0, sizeof(key));
        if (api.parse_full(api.inst, render_inputs[i], &key)) {
            render_probe("full", render_inputs[i], &key);
        } else {
            printf("render|full|%s|unparsed\n", render_inputs[i]);
        }
    }

    /* Chewed keys render through the same getters (scheme 1). */
    api.set_zhuyin(g_context, 1);
    const char *chew_render[] = {"18", "183", "1j", "1j0"};
    for (unsigned w = 0; w < sizeof(words) / sizeof(words[0]); ++w) {
        if (!api.set_options(g_context, (uint32_t)strtoul(words[w], NULL, 16))) {
            continue;
        }
        for (unsigned i = 0; i < sizeof(chew_render) / sizeof(chew_render[0]);
             ++i) {
            ChewingKey key;
            memset(&key, 0, sizeof(key));
            if (api.parse_chewing(api.inst, chew_render[i], &key)) {
                render_probe("chewing", chew_render[i], &key);
            } else {
                printf("render|chewing|%s|%s|unparsed\n", words[w],
                       chew_render[i]);
            }
        }
    }

    /* The zero key through every getter: the string getters NULL the
     * out-param and answer false; get_pinyin_strings' guard must leave
     * its sentinel-prefilled out-params UNTOUCHED; is_incomplete
     * answers true (zero middle, zero final). Writing through the
     * sentinels would crash here, which is the check. */
    {
        ChewingKey zero;
        memset(&zero, 0, sizeof(zero));
        gchar *out = NULL;
        bool ok = api.get_zhuyin(api.inst, &zero, &out);
        printf("zero|zhuyin|%d|%s\n", ok, out ? out : "-");
        /* A guard violation would write through 0x1/0x2 and crash
         * before this line — the absence of a crash IS the check. */
        ok = api.get_strings(api.inst, &zero, (gchar **)0x1,
                                    (gchar **)0x2);
        printf("zero|strings|%d\n", ok ? 1 : 0);
        printf("zero|incomplete|%d\n",
               api.is_incomplete(api.inst, &zero) ? 1 : 0);
    }

    /* context + addon unload contracts. */
    api.set_options(g_context, 0x18a);
    printf("context|match|%d\n", get_context(api.inst) == g_context ? 1 : 0);
    /* No null-instance probe here: the pin has no null guards
     * (pinyin_get_context dereferences, pinyin.cpp:1358-1360) — the
     * null-to-sentinel contract is oxpinyin-side and pinned by the
     * Rust ABI tests. */
    /* In-range indexes only: the pin ASSERTS on 16+ (pinyin.cpp:499) —
     * the no-abort `false` for out-of-range is pinned by the Rust ABI
     * test (unload_addon_contract), not differentialable. */
    const int addon_indexes[] = {0, 5, 15};
    for (unsigned i = 0; i < sizeof(addon_indexes) / sizeof(addon_indexes[0]);
         ++i) {
        printf("unload_addon|%d|%d\n", addon_indexes[i],
               unload_addon(g_context, (uint8_t)addon_indexes[i]) ? 1 : 0);
    }

    free_inst(api.inst);
    fini(g_context);
    return 0;
}
