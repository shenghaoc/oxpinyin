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

// A non-null stand-in for an empty record's data pointer, so the Rust
// side can always build a well-formed empty slice.
const std::uint8_t* non_null(const char* data) {
  return reinterpret_cast<const std::uint8_t*>(data != nullptr ? data : "");
}

// Maps an exception escaping an entry point onto the status channel:
// UNKNOWN_ERROR ("generic error whose cause is unknown") fits an
// allocation failure or similar. Callers report any non-SUCCESS code
// verbatim, so nothing here has to be named on the Rust side.
//
// Translation must not itself throw: these handlers run from the catch
// blocks at the bridge boundary, so a throw here would keep unwinding
// into Rust, which cannot catch it. Building the message — or the
// Status that copies it into a std::string — can allocate, so it is
// guarded, and a failure falls back to a message-less UNKNOWN_ERROR.
ShimStatus wrap(const tkrzw::Status& status);

// The "no allocation left to say anything" status: UNKNOWN_ERROR with an
// empty message. Default-constructing the rust::String leaves it empty
// without allocating, so this cannot throw. Every translator falls back
// to it when even building the message fails.
ShimStatus message_less_unknown() {
  ShimStatus out;
  out.code = static_cast<std::int32_t>(tkrzw::Status::UNKNOWN_ERROR);
  out.message = rust::String();
  return out;
}

ShimStatus caught(const std::exception& e) {
  try {
    return wrap(tkrzw::Status(tkrzw::Status::UNKNOWN_ERROR, e.what()));
  } catch (...) {
    return message_less_unknown();
  }
}

ShimStatus caught_unknown() {
  try {
    return wrap(
        tkrzw::Status(tkrzw::Status::UNKNOWN_ERROR, "unknown C++ exception"));
  } catch (...) {
    return message_less_unknown();
  }
}

ShimStatus wrap(const tkrzw::Status& status) {
  try {
    ShimStatus out;
    out.code = static_cast<std::int32_t>(status.GetCode());
    out.message = rust::String(status.GetMessage());
    return out;
  } catch (...) {
    return message_less_unknown();
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

// Forwards one record's value to the Rust get callback, borrowed.  The
// callback firing at all is the "found" answer; ProcessEmpty below is
// the "not found" one.
class GetVisitor final : public tkrzw::DBM::RecordProcessor {
 public:
  GetVisitor(GetSink visit, std::size_t ctx) : visit_(visit), ctx_(ctx) {}

  std::string_view ProcessFull(std::string_view, std::string_view value) override {
    visit_(ctx_, non_null(value.data()), value.size());
    return NOOP;
  }

  std::string_view ProcessEmpty(std::string_view) override {
    return NOOP;
  }

 private:
  GetSink visit_;
  std::size_t ctx_;
};

// Forwards each walked record to the Rust scan callback, borrowed.  The
// callback's return value decides the walk: false stops it.
class WalkVisitor final : public tkrzw::DBM::RecordProcessor {
 public:
  WalkVisitor(WalkSink visit, std::size_t ctx) : visit_(visit), ctx_(ctx) {}

  std::string_view ProcessFull(std::string_view key, std::string_view value) override {
    kept_going_ = visit_(ctx_, non_null(key.data()), key.size(),
                         non_null(value.data()), value.size());
    return NOOP;
  }

  std::string_view ProcessEmpty(std::string_view) override {
    return NOOP;
  }

  bool kept_going() const { return kept_going_; }

 private:
  WalkSink visit_;
  std::size_t ctx_;
  bool kept_going_ = true;
};

}  // namespace

Db::~Db() {
  if (dbm.IsOpen()) {
    dbm.Close();
  }
}

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
                  GetSink visit, std::size_t ctx) {
  try {
    GetVisitor visitor(visit, ctx);
    return wrap(db.dbm.Process(as_view(key), &visitor, false));
  } catch (const std::exception& e) {
    return caught(e);
  } catch (...) {
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

ShimStatus db_scan(const Db& db, rust::Slice<const std::uint8_t> start,
                   WalkSink visit, std::size_t ctx) {
  try {
    std::unique_ptr<tkrzw::DBM::Iterator> iter = db.dbm.MakeIterator();
    if (!iter) {
      ShimStatus out;
      out.code = static_cast<std::int32_t>(tkrzw::Status::SYSTEM_ERROR);
      out.message = rust::String("TreeDBM::MakeIterator returned null");
      return out;
    }
    const tkrzw::Status jump = iter->Jump(as_view(start));
    if (jump != tkrzw::Status::SUCCESS) {
      return wrap(jump);
    }
    for (;;) {
      WalkVisitor visitor(visit, ctx);
      const tkrzw::Status status = iter->Process(&visitor, false);
      if (status == tkrzw::Status::NOT_FOUND_ERROR) {
        return ok();
      }
      if (status != tkrzw::Status::SUCCESS) {
        return wrap(status);
      }
      if (!visitor.kept_going()) {
        return ok();
      }
      // Next past the last record does not fail; the Process above is what
      // ends the walk with NOT_FOUND_ERROR.
      const tkrzw::Status next = iter->Next();
      if (next == tkrzw::Status::NOT_FOUND_ERROR) {
        return ok();
      }
      if (next != tkrzw::Status::SUCCESS) {
        return wrap(next);
      }
    }
  } catch (const std::exception& e) {
    return caught(e);
  } catch (...) {
    return caught_unknown();
  }
}

}  // namespace oxpinyin_tkrzw
