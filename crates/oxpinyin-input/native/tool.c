/*
 * The pin utilities' C and GLib calls at their edge (074a2219 utils/):
 * locale, option parsing, numeric fields, text splitting and conversion,
 * and the output stream, one call (or the pin's fixed sequence of calls)
 * per function, so the host libc and GLib parse, convert and print exactly
 * as the pin's tools do. Compiled only for the `tools` feature, against
 * GLib's own headers, so GOptionEntry and the GLib types are not guessed.
 */

#include <errno.h>
#include <glib.h>
#include <locale.h>
#include <stdio.h>
#include <stdlib.h>

/* setlocale(LC_ALL, ""): the first call of every utility's main, e.g.
 * utils/segment/ngseg.cpp:111, utils/storage/gen_binary_files.cpp:112. */
char *oxpinyin_tool_setlocale_all_environment(void) {
    return setlocale(LC_ALL, "");
}

/* The four GOptionArg kinds the utilities use, numbered by the shim and
 * mapped to GLib's own constants below. */
enum oxpinyin_tool_option_kind {
    OXPINYIN_TOOL_OPTION_NONE = 0,
    OXPINYIN_TOOL_OPTION_INT = 1,
    OXPINYIN_TOOL_OPTION_FILENAME = 2,
    OXPINYIN_TOOL_OPTION_DOUBLE = 3,
};

static GOptionArg oxpinyin_tool_option_arg(int kind) {
    switch (kind) {
    case OXPINYIN_TOOL_OPTION_INT:
        return G_OPTION_ARG_INT;
    case OXPINYIN_TOOL_OPTION_FILENAME:
        return G_OPTION_ARG_FILENAME;
    case OXPINYIN_TOOL_OPTION_DOUBLE:
        return G_OPTION_ARG_DOUBLE;
    default:
        return G_OPTION_ARG_NONE;
    }
}

/* One GOptionEntry, with arg_data supplied by the shim. */
struct oxpinyin_tool_option {
    const char *long_name;
    char short_name;
    int flags;
    int arg;
    const char *description;
    const char *arg_description;
};

/* The storage behind one entry's arg_data: the type its GOptionArg names. */
union oxpinyin_tool_option_value {
    gboolean flag;
    gchar *string;
    gint integer;
    gdouble number;
};

/*
 * The option sequence every option-taking utility runs (e.g. ngseg.cpp:113-
 * 120, gen_binary_files.cpp:114-121): g_option_context_new,
 * g_option_context_add_main_entries with a NULL translation domain,
 * g_option_context_parse, and on failure
 * g_print("option parsing failed:%s\n", error->message). The caller then
 * makes the pin's exit(EINVAL). The entries and the context stay allocated
 * for the process, as the pin's static entries and unfreed context do.
 */
int oxpinyin_tool_parse_options(const char *parameter_string,
                                const struct oxpinyin_tool_option *options,
                                size_t count,
                                union oxpinyin_tool_option_value *values,
                                int *argc, char ***argv) {
    GOptionEntry *entries = g_new0(GOptionEntry, count + 1);
    for (size_t i = 0; i < count; i++) {
        entries[i].long_name = options[i].long_name;
        entries[i].short_name = options[i].short_name;
        entries[i].flags = options[i].flags;
        entries[i].arg = oxpinyin_tool_option_arg(options[i].arg);
        entries[i].arg_data = &values[i];
        entries[i].description = options[i].description;
        entries[i].arg_description = options[i].arg_description;
    }
    GError *error = NULL;
    GOptionContext *context = g_option_context_new(parameter_string);
    g_option_context_add_main_entries(context, entries, NULL);
    if (!g_option_context_parse(context, argc, argv, &error)) {
        g_print("option parsing failed:%s\n", error->message);
        return 0;
    }
    return 1;
}

/* g_free, for the strings GLib's option parser allocated. */
void oxpinyin_tool_g_free(void *memory) {
    g_free(memory);
}

/* atoi: TAGLIB_PARSE_SEGMENTED_LINE and TAGLIB_GET_TOKEN
 * (utils/utils_helper.h:27-69), gen_zhuyin_table.cpp:157. */
int oxpinyin_tool_atoi(const char *str) {
    return atoi(str);
}

/* atol: TAGLIB_GET_TAGVALUE (utils/utils_helper.h:60-69). */
long oxpinyin_tool_atol(const char *str) {
    return atol(str);
}

/* g_strsplit_set(line, delimiters, max_tokens): TAGLIB_PARSE_SEGMENTED_LINE
 * (utils/utils_helper.h:27-46). Freed with g_strfreev. */
char **oxpinyin_tool_strsplit_set(const char *string, const char *delimiters,
                                  int max_tokens) {
    return g_strsplit_set(string, delimiters, max_tokens);
}

void oxpinyin_tool_strfreev(char **str_array) {
    g_strfreev(str_array);
}

/* g_utf8_strlen(str, -1): ngseg.cpp:190, spseg.cpp:271. */
long oxpinyin_tool_utf8_strlen(const char *str) {
    return g_utf8_strlen(str, -1);
}

/* g_utf8_to_ucs4(str, -1, NULL, &items_written, NULL): ngseg.cpp:192,
 * spseg.cpp:273. NULL on a conversion error; freed with g_free. */
gunichar *oxpinyin_tool_utf8_to_ucs4(const char *str, long *items_written) {
    glong written = 0;
    gunichar *result = g_utf8_to_ucs4(str, -1, NULL, &written, NULL);
    *items_written = written;
    return result;
}

