/*
 * driver-common.h — helpers the C drivers in tools/bisection share.
 *
 * Only code that was identical across drivers lives here, each piece with
 * the output, exit status and cleanup it had in every driver. Setup,
 * teardown, the typed API tables and the scenario printers stay in each
 * driver. Every helper is static inline: a driver links no new object
 * and pays nothing for a helper it does not call. Include this after the
 * driver's feature macros and system headers.
 */
#ifndef OXPINYIN_DRIVER_COMMON_H
#define OXPINYIN_DRIVER_COMMON_H

#include <dirent.h>
#include <dlfcn.h>
#include <errno.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/* ── Required symbols ─────────────────────────────────────────────────── */

/* Resolves `name`, or prints "  MISSING: <name>" and exits 1. */
static inline void *dc_require(void *handle, const char *name) {
    void *symbol = dlsym(handle, name);
    if (!symbol) {
        fprintf(stderr, "  MISSING: %s\n", name);
        exit(1);
    }
    return symbol;
}

/* dc_require with the residue drivers' "MISSING <name>" spelling. */
static inline void *dc_require_terse(void *handle, const char *name) {
    void *symbol = dlsym(handle, name);
    if (!symbol) {
        fprintf(stderr, "MISSING %s\n", name);
        exit(1);
    }
    return symbol;
}

/* Resolves `name`; when it is absent, prints "  MISSING: <name>", counts
 * it in *missing and returns NULL, so the caller reports every gap. */
static inline void *dc_resolve(void *handle, const char *name, int *missing) {
    void *symbol = dlsym(handle, name);
    if (!symbol) {
        fprintf(stderr, "  MISSING: %s\n", name);
        (*missing)++;
    }
    return symbol;
}

/* ── Caller-owned strings ─────────────────────────────────────────────── */

/* Releases a string the library may have left NULL. */
static inline void dc_release(void (*release)(void *), void *text) {
    if (text)
        release(text);
}

/* ── Temporary user directories ───────────────────────────────────────── */

/* Unlinks the entries directly inside `dir`, then removes `dir`. Shallow
 * on purpose: the user stores write regular files only, so nothing
 * descends; a subdirectory would keep `dir` in place. */
static inline void dc_remove_dir_shallow(const char *dir) {
    DIR *d = opendir(dir);
    if (d) {
        struct dirent *entry;
        while ((entry = readdir(d)) != NULL) {
            if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0)
                continue;
            char path[4096];
            if (snprintf(path, sizeof(path), "%s/%s", dir, entry->d_name) >=
                (int)sizeof(path))
                continue;
            unlink(path);
        }
        closedir(d);
    }
    rmdir(dir);
}

/* ── Inputs ───────────────────────────────────────────────────────────── */

/* 1 when <dir>/<name> opens for reading, else 0. The path is cut at 4095
 * bytes without a diagnostic. */
static inline int dc_file_readable(const char *dir, const char *name) {
    char path[4096];
    snprintf(path, sizeof(path), "%s/%s", dir, name);
    FILE *file = fopen(path, "rb");
    if (file) {
        fclose(file);
        return 1;
    }
    return 0;
}

/* Reads a rounds env var into `*out`, rejecting everything atoi would
 * silently accept: empty values, trailing text ("3junk"), conversion
 * errors, overflow, and anything outside 1..8. Unset keeps the caller's
 * default. Returns 1 after the diagnostic, else 0. */
static inline int dc_parse_rounds_env(const char *name, int *out) {
    const char *value = getenv(name);
    if (!value)
        return 0;
    char *end = NULL;
    errno = 0;
    long parsed = strtol(value, &end, 10);
    if (value[0] == '\0' || *end != '\0' || errno != 0 ||
        parsed < 1 || parsed > 8) {
        fprintf(stderr, "%s must be an integer in 1..8, got \"%s\"\n",
                name, value);
        return 1;
    }
    *out = (int)parsed;
    return 0;
}

/* ── Candidate-type labels ────────────────────────────────────────────── */

/* The pinyin lookup_candidate_type_t values 1..8 by their header names. */
static inline const char *dc_type_name(int type) {
    switch (type) {
    case 1: return "NBEST_MATCH";
    case 2: return "NORMAL";
    case 3: return "ZOMBIE";
    case 4: return "PREDICTED_BIGRAM";
    case 5: return "PREDICTED_PREFIX";
    case 6: return "ADDON";
    case 7: return "LONGER";
    case 8: return "PREDICTED_PUNCT";
    default: return "UNKNOWN";
    }
}

/* The same values by the probe drivers' short names. */
static inline const char *dc_type_short_name(int type) {
    switch (type) {
    case 1: return "NBEST";
    case 2: return "NORMAL";
    case 3: return "ZOMBIE";
    case 4: return "PRED_BIGRAM";
    case 5: return "PRED_PREFIX";
    case 6: return "ADDON";
    case 7: return "LONGER";
    case 8: return "PRED_PUNCT";
    default: return "?";
    }
}

/* NBEST_MATCH (1), and NORMAL for every other value: the scheme drivers
 * whose windows hold only those two types. */
static inline const char *dc_type_nbest_or_normal(int type) {
    return type == 1 ? "NBEST_MATCH" : "NORMAL";
}

/* ── Bigram export ────────────────────────────────────────────────────── */

/* The residue drivers' user-bigram dump: begin, at most 33 rows from the
 * has_next/get_next pair with both strings released, the row count, end.
 * Each row prints get_next's return as given. */
static inline void dc_dump_bigram(void *(*begin)(void *), bool (*has_next)(void *),
                                  bool (*get_next)(void *, char **, char **, int32_t *),
                                  void (*end)(void *), void (*release)(void *),
                                  void *context, const char *tag) {
    void *it = begin(context);
    printf("%s:begin_bigram=%s\n", tag, it ? "ok" : "NULL");
    if (!it)
        return;
    unsigned row = 0;
    while (has_next(it)) {
        char *phrase = NULL, *pinyin = NULL;
        int32_t count = 0;
        bool ok = get_next(it, &phrase, &pinyin, &count);
        printf("%s:bigram[%u]=%s %s|%s|%d\n", tag, row, ok ? "true" : "false",
               phrase ? phrase : "(null)", pinyin ? pinyin : "(null)", count);
        dc_release(release, phrase);
        dc_release(release, pinyin);
        row++;
        if (row > 32)
            break;
    }
    printf("%s:bigram_rows=%u\n", tag, row);
    end(it);
}

#endif
