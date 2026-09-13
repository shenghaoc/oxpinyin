/* user_driver.c — pin-side driver for the user-dir round trip.
 *
 * Modes:
 *   train    <system> <user> <input>... parse, guess, select, train and
 *                                      remember each input the way ibus
 *                                      drives the library, then save
 *   dump     <system> <user>            print the §9 exports, one line each:
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

/* ibus's default candidate order (`PYPConfig.cc:151`): phrase length,
 * then pinyin length, then frequency. Passing 0 — as this driver used
 * to — asks for no ordering at all, which leaves "the first NORMAL
 * candidate" an arbitrary rare single character (`疒` for `nihao`)
 * rather than the word a user would pick. That is not only cosmetic:
 * the grams those rare characters produce segfault the pin's own
 * bigram-export iterator on the BerkeleyDB oracle (the class-(b)
 * use-after-free, compatibility-policy row 1), while the rows a real
 * selection produces export cleanly on all three DBMs.
 *
 * Neither SORT_WITHOUT_SENTENCE_CANDIDATE nor
 * SORT_WITHOUT_LONGER_CANDIDATE is set, so the n-best and longer
 * candidates are still prepended exactly as a real frontend sees them;
 * the scan below simply walks past them. */
#define DRIVER_SORT_OPTION \
    (SORT_BY_PHRASE_LENGTH | SORT_BY_PINYIN_LENGTH | SORT_BY_FREQUENCY)

/* The first NORMAL_CANDIDATE in the current list, or NULL.
 *
 * Candidate 0 is deliberately not what this returns. Once
 * m_nbest_results is populated, _prepend_sentence_candidates puts an
 * NBEST_MATCH_CANDIDATE at position 0 (pinyin.cpp:1934), and choosing
 * one at n-best index 0 runs diff_result(best, best)
 * (pinyin.cpp:2515-2521), which `continue`s past every equal token
 * (phonetic_lookup.cpp:187) and so reaches add_constraint for none of
 * them. train_result3 gates its entire body — the m_user_bigram->store
 * included — on a constraint being present at the position
 * (phonetic_lookup.h:866), so a train after that selection writes
 * nothing at all. ibus refuses the same case outright: it trains an
 * n-best selection only `if (index != 0)`
 * (PYPLibPinyinCandidates.cc:116).
 *
 * A NORMAL_CANDIDATE is ibus's "the user picked a word" path, and
 * choosing one reaches constraints->add_constraint (pinyin.cpp:2582),
 * which installs the CONSTRAINT_ONESTEP training reads. */
static lookup_candidate_t *first_normal_candidate(pinyin_instance_t *instance) {
    guint n_candidates = 0;
    if (!pinyin_get_n_candidate(instance, &n_candidates))
        return NULL;

    for (guint i = 0; i < n_candidates; ++i) {
        lookup_candidate_t *candidate = NULL;
        if (!pinyin_get_candidate(instance, i, &candidate) || !candidate)
            continue;
        lookup_candidate_type_t type;
        if (!pinyin_get_candidate_type(instance, candidate, &type))
            continue;
        if (NORMAL_CANDIDATE == type)
            return candidate;
    }
    return NULL;
}

/* One input, in the order ibus drives the library.
 *
 * PYPFullPinyinEditor::updatePinyin parses and then calls
 * pinyin_guess_sentence on every keystroke, and PhoneticEditor::update
 * lists candidates only afterwards (PYPPhoneticEditor.cc:355) — so
 * m_nbest_results is always populated before pinyin_guess_candidates
 * runs. Listing candidates first, as this driver used to, gives the
 * first input no sentence candidate at all and every later input the
 * leaked n-best results of the one before it: neither
 * pinyin_parse_more_full_pinyins (pinyin.cpp:1497-1525) nor
 * pinyin_guess_candidates clears them.
 *
 * The selection loop is PhoneticEditor::selectCandidateInternal
 * (PYPPhoneticEditor.cc:494-511): choose at the lookup cursor, re-guess
 * the sentence under the new constraint, advance, repeat. Upstream's own
 * end test compares that key-position cursor against a character count,
 * so the end of the input is taken here from the call ibus makes right
 * after it — pinyin_get_pinyin_key_rest, which is false once the cursor
 * passes the last key position (pinyin.cpp:2950) and is in the cursor's
 * own unit. */
