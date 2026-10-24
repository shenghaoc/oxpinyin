// #1125 prototype: construct the pin's concrete DBMs, bypassing PolyDBM and
// key=value parsing. Default constructors/Open match libpinyin at 074a2219:
// ngram_tkrzwdb.cpp:48-80, chewing_large_table2_tkrzwdb.cpp:87-126,
// phrase_large_table3_tkrzwdb.cpp:77-117; attach_options in tkrzwdb_utils.h.
// The callback contract follows tkrzw_langc.cc at tkrzw 9ee8416; pointer tags
// are translated to the library's own C++ sentinels. Use a probe-certified
// libtkrzw: the C++ ABI and sentinel identity must match the installed headers.
#include "shim.h"

#include <tkrzw_dbm_baby.h>
#include <tkrzw_dbm_hash.h>
#include <tkrzw_dbm_tiny.h>
#include <tkrzw_dbm_tree.h>

#include <exception>
#include <memory>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

static_assert(int{tkrzw::Status::SUCCESS} == OXTKRZW_STATUS_SUCCESS, "SUCCESS");
static_assert(int{tkrzw::Status::SYSTEM_ERROR} == OXTKRZW_STATUS_SYSTEM_ERROR, "SYSTEM_ERROR");
static_assert(int{tkrzw::Status::NOT_FOUND_ERROR} == OXTKRZW_STATUS_NOT_FOUND_ERROR, "NOT_FOUND");

namespace {
thread_local int32_t last_code = OXTKRZW_STATUS_SUCCESS;
thread_local std::string last_message;
// Set the code before attempting any allocation. Even recording an allocation
// failure's text may fail; returning failure and SYSTEM_ERROR always holds.
void fail(const char* message) noexcept {
  last_code = OXTKRZW_STATUS_SYSTEM_ERROR;
  last_message.clear();
  try {
    last_message = message;
  } catch (...) {
  }
}

bool finish(tkrzw::Status status) {
  last_code = status.GetCode();
  // Most successful record operations return a status without a message.
  // Preserve an empty message without constructing and move-assigning a
  // temporary std::string. Nonempty messages still take the original path;
  // every caller remains inside guarded(), including allocation failures.
  if (status.HasMessage()) {
    last_message = status.GetMessage();
  } else {
    last_message.clear();
  }
  return last_code == OXTKRZW_STATUS_SUCCESS;
}

template <typename F, typename R>
R guarded(F&& work, R failure) noexcept {
  try {
    return work();
  } catch (const std::exception& error) {
    fail(error.what());
  } catch (...) {
    fail("C++ exception in the tkrzw shim");
  }
  return failure;
}

tkrzw::DBM* dbm(OxTkrzwDBM* db) noexcept {
  return reinterpret_cast<tkrzw::DBM*>(db);
}
tkrzw::DBM::Iterator* iterator(OxTkrzwDBMIter* iter) noexcept {
  return reinterpret_cast<tkrzw::DBM::Iterator*>(iter);
}

class Processor final : public tkrzw::DBM::RecordProcessor {
 public:
  Processor(oxtkrzw_record_processor proc, void* arg) : proc_(proc), arg_(arg) {}
  std::string_view ProcessFull(std::string_view key, std::string_view value) override {
    return invoke(key, value.data(), value.size());
  }
  std::string_view ProcessEmpty(std::string_view key) override {
    return invoke(key, nullptr, -1);
  }
 private:
  std::string_view invoke(std::string_view key, const char* value, int32_t size) {
    int32_t new_size = 0;
    const char* result = proc_(arg_, key.data(), key.size(), value, size, &new_size);
    if (result == OXTKRZW_REC_PROC_NOOP) return NOOP;
    if (result == OXTKRZW_REC_PROC_REMOVE) return REMOVE;
    if (!result || new_size < 0) throw std::invalid_argument("invalid record callback result");
    return std::string_view(result, new_size);
  }
  oxtkrzw_record_processor proc_;
  void* arg_;
};
}  // namespace

