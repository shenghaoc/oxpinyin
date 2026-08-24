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

// The Rust side pins these codes by number (src/tkrzw/mod.rs:
// STATUS_SUCCESS / STATUS_SYSTEM_ERROR). If upstream ever renumbers
// them, this build must fail rather than let statuses be silently
// misclassified.
static_assert(static_cast<std::int32_t>(tkrzw::Status::SUCCESS) == 0);
static_assert(static_cast<std::int32_t>(tkrzw::Status::UNKNOWN_ERROR) == 1);
static_assert(static_cast<std::int32_t>(tkrzw::Status::SYSTEM_ERROR) == 2);

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

// Maps an exception escaping an entry point onto the status channel:
// UNKNOWN_ERROR ("generic error whose cause is unknown") fits an
// allocation failure or similar. Callers report any non-SUCCESS code
// verbatim, so nothing here has to be named on the Rust side. Building
// the message can itself throw under memory exhaustion; that terminates
// the process, which is still better than unwinding into Rust.
ShimStatus wrap(const tkrzw::Status& status);

ShimStatus caught(const std::exception& e) {
  return wrap(tkrzw::Status(tkrzw::Status::UNKNOWN_ERROR, e.what()));
}

ShimStatus caught_unknown() {
  return wrap(
      tkrzw::Status(tkrzw::Status::UNKNOWN_ERROR, "unknown C++ exception"));
}

ShimStatus wrap(const tkrzw::Status& status) {
  try {
    ShimStatus out;
    out.code = static_cast<std::int32_t>(status.GetCode());
    out.message = rust::String(status.GetMessage());
    return out;
  } catch (...) {
    // No allocation to report with; hand back a message-less status.
    ShimStatus out;
    out.code = static_cast<std::int32_t>(tkrzw::Status::UNKNOWN_ERROR);
    out.message = rust::String();
    return out;
  }
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
  try {
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
  } catch (const std::exception& e) {
    status = caught(e);
    return nullptr;
  } catch (...) {
    status = caught_unknown();
    return nullptr;
  }
}

ShimStatus db_get(const Db& db, rust::Slice<const std::uint8_t> key,
                  rust::Vec<std::uint8_t>& value, bool& found) {
  try {
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
  } catch (const std::exception& e) {
    found = false;
    return caught(e);
  } catch (...) {
    found = false;
    return caught_unknown();
  }
}

ShimStatus db_apply(const Db& db, rust::Slice<const Mutation> mutations) {
  try {
    std::vector<Apply> procs;
    procs.reserve(mutations.size());
    std::vector<std::pair<std::string_view, tkrzw::DBM::RecordProcessor*>>
        pairs;
    pairs.reserve(mutations.size());
    for (const Mutation& mutation : mutations) {
      procs.emplace_back(vec_view(mutation.value), mutation.remove);
    }
    // Built in a second pass. The reserve calls above fix both vectors'
    // capacity, so no emplace_back reallocates and the `&procs[i]`
    // pointers taken here stay stable while ProcessMulti runs.
    for (std::size_t i = 0; i < procs.size(); ++i) {
      pairs.emplace_back(vec_view(mutations[i].key), &procs[i]);
    }
    return wrap(db.dbm.ProcessMulti(pairs, true));
  } catch (const std::exception& e) {
    return caught(e);
  } catch (...) {
    return caught_unknown();
  }
}

ShimStatus db_synchronize(const Db& db, bool hard) {
  try {
    return wrap(db.dbm.Synchronize(hard, nullptr));
  } catch (const std::exception& e) {
    return caught(e);
  } catch (...) {
    return caught_unknown();
  }
}

ShimStatus db_rebuild(const Db& db) {
  try {
    return wrap(db.dbm.Rebuild());
  } catch (const std::exception& e) {
    return caught(e);
  } catch (...) {
    return caught_unknown();
  }
}

std::unique_ptr<Iter> db_iter(const Db& db) {
  try {
    return std::make_unique<Iter>(db.dbm.MakeIterator());
  } catch (...) {
    // The Rust side treats a null iterator as a closed handle and
    // reports it; the status channel is not in this signature.
    return nullptr;
  }
}

ShimStatus iter_jump(Iter& iter, rust::Slice<const std::uint8_t> key) {
  try {
    return wrap(iter.iter->Jump(as_view(key)));
  } catch (const std::exception& e) {
    return caught(e);
  } catch (...) {
    return caught_unknown();
  }
}

ShimStatus iter_get(Iter& iter, rust::Vec<std::uint8_t>& key,
                    rust::Vec<std::uint8_t>& value, bool& found) {
  try {
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
  } catch (const std::exception& e) {
    found = false;
    return caught(e);
  } catch (...) {
    found = false;
    return caught_unknown();
  }
}

ShimStatus iter_next(Iter& iter) {
  try {
    const tkrzw::Status status = iter.iter->Next();
    if (status == tkrzw::Status::NOT_FOUND_ERROR) {
      return ok();
    }
    return wrap(status);
  } catch (const std::exception& e) {
    return caught(e);
  } catch (...) {
    return caught_unknown();
  }
}

}  // namespace oxpinyin_tkrzw