static int train_one(pinyin_instance_t *instance, const char *input) {
    if ((int)strlen(input) !=
        pinyin_parse_more_full_pinyins(instance, input)) {
        fprintf(stderr, "parse failed: %s\n", input);
        return 1;
    }
    if (!pinyin_guess_sentence(instance)) {
        fprintf(stderr, "guess failed: %s\n", input);
        return 1;
    }

    size_t cursor = 0;
    ChewingKeyRest *key_rest = NULL;
    while (pinyin_get_pinyin_key_rest(instance, cursor, &key_rest)) {
        if (!pinyin_guess_candidates(instance, cursor,
                                     DRIVER_SORT_OPTION)) {
            fprintf(stderr, "guess candidates failed: %s\n", input);
            return 1;
        }
        lookup_candidate_t *candidate = first_normal_candidate(instance);
        if (!candidate) {
            fprintf(stderr, "no normal candidate at %zu: %s\n", cursor,
                    input);
            return 1;
        }
        /* add_constraint returns 0 when the span does not fit the
         * constraint array (phonetic_lookup.cpp:64), which leaves the
         * cursor where it was: a selection that cannot advance would
         * spin here rather than fail. */
        int next = pinyin_choose_candidate(instance, cursor, candidate);
        if (next <= (int)cursor) {
            fprintf(stderr, "choose failed at %zu: %s\n", cursor, input);
            return 1;
        }
        cursor = (size_t)next;
        if (!pinyin_guess_sentence(instance)) {
            fprintf(stderr, "guess failed after choose: %s\n", input);
            return 1;
        }
    }

    if (!pinyin_train(instance, 0)) {
        fprintf(stderr, "train failed: %s\n", input);
        return 1;
    }

    /* ibus's `remember-every-input` path (§6): the committed
     * sentence is also added as a user phrase, which is what writes
     * user.bin and the two index trees. Without it the profile would
     * be phrase-free and those files untested — so a failed remember
     * is a failed run, not a note: a silent skip here would let a
     * green differential through without those files exercised. */
    char *sentence = NULL;
    if (!pinyin_get_sentence(instance, 0, &sentence) || !sentence) {
        fprintf(stderr, "no sentence to remember: %s\n", input);
        return 1;
    }
    if (!pinyin_remember_user_input(instance, sentence, -1)) {
        fprintf(stderr, "remember failed: %s (%s)\n", input, sentence);
        g_free(sentence);
        return 1;
    }
    g_free(sentence);

    /* PhoneticEditor::reset (PYPPhoneticEditor.cc:341): a committed
     * sentence clears the instance. Only pinyin_reset clears
     * m_constraints and m_nbest_results (pinyin.cpp:2693), so without
     * it each input trains against the state its predecessor left. */
    if (!pinyin_reset(instance)) {
        fprintf(stderr, "reset failed: %s\n", input);
        return 1;
    }
    return 0;
}

static int train(const char *system_dir, const char *user_dir,
                 char **inputs, int n_inputs) {
    pinyin_context_t *context = pinyin_init(system_dir, user_dir);
    if (!context)
        return 1;

    pinyin_instance_t *instance = pinyin_alloc_instance(context);
    for (int i = 0; i < n_inputs; ++i) {
        if (train_one(instance, inputs[i]))
            return 1;
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

/* The phrase export alone. Phase D uses this rather than `dump`: the
 * pin's bigram-export iterator carries a registered class-(b)
 * use-after-free that segfaults when an export cycle repeats
 * (compatibility-policy.md row 1, pinyin.cpp:842-872), so the reverse
 * direction asserts on the pin's phrase surface — the file-format
 * question — not on a surface with a known divergence. */
static int dump_phrases_only(const char *system_dir, const char *user_dir) {
    pinyin_context_t *context = pinyin_init(system_dir, user_dir);
    if (!context)
        return 1;
    if (dump_phrases(context))
        return 1;
    pinyin_fini(context);
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
                "usage: %s train|dump|phrases <system-dir> <user-dir> [input...]\n"
                "  train    parse, choose, train, and remember each input, then save\n"
                "  dump     print the P (phrase) and B (bigram) export rows\n"
                "  phrases  print the P rows only (the pin's reliable export surface)\n",
                argv[0]);
        return 2;
    }
    if (strcmp(argv[1], "train") == 0)
        return train(argv[2], argv[3], argv + 4, argc - 4);
    if (strcmp(argv[1], "dump") == 0)
        return dump(argv[2], argv[3]);
    if (strcmp(argv[1], "phrases") == 0)
        return dump_phrases_only(argv[2], argv[3]);
    fprintf(stderr, "unknown mode: %s\n", argv[1]);
    return 2;
}
