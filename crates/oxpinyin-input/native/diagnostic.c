/*
 * The library's other fprintf(stderr, ...) lines (074a2219 src/pinyin.cpp,
 * src/zhuyin.cpp): one call per function, with the pin's own format string,
 * so every library line goes through the host C library's stderr stream, as
 * table_info.cpp's lines in config.c do. Rust passes no format string and
 * makes no variadic call.
 */

#include <stdio.h>

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
 * pinyin.cpp:256, :290 (the init's library loop and the library load
 * calls), :956 (the save's _write_files), :1265 (mask_out); zhuyin.cpp:200,
 * :234, :589, :800: a library MemoryChunk::mmap refused.
 */
void oxpinyin_input_report_mmap_failed(const char *filename) {
    fprintf(stderr, "mmap %s failed!\n", filename);
}
