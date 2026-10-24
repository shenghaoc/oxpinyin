/* Minimal C ABI over the four concrete DBMs used by libpinyin at 074a2219.
 * All entry points contain C++ exceptions. Handles and record buffers remain
 * owned by C++; callback results are borrowed until the call returns.
 * Callers supply non-null handles/buffers and nonnegative, exact lengths.
 * Status messages are thread-local and valid until the next shim call.
 */
#ifndef OXPINYIN_TKRZW_SHIM_H
#define OXPINYIN_TKRZW_SHIM_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
#define OXTKRZW_NOEXCEPT noexcept
extern "C" {
#else
#define OXTKRZW_NOEXCEPT
#endif

typedef struct OxTkrzwDBM OxTkrzwDBM;
typedef struct OxTkrzwDBMIter OxTkrzwDBMIter;
typedef struct {
  int32_t code;
  const char* message;
} OxTkrzwStatus;

enum { OXTKRZW_HASH, OXTKRZW_TREE, OXTKRZW_TINY, OXTKRZW_BABY };
enum {
  OXTKRZW_STATUS_SUCCESS = 0,
  OXTKRZW_STATUS_SYSTEM_ERROR = 2,
  OXTKRZW_STATUS_NOT_FOUND_ERROR = 7
};

/* Tags translated to DBM::RecordProcessor's own sentinel objects by shim.cc. */
extern const char* const OXTKRZW_REC_PROC_NOOP;
extern const char* const OXTKRZW_REC_PROC_REMOVE;
typedef const char* (*oxtkrzw_record_processor)(
    void* arg, const char* key, int32_t key_size, const char* value,
    int32_t value_size, int32_t* new_value_size);
typedef struct {
  const char* key_ptr;
  int32_t key_size;
  oxtkrzw_record_processor proc;
  void* proc_arg;
} OxTkrzwKeyProcPair;

OxTkrzwStatus oxtkrzw_get_last_status(void) OXTKRZW_NOEXCEPT;
int32_t oxtkrzw_get_last_status_code(void) OXTKRZW_NOEXCEPT;
/* File classes Open(path, writable, OPEN_DEFAULT/OPEN_NO_CREATE), default
 * tuning. Memory classes are constructed without Open, as at the pin. */
OxTkrzwDBM* oxtkrzw_dbm_open(int32_t type, const char* path, bool writable,
                            bool no_create) OXTKRZW_NOEXCEPT;
/* Close (when opened) and delete, even if Close reports failure. */
bool oxtkrzw_dbm_close(OxTkrzwDBM* db) OXTKRZW_NOEXCEPT;
bool oxtkrzw_dbm_check(OxTkrzwDBM* db, const char* key, int32_t size) OXTKRZW_NOEXCEPT;
bool oxtkrzw_dbm_process(OxTkrzwDBM* db, const char* key, int32_t size,
                         oxtkrzw_record_processor proc, void* arg, bool writable) OXTKRZW_NOEXCEPT;
bool oxtkrzw_dbm_process_multi(OxTkrzwDBM* db, OxTkrzwKeyProcPair* pairs,
                               int32_t count, bool writable) OXTKRZW_NOEXCEPT;
/* #1114's load path: Set copies borrowed bytes, without a one-row batch. */
bool oxtkrzw_dbm_set(OxTkrzwDBM* db, const char* key, int32_t key_size,
                     const char* value, int32_t value_size, bool overwrite) OXTKRZW_NOEXCEPT;
int64_t oxtkrzw_dbm_count(OxTkrzwDBM* db) OXTKRZW_NOEXCEPT;
bool oxtkrzw_dbm_is_healthy(OxTkrzwDBM* db) OXTKRZW_NOEXCEPT;
bool oxtkrzw_dbm_synchronize(OxTkrzwDBM* db, bool hard) OXTKRZW_NOEXCEPT;
bool oxtkrzw_dbm_rebuild(OxTkrzwDBM* db) OXTKRZW_NOEXCEPT;
OxTkrzwDBMIter* oxtkrzw_dbm_make_iterator(OxTkrzwDBM* db) OXTKRZW_NOEXCEPT;
void oxtkrzw_dbm_iter_free(OxTkrzwDBMIter* iter) OXTKRZW_NOEXCEPT;
bool oxtkrzw_dbm_iter_first(OxTkrzwDBMIter* iter) OXTKRZW_NOEXCEPT;
bool oxtkrzw_dbm_iter_jump(OxTkrzwDBMIter* iter, const char* key, int32_t size) OXTKRZW_NOEXCEPT;
bool oxtkrzw_dbm_iter_next(OxTkrzwDBMIter* iter) OXTKRZW_NOEXCEPT;
bool oxtkrzw_dbm_iter_process(OxTkrzwDBMIter* iter, oxtkrzw_record_processor proc,
                              void* arg, bool writable) OXTKRZW_NOEXCEPT;

#ifdef __cplusplus
}
#endif
#endif
