// C++ shim over tkrzw's TreeDBM for the oxpinyin store backend.
//
// The interface is deliberately narrow and boring: no templates, no
// overloads, no exceptions, no default arguments.  Every entry point
// takes and returns types cxx can bridge directly, and every failure
// comes back as a ShimStatus carrying tkrzw's own status code and
// message rather than throwing.  All policy — table-name framing, bound
// semantics, write buffering — lives on the Rust side; this file only
// moves bytes.
//
// Reads borrow tkrzw's record memory instead of copying it, the way
// libpinyin's own tkrzw code does (KeyCollectProcessor walks records
// through ProcessEach with in-place string_views).  db_get and db_scan
// install a RecordProcessor whose ProcessFull hands the key and value
// straight to a Rust callback as (pointer, length) slices; there is no
// intermediary std::string and no per-byte push.  A borrow lasts only
// for the callback's duration — a callback that wants to keep a row
// must copy it itself, which is the Rust visitor's business, not the
// shim's.

#pragma once

#include <cstdint>
#include <memory>

#include <tkrzw_dbm.h>
#include <tkrzw_dbm_tree.h>

#include "rust/cxx.h"

namespace oxpinyin_tkrzw {

struct ShimStatus;
struct Mutation;

// An open TreeDBM.  Closed by the destructor, so a dropped Rust handle
// always flushes and releases the file lock.
class Db {
 public:
  Db() = default;
  ~Db();
  Db(const Db&) = delete;
  Db& operator=(const Db&) = delete;

  // `mutable` because tkrzw documents every TreeDBM operation as
  // thread-safe: the database carries its own locking, so a read, a
  // ProcessMulti batch and a Synchronize are all sound through a shared
  // reference.  That is what lets the Rust side keep `get` and `write`
  // on `&self`, as the redb and LMDB backends do.
  mutable tkrzw::TreeDBM dbm;
};

// The Rust-side record sinks.  A sink receives `ctx` — an opaque token
// the Rust caller owns — plus each borrowed record half as a raw
// (pointer, length) pair over tkrzw's record memory.  Raw words rather
// than rust::Slice on purpose: every Slice construction is an
// out-of-line call into the cxx runtime, which is measurable on a
// per-record path; the Rust side assembles the slice itself.  The get
// sink returns nothing; the walk sink returns false to stop the walk.
using GetSink = rust::Fn<void(std::size_t, const std::uint8_t*, std::size_t)>;
using WalkSink = rust::Fn<bool(std::size_t, const std::uint8_t*, std::size_t,
                               const std::uint8_t*, std::size_t)>;

// Opens `path` as a TreeDBM with tkrzw's default tuning, which means the
// default LexicalKeyComparator: plain unsigned byte order, the order the
// redb and LMDB backends already provide.  No custom comparator is set,
// exactly as libpinyin's tkrzwdb_utils.h leaves it.
std::unique_ptr<Db> open_db(rust::Slice<const std::uint8_t> path, bool writable,
                            bool no_create, ShimStatus& status);

// Reads one record, handing the borrowed value to `visit` exactly once.
// The Rust side's copy inside `visit` is the only one the read pays.
ShimStatus db_get(const Db& db, rust::Slice<const std::uint8_t> key,
                  GetSink visit, std::size_t ctx);

// Applies every mutation in one ProcessMulti call: tkrzw locks all the
// named records for the duration, so the batch lands as a unit against
// any other reader or writer of the same database.
ShimStatus db_apply(const Db& db, rust::Slice<const Mutation> mutations);

ShimStatus db_synchronize(const Db& db, bool hard);

ShimStatus db_rebuild(const Db& db);

// Walks the records from the lower-bound `start` in ascending key order,
// handing each to `visit` borrowed, until `visit` returns false, the
// records run out, or the walk leaves the key space past the end.  One
// iterator's Jump + Process + Next loop — the same walk a native tkrzw
// client performs, with each row passed in place instead of copied into
// intermediary strings.
ShimStatus db_scan(const Db& db, rust::Slice<const std::uint8_t> start,
                   WalkSink visit, std::size_t ctx);

}  // namespace oxpinyin_tkrzw
