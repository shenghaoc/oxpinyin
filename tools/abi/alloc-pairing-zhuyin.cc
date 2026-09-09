// alloc-pairing-zhuyin.cc — the C consumer half of the allocator-pairing
// gate for libzhuyin.so.15.
//
// The libpinyin driver beside this one (alloc-pairing-pinyin.cc) carries the
// full rationale: the register declares a deallocator per pointer-shaped
// slot, this TU releases each slot with exactly that deallocator, and
// tools/abi/check-alloc-pairing.sh runs it under LeakSanitizer with the same
// two properties — per-slot coverage, and a warm pass that keeps one-time
// library statics out of the report so a leak reported here is per-call.
//
// A separate translation unit rather than a second half of the pinyin one:
// pinyin.h and zhuyin.h each pull in their own novel_types.h/​*_custom2.h
// tuple and define the same typedefs, so the two ABIs cannot share a TU.
// That mirrors the crates, whose marshalling layers are duplicated by design.

#include <zhuyin.h>

#include <glib.h>

#include <cstdio>
#include <cstring>

#include <sanitizer/lsan_interface.h>

namespace {

struct Slot {
    const char *symbol;
    const char *name;
};

// clang-format off
const Slot kSlots[] = {
    {"zhuyin_init",                  "return"},
    {"zhuyin_alloc_instance",        "return"},
    {"zhuyin_begin_add_phrases",     "return"},
    {"zhuyin_get_candidate",         "candidate"},
    {"zhuyin_get_candidate_string",  "utf8_str"},
    {"zhuyin_get_zhuyin_key",        "key"},
    {"zhuyin_get_zhuyin_key_rest",   "key_rest"},
    {"zhuyin_get_sentence",          "sentence"},
    {"zhuyin_get_zhuyin_string",     "utf8_str"},
    {"zhuyin_get_pinyin_string",     "utf8_str"},
    {"zhuyin_token_get_phrase",      "utf8_str"},
    {"zhuyin_in_chewing_keyboard",   "symbols"},
};
// clang-format on

constexpr size_t kSlotCount = sizeof(kSlots) / sizeof(kSlots[0]);
bool g_hit[kSlotCount];

size_t slot_id(const char *symbol, const char *name) {
    for (size_t i = 0; i < kSlotCount; ++i) {
        if (std::strcmp(kSlots[i].symbol, symbol) == 0 &&
            std::strcmp(kSlots[i].name, name) == 0) {
            return i;
        }
    }
    std::fprintf(stderr, "fatal: no slot %s/%s in the driver table\n", symbol, name);
    std::fflush(stderr);
    return kSlotCount;
}

void mark(const char *symbol, const char *name, const void *ptr) {
    if (ptr == nullptr) {
        return;
    }
    const size_t id = slot_id(symbol, name);
    if (id < kSlotCount) {
        g_hit[id] = true;
    }
}

void release_g_free(const char *symbol, const char *name, gchar *ptr) {
    mark(symbol, name, ptr);
#ifdef OXPINYIN_ALLOC_PAIRING_LEAK
    (void)ptr;
#else
    g_free(ptr);
#endif
}

void release_g_strfreev(const char *symbol, const char *name, gchar **ptr) {
    mark(symbol, name, ptr);
#ifdef OXPINYIN_ALLOC_PAIRING_LEAK
    (void)ptr;
#else
    g_strfreev(ptr);
#endif
}

void exercise_candidates(zhuyin_instance_t *instance) {
    // The composition-anchored window, the shape the zhuyin frontend drives
    // (`zhuyin_guess_candidates_after_cursor(instance, 0)`).
    if (!zhuyin_guess_candidates_after_cursor(instance, 0)) {
        return;
    }
    guint num = 0;
    if (!zhuyin_get_n_candidate(instance, &num) || num == 0) {
        return;
    }
    lookup_candidate_t *candidate = nullptr;
    if (!zhuyin_get_candidate(instance, 0, &candidate)) {
        return;
    }
    mark("zhuyin_get_candidate", "candidate", candidate);

    const gchar *text = nullptr;
    if (zhuyin_get_candidate_string(instance, candidate, &text)) {
        mark("zhuyin_get_candidate_string", "utf8_str", text);
    }
}

void exercise_key_strings(zhuyin_instance_t *instance) {
    ChewingKey *key = nullptr;
    ChewingKeyRest *key_rest = nullptr;
    if (zhuyin_get_zhuyin_key(instance, 0, &key)) {
        mark("zhuyin_get_zhuyin_key", "key", key);
    }
    if (zhuyin_get_zhuyin_key_rest(instance, 0, &key_rest)) {
        mark("zhuyin_get_zhuyin_key_rest", "key_rest", key_rest);
    }
    if (key == nullptr) {
        return;
    }

    gchar *utf8 = nullptr;
    zhuyin_get_zhuyin_string(instance, key, &utf8);
    release_g_free("zhuyin_get_zhuyin_string", "utf8_str", utf8);

    utf8 = nullptr;
    zhuyin_get_pinyin_string(instance, key, &utf8);
    release_g_free("zhuyin_get_pinyin_string", "utf8_str", utf8);
}

void exercise_tokens(zhuyin_instance_t *instance) {
    GArray *tokens = g_array_new(FALSE, FALSE, sizeof(phrase_token_t));
    if (zhuyin_lookup_tokens(instance, "你好", tokens) && tokens->len > 0) {
        const phrase_token_t token = g_array_index(tokens, phrase_token_t, 0);
        guint len = 0;
        gchar *utf8 = nullptr;
        zhuyin_token_get_phrase(instance, token, &len, &utf8);
        release_g_free("zhuyin_token_get_phrase", "utf8_str", utf8);
    }
    g_array_free(tokens, TRUE);
}

void exercise_chewing_keyboard(zhuyin_instance_t *instance) {
    for (char key = 0x21; key < 0x7f; ++key) {
        gchar **symbols = nullptr;
        if (zhuyin_in_chewing_keyboard(instance, key, &symbols)) {
            release_g_strfreev("zhuyin_in_chewing_keyboard", "symbols", symbols);
            return;
        }
        release_g_strfreev("zhuyin_in_chewing_keyboard", "symbols", symbols);
    }
}

void exercise_import_iterator(zhuyin_context_t *context) {
    import_iterator_t *iter = zhuyin_begin_add_phrases(context, USER_DICTIONARY);
    mark("zhuyin_begin_add_phrases", "return", iter);
    if (iter != nullptr) {
        zhuyin_iterator_add_phrase(iter, "你好", "ni'hao", 3);
        zhuyin_end_add_phrases(iter);
    }
}

int lifecycle(const char *systemdir, const char *userdir) {
    zhuyin_context_t *context = zhuyin_init(systemdir, userdir);
    if (context == nullptr) {
        return 1;
    }
    mark("zhuyin_init", "return", context);

    zhuyin_instance_t *instance = zhuyin_alloc_instance(context);
    if (instance == nullptr) {
        zhuyin_fini(context);
        return 2;
    }
    mark("zhuyin_alloc_instance", "return", instance);

    // Standard-scheme chewing keystrokes for 你好 — the zhuyin facade's own
    // fixture input. `zhuyin_parse_more_full_pinyins` parses into the same
    // matrix but the committed mini fixture offers no rows for it, which
    // would leave every instance-side slot unreached.
    zhuyin_parse_more_chewings(instance, "su3cl3");
    zhuyin_guess_sentence(instance);

    exercise_candidates(instance);
    exercise_key_strings(instance);
    exercise_tokens(instance);
    exercise_chewing_keyboard(instance);

    gchar *sentence = nullptr;
    zhuyin_get_sentence(instance, &sentence);
    release_g_free("zhuyin_get_sentence", "sentence", sentence);

    zhuyin_train(instance);
    exercise_import_iterator(context);

    // The `false-nulls` half of the sentence contract: after a reset there
    // is nothing to decode, so the call answers false with a NULL out-param
    // and the unconditional g_free below is the documented no-op.
    zhuyin_reset(instance);
    sentence = nullptr;
    zhuyin_get_sentence(instance, &sentence);
    release_g_free("zhuyin_get_sentence", "sentence", sentence);

    zhuyin_free_instance(instance);
    zhuyin_fini(context);
    return 0;
}

}  // namespace

int main(int argc, char **argv) {
    if (argc < 3) {
        std::fprintf(stderr, "usage: %s <systemdir> <userdir> [--coverage]\n", argv[0]);
        return 64;
    }
    const bool coverage = argc > 3 && std::strcmp(argv[3], "--coverage") == 0;

    __lsan_disable();
    const int warm = lifecycle(argv[1], argv[2]);
    __lsan_enable();
    if (warm != 0) {
        std::fprintf(stderr, "fatal: warm-up lifecycle failed (%d)\n", warm);
        return warm;
    }

    const int rc = lifecycle(argv[1], argv[2]);
    if (rc != 0) {
        std::fprintf(stderr, "fatal: measured lifecycle failed (%d)\n", rc);
        return rc;
    }

    if (coverage) {
        for (size_t i = 0; i < kSlotCount; ++i) {
            std::printf("SLOT %s %s %s\n", kSlots[i].symbol, kSlots[i].name,
                        g_hit[i] ? "hit" : "miss");
        }
        std::fflush(stdout);
    }

    if (__lsan_do_recoverable_leak_check() != 0) {
        std::fprintf(stderr, "FAIL: LeakSanitizer reported a per-call leak\n");
        return 10;
    }
    return 0;
}
