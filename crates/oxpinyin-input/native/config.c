/*
 * The C library calls of libpinyin's table.conf and user.conf readers and
 * writer (074a2219 src/storage/table_info.cpp:194-397), and the init's and
 * the save's lines that share a failure sequence with those: one call per
 * function, with the pin's own format strings, so the host libc parses,
 * prints and sets the locale exactly as the pin's does. Rust passes no
 * format string and makes no variadic call; it chooses which of these to
 * run and in what order.
 */

#include <locale.h>
#include <stdio.h>
#include <stdlib.h>

/* table_info.cpp:197, :291, :328, :372, :378, :394. */
char *oxpinyin_input_setlocale_numeric(const char *locale) {
    return setlocale(LC_NUMERIC, locale);
}

/* table_info.cpp:199, :330. */
FILE *oxpinyin_input_fopen_read(const char *filename) {
    return fopen(filename, "r");
}

/* table_info.cpp:380. */
FILE *oxpinyin_input_fopen_write(const char *filename) {
    return fopen(filename, "w");
}

/* table_info.cpp:210, :216, :222, :289, :340, :346, :370, :392. */
int oxpinyin_input_fclose(FILE *stream) {
    return fclose(stream);
}

/* table_info.cpp:254. */
int oxpinyin_input_feof(FILE *stream) {
    return feof(stream);
}

/* table_info.cpp:208, :338. */
int oxpinyin_input_scan_binary_format_version(FILE *input, int *binver) {
    return fscanf(input, "binary format version:%d\n", binver);
}

/* table_info.cpp:214, :344. */
int oxpinyin_input_scan_model_data_version(FILE *input, int *modelver) {
    return fscanf(input, "model data version:%d\n", modelver);
}

/* table_info.cpp:220 (gfloat is float). */
int oxpinyin_input_scan_lambda_parameter(FILE *input, float *lambda) {
    return fscanf(input, "lambda parameter:%f\n", lambda);
}

/* table_info.cpp:228. */
int oxpinyin_input_scan_source_table_format(FILE *input, char str[256]) {
    return fscanf(input, "source table format:%255s\n", str);
}

/* table_info.cpp:232, :352. */
int oxpinyin_input_scan_database_format(FILE *input, char str[256]) {
    return fscanf(input, "database format:%255s\n", str);
}

/* table_info.cpp:357. */
int oxpinyin_input_scan_open_counter(FILE *input, int *counter) {
    return fscanf(input, "open counter:%d\n", counter);
}

/* table_info.cpp:255-257. */
int oxpinyin_input_scan_table_row(FILE *input, char words[6][256]) {
    return fscanf(input, "%255s %255s %255s %255s %255s %255s\n", words[0],
                  words[1], words[2], words[3], words[4], words[5]);
}

/* table_info.cpp:160. */
int oxpinyin_input_atoi(const char *str) {
    return atoi(str);
}

/* table_info.cpp:386-390: four writes whose results the pin ignores. */
void oxpinyin_input_print_user_table_info(FILE *output, int binver,
                                          int modelver, const char *format,
                                          int counter) {
    fprintf(output, "binary format version:%d\n", binver);
    fprintf(output, "model data version:%d\n", modelver);
    fprintf(output, "database format:%s\n", format);
    fprintf(output, "open counter:%d\n", counter);
}

/* table_info.cpp:201. */
void oxpinyin_input_report_system_open_failed(const char *filename) {
    fprintf(stderr, "open %s failed.\n", filename);
}

/* table_info.cpp:332: no newline, as the pin's has none. */
void oxpinyin_input_report_user_open_failed(const char *filename) {
    fprintf(stderr, "open %s failed.", filename);
}

/* table_info.cpp:382. */
void oxpinyin_input_report_user_write_failed(const char *filename) {
    fprintf(stderr, "write %s failed.\n", filename);
}

/*
 * pinyin.cpp:1061, :1080, :1096, :1110, :1123; zhuyin.cpp:611, :637, :657,
 * :674, :689: a save's failed rename, on the stream mark_version's
 * table_info.cpp:382 line follows it on.
 */
void oxpinyin_input_report_rename_failed(const char *from, const char *to) {
    fprintf(stderr, "rename %s to %s failed.\n", from, to);
}

/*
 * pinyin.cpp:338, zhuyin.cpp:282: the init's report of a table.conf
 * SystemTableInfo2::load refused, after the load's own table_info.cpp:201
 * line when the file did not open.
 */
void oxpinyin_input_report_load_failed(const char *filename) {
    fprintf(stderr, "load %s failed!\n", filename);
}

/*
 * pinyin.cpp:256, :290 (the init's library loop), :956 (the save's
 * _write_files); zhuyin.cpp:200, :234, :589: a library MemoryChunk::mmap
 * refused, in the sequence check_format's and mark_version's lines share.
 */
void oxpinyin_input_report_mmap_failed(const char *filename) {
    fprintf(stderr, "mmap %s failed!\n", filename);
}
