//! Generated declarations for the tkrzw shim's plain-C ABI (`shim.h`).
//!
//! The shim constructs concrete DBMs with default tuning and contains all
//! C++ exceptions. Failure status and its message are thread-local; the
//! safe wrapper copies them immediately. Record buffers are borrowed only
//! for a callback; returned values are copied before that call completes.
//! The NOOP/REMOVE pointer tags belong to the shim, which translates them
//! to libtkrzw's own C++ RecordProcessor sentinel objects. A certified
//! library is still required for cross-library sentinel identity.
//!
//! No C++ types or owned allocations cross this ABI. Every unsafe call's
//! pointer and lifetime contract is documented where the wrapper makes it.
#![expect(unsafe_code, reason = "raw FFI declarations; see the module docs")]
#![allow(dead_code, missing_docs, non_camel_case_types)]
#![allow(non_snake_case, non_upper_case_globals)]
include!(concat!(env!("OUT_DIR"), "/tkrzw_shim.rs"));