extern "C" {
const char* const OXTKRZW_REC_PROC_NOOP = reinterpret_cast<const char*>(intptr_t{-1});
const char* const OXTKRZW_REC_PROC_REMOVE = reinterpret_cast<const char*>(intptr_t{-2});

OxTkrzwStatus oxtkrzw_get_last_status(void) noexcept {
  return {last_code, last_message.c_str()};
}
int32_t oxtkrzw_get_last_status_code(void) noexcept { return last_code; }

OxTkrzwDBM* oxtkrzw_dbm_open(int32_t type, const char* path, bool writable,
                            bool no_create) noexcept {
  return guarded([&]() -> OxTkrzwDBM* {
    std::unique_ptr<tkrzw::DBM> db;
    switch (type) {
      case OXTKRZW_HASH: db = std::make_unique<tkrzw::HashDBM>(); break;
      case OXTKRZW_TREE: db = std::make_unique<tkrzw::TreeDBM>(); break;
      case OXTKRZW_TINY: db = std::make_unique<tkrzw::TinyDBM>(); break;
      case OXTKRZW_BABY: db = std::make_unique<tkrzw::BabyDBM>(); break;
      default:
        finish(tkrzw::Status(tkrzw::Status::INVALID_ARGUMENT_ERROR, "unknown DBM class"));
        return nullptr;
    }
    if (type == OXTKRZW_HASH || type == OXTKRZW_TREE) {
      const int32_t options = no_create ? tkrzw::File::OPEN_NO_CREATE : tkrzw::File::OPEN_DEFAULT;
      if (!finish(db->Open(path, writable, options))) return nullptr;
    } else {
      finish(tkrzw::Status());
    }
    return reinterpret_cast<OxTkrzwDBM*>(db.release());
  }, static_cast<OxTkrzwDBM*>(nullptr));
}

bool oxtkrzw_dbm_close(OxTkrzwDBM* db) noexcept {
  return guarded([&] {
    std::unique_ptr<tkrzw::DBM> owned(dbm(db));
    return finish(owned->IsOpen() ? owned->Close() : tkrzw::Status());
  }, false);
}
bool oxtkrzw_dbm_check(OxTkrzwDBM* db, const char* key, int32_t size) noexcept {
  return guarded([&] { return finish(dbm(db)->Get(std::string_view(key, size))); }, false);
}
bool oxtkrzw_dbm_process(OxTkrzwDBM* db, const char* key, int32_t size,
                         oxtkrzw_record_processor proc, void* arg, bool writable) noexcept {
  return guarded([&] {
    Processor processor(proc, arg);
    return finish(dbm(db)->Process(std::string_view(key, size), &processor, writable));
  }, false);
}
bool oxtkrzw_dbm_process_multi(OxTkrzwDBM* db, OxTkrzwKeyProcPair* pairs,
                               int32_t count, bool writable) noexcept {
  return guarded([&] {
    std::vector<Processor> processors;
    processors.reserve(count);
    std::vector<std::pair<std::string_view, tkrzw::DBM::RecordProcessor*>> batch;
    batch.reserve(count);
    for (int32_t i = 0; i < count; ++i) {
      processors.emplace_back(pairs[i].proc, pairs[i].proc_arg);
      batch.emplace_back(std::string_view(pairs[i].key_ptr, pairs[i].key_size), &processors.back());
    }
    return finish(dbm(db)->ProcessMulti(batch, writable));
  }, false);
}
bool oxtkrzw_dbm_set(OxTkrzwDBM* db, const char* key, int32_t key_size,
                     const char* value, int32_t value_size, bool overwrite) noexcept {
  return guarded([&] {
    return finish(dbm(db)->Set(std::string_view(key, key_size), std::string_view(value, value_size), overwrite));
  }, false);
}
int64_t oxtkrzw_dbm_count(OxTkrzwDBM* db) noexcept {
  return guarded([&]() -> int64_t {
    int64_t count = -1;
    return finish(dbm(db)->Count(&count)) ? count : -1;
  }, int64_t{-1});
}
bool oxtkrzw_dbm_is_healthy(OxTkrzwDBM* db) noexcept {
  return guarded([&] { return dbm(db)->IsHealthy(); }, false);
}
bool oxtkrzw_dbm_synchronize(OxTkrzwDBM* db, bool hard) noexcept {
  return guarded([&] { return finish(dbm(db)->Synchronize(hard)); }, false);
}
bool oxtkrzw_dbm_rebuild(OxTkrzwDBM* db) noexcept {
  return guarded([&] { return finish(dbm(db)->Rebuild()); }, false);
}
OxTkrzwDBMIter* oxtkrzw_dbm_make_iterator(OxTkrzwDBM* db) noexcept {
  return guarded([&] {
    auto iter = dbm(db)->MakeIterator();
    if (!iter) {
      fail("iterator allocation failed");
      return static_cast<OxTkrzwDBMIter*>(nullptr);
    }
    return reinterpret_cast<OxTkrzwDBMIter*>(iter.release());
  }, static_cast<OxTkrzwDBMIter*>(nullptr));
}
void oxtkrzw_dbm_iter_free(OxTkrzwDBMIter* iter) noexcept {
  guarded([&] { delete iterator(iter); return true; }, false);
}
bool oxtkrzw_dbm_iter_first(OxTkrzwDBMIter* iter) noexcept {
  return guarded([&] { return finish(iterator(iter)->First()); }, false);
}
bool oxtkrzw_dbm_iter_jump(OxTkrzwDBMIter* iter, const char* key, int32_t size) noexcept {
  return guarded([&] { return finish(iterator(iter)->Jump(std::string_view(key, size))); }, false);
}
bool oxtkrzw_dbm_iter_next(OxTkrzwDBMIter* iter) noexcept {
  return guarded([&] { return finish(iterator(iter)->Next()); }, false);
}
bool oxtkrzw_dbm_iter_process(OxTkrzwDBMIter* iter, oxtkrzw_record_processor proc,
                              void* arg, bool writable) noexcept {
  return guarded([&] {
    Processor processor(proc, arg);
    return finish(iterator(iter)->Process(&processor, writable));
  }, false);
}
}  // extern "C"
