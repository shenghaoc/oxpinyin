/* C entry points of the Kyoto Cabinet shim (shim.cc, #1118).
 *
 * The subset of kclangc.h this backend calls, with the database class
 * chosen at construction instead of through PolyDB's "#type=" open
 * parameter. libpinyin constructs TreeDB, HashDB, ProtoTreeDB and StashDB
 * directly (chewing_large_table2_kyotodb.cpp:87,
 * phrase_large_table3_kyotodb.cpp:102, punct_table_kyotodb.cpp:62,
 * ngram_kyotodb.cpp:58,117 at 074a2219); so does this shim.
 *
 * Every function is noexcept. A C++ exception is always caught in shim.cc
 * and turned into the function's failure return; recording it as a SYSTEM
 * error on the database is best effort. Visitor callbacks are the caller's
 * C functions; a null return from one means "no operation". */
#ifndef OXPINYIN_KC_SHIM_H
#define OXPINYIN_KC_SHIM_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
#define OXKC_NOEXCEPT noexcept
extern "C" {
#else
#define OXKC_NOEXCEPT
#endif

typedef struct oxkc_db oxkc_db;
typedef struct oxkc_cur oxkc_cur;

/* The database classes libpinyin constructs. */
enum {
  OXKC_TREE = 0,
  OXKC_HASH = 1,
  OXKC_STASH = 2,
  OXKC_PROTO_TREE = 3
};

/* BasicDB open modes; shim.cc checks them against kcdb.h. */
enum {
  OXKC_OREADER = 1 << 0,
  OXKC_OWRITER = 1 << 1,
  OXKC_OCREATE = 1 << 2
};

typedef const char* (*oxkc_visit_full)(const char* kbuf, size_t ksiz, const char* vbuf,
                                       size_t vsiz, size_t* sp, void* opq);
typedef const char* (*oxkc_visit_empty)(const char* kbuf, size_t ksiz, size_t* sp, void* opq);

const char* oxkc_version(void) OXKC_NOEXCEPT;
int32_t oxkc_enorec(void) OXKC_NOEXCEPT;
const char* oxkc_ecodename(int32_t code) OXKC_NOEXCEPT;

oxkc_db* oxkc_db_new(int32_t type) OXKC_NOEXCEPT;
int32_t oxkc_db_tune_buckets(oxkc_db* db, int64_t bnum) OXKC_NOEXCEPT;
int32_t oxkc_db_open(oxkc_db* db, const char* path, uint32_t mode) OXKC_NOEXCEPT;
int32_t oxkc_db_close(oxkc_db* db) OXKC_NOEXCEPT;
void oxkc_db_del(oxkc_db* db) OXKC_NOEXCEPT;
int32_t oxkc_db_ecode(oxkc_db* db) OXKC_NOEXCEPT;
const char* oxkc_db_emsg(oxkc_db* db) OXKC_NOEXCEPT;
int32_t oxkc_db_accept(oxkc_db* db, const char* kbuf, size_t ksiz, oxkc_visit_full full,
                       oxkc_visit_empty empty, void* opq, int32_t writable) OXKC_NOEXCEPT;
int32_t oxkc_db_check(oxkc_db* db, const char* kbuf, size_t ksiz) OXKC_NOEXCEPT;
int32_t oxkc_db_iterate(oxkc_db* db, oxkc_visit_full full, void* opq, int32_t writable) OXKC_NOEXCEPT;
int32_t oxkc_db_set(oxkc_db* db, const char* kbuf, size_t ksiz, const char* vbuf, size_t vsiz) OXKC_NOEXCEPT;
int32_t oxkc_db_remove(oxkc_db* db, const char* kbuf, size_t ksiz) OXKC_NOEXCEPT;
int32_t oxkc_db_sync(oxkc_db* db, int32_t hard) OXKC_NOEXCEPT;
int32_t oxkc_db_load_snapshot(oxkc_db* db, const char* src) OXKC_NOEXCEPT;
int32_t oxkc_db_dump_snapshot(oxkc_db* db, const char* dest) OXKC_NOEXCEPT;
int64_t oxkc_db_count(oxkc_db* db) OXKC_NOEXCEPT;
int32_t oxkc_db_begin_transaction(oxkc_db* db, int32_t hard) OXKC_NOEXCEPT;
int32_t oxkc_db_end_transaction(oxkc_db* db, int32_t commit) OXKC_NOEXCEPT;
oxkc_cur* oxkc_db_cursor(oxkc_db* db) OXKC_NOEXCEPT;

void oxkc_cur_del(oxkc_cur* cur) OXKC_NOEXCEPT;
int32_t oxkc_cur_jump(oxkc_cur* cur) OXKC_NOEXCEPT;
int32_t oxkc_cur_jump_key(oxkc_cur* cur, const char* kbuf, size_t ksiz) OXKC_NOEXCEPT;
int32_t oxkc_cur_accept(oxkc_cur* cur, oxkc_visit_full full, void* opq, int32_t writable,
                        int32_t step) OXKC_NOEXCEPT;
int32_t oxkc_cur_ecode(oxkc_cur* cur) OXKC_NOEXCEPT;

#ifdef __cplusplus
}
#endif

#endif
