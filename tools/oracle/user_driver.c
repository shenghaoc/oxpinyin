/* user_driver.c — pin-side driver for the user-dir round trip.
 *
 * Modes:
 *   train <system> <user> <input>...   parse+guess+train each input
 *                                      (sentence n-best index 0), then save
 *   dump  <system> <user>              print the §9 exports, one line each:
 *                                      "P\t<phrase>\t<pinyin>\t<count>"
 *                                      "B\t<phrase>\t<pinyin>\t<count>"
 *                                      (the bigram count is the stored
 *                                      count × 2 — the pin's rendering)
 *
 * Built and driven by tools/oracle/user-dir-round-trip.sh against the
 * pin-built prefix (tools/oracle/build-oracle.sh).
 *
 * This file is part of oxpinyin, GPL-3.0-or-later like the rest of it.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <glib.h>
#include "pinyin.h"

static int train(const char *system_dir, const char *user_dir,
                 char **inputs, int n_inputs) {
    pinyin_context_t *context = pinyin_init(system_dir, user_dir);
    if (!context)
        return 1;

    pinyin_instance_t *instance = pinyin_alloc_instance(context);
    for (int i = 0; i < n_inputs; ++i) {
        if ((int)strlen(inputs[i]) !=
            pinyin_parse_more_full_pinyins(instance, inputs[i])) {
            fprintf(stderr, "parse failed: %s\n", inputs[i]);
            return 1;
        }
        /* The ibus shape, §6: training follows a selection — an
         * unconstrained train_result3 walks an empty constraint set and
         * trains nothing. Choose candidate 0 (a sentence candidate),
         * then train the constrained n-best. */
        if (!pinyin_guess_candidates(instance, 0, 0)) {
            fprintf(stderr, "guess candidates failed: %s\n", inputs[i]);
            return 1;
        }
        lookup_candidate_t *candidate = NULL;
        if (!pinyin_get_candidate(instance, 0, &candidate) || !candidate) {
            fprintf(stderr, "no candidate: %s\n", inputs[i]);
            return 1;
        }
        if (pinyin_choose_candidate(instance, 0, candidate) < 1) {
            fprintf(stderr, "choose failed: %s\n", inputs[i]);
            return 1;
        }
        if (!pinyin_guess_sentence(instance)) {
            fprintf(stderr, "guess failed: %s\n", inputs[i]);
            return 1;
        }
        if (!pinyin_train(instance, 0)) {
            fprintf(stderr, "train failed: %s\n", inputs[i]);
            return 1;
        }
        /* ibus's `remember-every-input` path (§6): the committed
         * sentence is also added as a user phrase, which is what writes
         * user.bin and the two index trees. Without it the profile would
         * be phrase-free and those files untested. */
        char *sentence = NULL;
        if (pinyin_get_sentence(instance, 0, &sentence) && sentence) {
            if (!pinyin_remember_user_input(instance, sentence, -1))
                fprintf(stderr, "remember failed: %s (%s)\n", inputs[i],
                        sentence);
            g_free(sentence);
        }
    }
    pinyin_free_instance(instance);

    if (!pinyin_save(context)) {
        fprintf(stderr, "save failed\n");
        return 1;
    }
    pinyin_fini(context);
    return 0;
}

static int dump_phrases(pinyin_context_t *context) {
    export_iterator_t *iter = pinyin_begin_get_phrases(context, 7);
    if (!iter)
        return 1;
    while (pinyin_iterator_has_next_phrase(iter)) {
        gchar *phrase = NULL, *pinyin = NULL;
        gint count = -1;
        /* A failed step is an unrenderable row, not a walk error: the
         * iterator also reports false when it cannot render; stop as
         * the frontend's export does. */
        if (!pinyin_iterator_get_next_phrase(iter, &phrase, &pinyin,
                                             &count))
            break;
        printf("P\t%s\t%s\t%d\n", phrase, pinyin, count);
        g_free(phrase);
        g_free(pinyin);
    }
    pinyin_end_get_phrases(iter);
    return 0;
}

static int dump_bigrams(pinyin_context_t *context) {
    bigram_export_iterator_t *iter = pinyin_begin_get_bigram_phrases(context);
    if (!iter)
        return 1;
    while (pinyin_bigram_iterator_has_next_phrase(iter)) {
        gchar *phrase = NULL, *pinyin = NULL;
        gint count = -1;
        if (!pinyin_bigram_iterator_get_next_phrase(iter, &phrase, &pinyin,
                                                    &count))
            break;
        printf("B\t%s\t%s\t%d\n", phrase, pinyin, count);
        g_free(phrase);
        g_free(pinyin);
    }
    pinyin_end_get_bigram_phrases(iter);
    return 0;
}

static int dump(const char *system_dir, const char *user_dir) {
    pinyin_context_t *context = pinyin_init(system_dir, user_dir);
    if (!context)
        return 1;
    if (dump_phrases(context) || dump_bigrams(context))
        return 1;
    pinyin_fini(context);
    return 0;
}

int main(int argc, char **argv) {
    if (argc < 4) {
        fprintf(stderr,
                "usage: %s train|dump <system-dir> <user-dir> [input...]\n",
                argv[0]);
        return 2;
    }
    if (strcmp(argv[1], "train") == 0)
        return train(argv[2], argv[3], argv + 4, argc - 4);
    if (strcmp(argv[1], "dump") == 0)
        return dump(argv[2], argv[3]);
    fprintf(stderr, "unknown mode: %s\n", argv[1]);
    return 2;
}