/* g_ucs4_to_utf8(str, len, NULL, NULL, NULL): ngseg.cpp:80, :98. NULL on a
 * conversion error; freed with g_free. */
char *oxpinyin_tool_ucs4_to_utf8(const gunichar *str, long len) {
    return g_ucs4_to_utf8(str, len, NULL, NULL, NULL);
}

/* g_unichar_isspace and g_unichar_isgraph: split_line
 * (src/storage/tag_utility.cpp:118-170). */
int oxpinyin_tool_unichar_isspace(gunichar c) {
    return g_unichar_isspace(c);
}

int oxpinyin_tool_unichar_isgraph(gunichar c) {
    return g_unichar_isgraph(c);
}

/* The output stream: stdout or fopen(outputfile, "w") (ngseg.cpp:122-126),
 * closed with fclose (ngseg.cpp:260). */
FILE *oxpinyin_tool_stdout(void) {
    return stdout;
}

FILE *oxpinyin_tool_fopen_write(const char *filename) {
    return fopen(filename, "w");
}

int oxpinyin_tool_fclose(FILE *stream) {
    return fclose(stream);
}

/* perror(s): ngseg.cpp:124, :135 and the other utilities' open failures. */
void oxpinyin_tool_perror(const char *s) {
    perror(s);
}

/* exit(status): EINVAL after a refused option or argument, ngseg.cpp:119. */
void oxpinyin_tool_exit(int status) {
    exit(status);
}

/* EINVAL as the target's <errno.h> defines it. */
int oxpinyin_tool_einval(void) {
    return EINVAL;
}

/* ENOENT as the target's <errno.h> defines it. */
int oxpinyin_tool_enoent(void) {
    return ENOENT;
}

/* fopen(filename, "r") and feof(stream), for the utilities' C input. */
FILE *oxpinyin_tool_fopen_read(const char *filename) {
    return fopen(filename, "r");
}

int oxpinyin_tool_feof(FILE *stream) {
    return feof(stream);
}

/*
 * gen_pinyin_table (utils/storage/gen_pinyin_table.cpp at 074a2219): its
 * scanner, its GLib calls on the scanned phrase and its formatted lines.
 */

/* feed_file's fscanf (:145). */
int oxpinyin_tool_gpt_scan(FILE *infile, char phrase[1024], char pinyin[1024],
                           guint32 *freq) {
    return fscanf(infile, "%1023s %1023s %u", phrase, pinyin, freq);
}

/* feed_file's open failure (:140). */
void oxpinyin_tool_gpt_report_open_failed(const char *filename) {
    fprintf(stderr, "Can't open file %s.\n", filename);
}

/* gen_phrase_file's open failure (:263). */
void oxpinyin_tool_gpt_report_write_failed(const char *filename) {
    fprintf(stderr, "Can't write file %s.\n", filename);
}

/* main's node count (:114). */
void oxpinyin_tool_gpt_print_nnodes(int nnodes) {
    printf("nnodes: %d\n", nnodes);
}

/* feed_line's refusals (:169, :186) and duplicate (:215-216). */
void oxpinyin_tool_gpt_report_too_long(const char *phrase, const char *pinyin,
                                       guint32 freq) {
    fprintf(stderr, "Too long phrase:%s\t%s\t%d\n", phrase, pinyin, freq);
}

void oxpinyin_tool_gpt_report_invalid_pinyin(const char *phrase,
                                             const char *pinyin, guint32 freq) {
    fprintf(stderr, "Invalid pinyin:%s\t%s\t%d\n", phrase, pinyin, freq);
}

void oxpinyin_tool_gpt_report_duplicate(const char *phrase, const char *pinyin,
                                        guint32 freq) {
    fprintf(stderr, "Duplicate item: phrase:%s\tpinyin:%s\tfreq:%u\n", phrase,
            pinyin, freq);
}

/*
 * store_one_item's g_array_append_val(g_item_array[0], item) for a phrase
 * of length zero (:240): g_item_array[0] is NULL (:117), so GLib's own
 * precondition logs its critical and appends nothing.
 */
void oxpinyin_tool_gpt_store_into_null_array(void) {
    GArray *array = NULL;
    int item = 0;
    g_array_append_val(array, item);
}

/*
 * Not a pin line: oxpinyin's report of a phrase g_utf8_to_ucs4 refused
 * (:174), where the pin's g_ucs4_to_utf8 (:280-281) reads through the NULL
 * it kept (class (b)). Written on the C stderr stream with the pin's other
 * lines; the phrase bytes as they were scanned.
 */
void oxpinyin_tool_gpt_report_unconverted_phrase(const char *phrase) {
    fprintf(stderr, "gen_pinyin_table: invalid UTF-8 phrase: %s\n", phrase);
}

/* gen_phrase_file's row (:314-316). */
void oxpinyin_tool_gpt_print_row(FILE *outfile, const char *pinyin_str,
                                 const char *phrase_str, int token,
                                 guint32 freq) {
    fprintf(outfile, "%s\t%s\t%d\t%d\n", pinyin_str, phrase_str, token, freq);
}

/* gen_zhuyin_table.cpp:156-159 at 074a2219. */
void oxpinyin_tool_gzt_wrong_line(const char *line) {
    fprintf(stderr, "wrong line format:%s\n", line);
}

/* Class (b) refusal at gen_zhuyin_table.cpp:153 or :286-287. */
void oxpinyin_tool_gzt_refuse(const char *reason, const char *text) {
    fprintf(stderr, "gen_zhuyin_table: %s: %s\n", reason, text);
}
