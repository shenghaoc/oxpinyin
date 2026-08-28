//! Raw bindgen bindings for tkrzw's plain-C API (`tkrzw_langc.h`).
//!
//! Generated at build time by `build.rs` — nothing generated is committed —
//! and allowlisted down to exactly the entry points the backend's safe
//! wrapper in `super` calls. The declarations are raw and unsafe by
//! design; every invariant that makes calling them sound is documented at
//! the call site that relies on it, not here.
//!
//! # What crosses the ABI
//!
//! The C API is a thin, exception-free wrapper over `PolyDBM`: each entry
//! point catches every C++ exception and reports it through a thread-local
//! "last status" (`tkrzw_get_last_status`), whose message region is valid
//! only until the next status-setting call on the same thread — the safe
//! wrapper copies it immediately after each failed call, before any other
//! tkrzw call can overwrite it.
//!
//! Record data is borrowed, not owned: `tkrzw_dbm_process`,
//! `tkrzw_dbm_process_multi` and `tkrzw_dbm_iter_process` hand each key
//! and value to a callback as `(pointer, size)` pairs into tkrzw's record
//! memory, valid only for the callback's duration, and the value a
//! writable callback returns is copied by tkrzw before it returns ("the
//! ownership of the return value is not taken"). No `char*`-returning
//! entry point is bound at all, so the crate never holds an allocation
//! whose deallocator the C header leaves unnamed ("the free function").
//!
//! # Sentinels
//!
//! `TKRZW_REC_PROC_NOOP` and `TKRZW_REC_PROC_REMOVE` are pointer values
//! the C wrapper compares by value. The wrapper reads them from the
//! library's own globals at each use, so agreement with whatever the
//! library compares against is by construction, never by assumption.
//!
//! # Status codes
//!
//! The C enum's numbering is the ABI (`tkrzw_langc.h` pins
//! SUCCESS=0 … APPLICATION_ERROR=13, identical to the C++ `Status::Code`
//! it casts from). The generated `TKRZW_STATUS_*` constants are what the
//! wrapper matches on; `super` static-asserts the three it names against
//! the pinned numbers so an upstream renumbering fails the build rather
//! than silently misclassifying statuses.
#![allow(unsafe_code)] // raw FFI declarations; see the module docs.
#![allow(dead_code, missing_docs, non_camel_case_types)]
#![allow(non_snake_case, non_upper_case_globals)]
include!(concat!(env!("OUT_DIR"), "/tkrzw_langc.rs"));
