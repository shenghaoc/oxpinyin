#include "shim.h"

#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include <tkrzw_file.h>
#include <tkrzw_lib_common.h>

#include "oxpinyin-store/src/tkrzw/bridge.rs.h"

namespace oxpinyin_tkrzw {
namespace {

std::string_view as_view(rust::Slice<const std::uint8_t> bytes) {
  return std::string_view(reinterpret_cast<const char*>(bytes.data()), bytes.size());
}

std::string_view vec_view(const rust::Vec<std::uint8_t>& bytes) {
  return std::string_view(reinterpret_cast<const char*>(bytes.data()), bytes.size());
}

void fill(rust::Vec<std::uint8_t>& out, std::string_view bytes) {
  out.clear();
  out.reserve(bytes.size());
  for (const char byte : bytes) {
    out.push_back(static_cast<std::uint8_t>(byte));
  }
}

ShimStatus wrap(const tkrzw::Status& status) {
  ShimStatus out;
  out.code = static_cast<std::int32_t>(status.GetCode());
  out.message = rust::String(status.GetMessage());
  return out;
}

ShimStatus ok() {
  return wrap(tkrzw::Status(tkrzw::Status::SUCCESS));
}

// Writes one buffered mutation: a new value, or removal.
class Apply final : public tkrzw::DBM::RecordProcessor {
 public:
  Apply(std::string_view value, bool remove) : value_(value), remove_(remove) {}

  std::string_view ProcessFull(std::string_view, std::string_view) override {
    return remove_ ? REMOVE : value_;
  }

  std::string_view ProcessEmpty(std::string_view) override {
    // Removing an absent record is a no-op, matching the redb and LMDB
    // backends' `WriteTxn::remove`.
    return remove_ ? NOOP : value_;
  }

 private:
  std::string_view value_;
  bool remove_;
};

}  // namespace

Db::~Db() {
  if (dbm.IsOpen()) {
    dbm.Close();
  }
}

Iter::Iter(std::unique_ptr<tkrzw::DBM::Iterator> iter) : iter(std::move(iter)) {}

std::unique_ptr<Db> open_db(rust::Slice<const std::uint8_t> path, bool writable,
                            bool no_create, ShimStatus& status) {
  auto db = std::make_unique<Db>();
  const std::int32_t options =
      no_create ? tkrzw::File::OPEN_NO_CREATE : tkrzw::File::OPEN_DEFAULT;
  const tkrzw::Status result =
      db->dbm.Open(std::string(as_view(path)), writable, options);
  status = wrap(result);
  if (result != tkrzw::Status::SUCCESS) {
    return nullptr;
  }
  return db;
}

ShimStatus db_get(const Db& db, rust::Slice<const std::uint8_t> key,
                  rust::Vec<std::uint8_t>& value, bool& found) {
  std::string stored;
  const tkrzw::Status status = db.dbm.Get(as_view(key), &stored);
  if (status == tkrzw::Status::NOT_FOUND_ERROR) {
    found = false;
    return ok();
  }
  if (status != tkrzw::Status::SUCCESS) {
    found = false;
    return wrap(status);
  }
  found = true;
  fill(value, stored);
  return wrap(status);
}

ShimStatus db_apply(const Db& db, rust::Slice<const Mutation> mutations) {
  std::vector<Apply> procs;
  procs.reserve(mutations.size());
  std::vector<std::pair<std::string_view, tkrzw::DBM::RecordProcessor*>> pairs;
  pairs.reserve(mutations.size());
  for (const Mutation& mutation : mutations) {
    procs.emplace_back(vec_view(mutation.value), mutation.remove);
  }
  // Built in a second pass: emplace_back may reallocate, which would
  // dangle any pointer taken during the first.
  for (std::size_t i = 0; i < procs.size(); ++i) {
    pairs.emplace_back(vec_view(mutations[i].key), &procs[i]);
  }
  return wrap(db.dbm.ProcessMulti(pairs, true));
}

ShimStatus db_synchronize(const Db& db, bool hard) {
  return wrap(db.dbm.Synchronize(hard, nullptr));
}

ShimStatus db_rebuild(const Db& db) {
  return wrap(db.dbm.Rebuild());
}

std::unique_ptr<Iter> db_iter(const Db& db) {
  return std::make_unique<Iter>(db.dbm.MakeIterator());
}

ShimStatus iter_jump(Iter& iter, rust::Slice<const std::uint8_t> key) {
  return wrap(iter.iter->Jump(as_view(key)));
}

ShimStatus iter_get(Iter& iter, rust::Vec<std::uint8_t>& key,
                    rust::Vec<std::uint8_t>& value, bool& found) {
  std::string stored_key;
  std::string stored_value;
  const tkrzw::Status status = iter.iter->Get(&stored_key, &stored_value);
  if (status == tkrzw::Status::NOT_FOUND_ERROR) {
    found = false;
    return ok();
  }
  if (status != tkrzw::Status::SUCCESS) {
    found = false;
    return wrap(status);
  }
  found = true;
  fill(key, stored_key);
  fill(value, stored_value);
  return wrap(status);
}

ShimStatus iter_next(Iter& iter) {
  const tkrzw::Status status = iter.iter->Next();
  if (status == tkrzw::Status::NOT_FOUND_ERROR) {
    return ok();
  }
  return wrap(status);
}

}  // namespace oxpinyin_tkrzw
