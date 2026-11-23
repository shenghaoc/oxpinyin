//! `spseg` (libpinyin 074a2219 `utils/segment/spseg.cpp`).

fn main() {
    #[allow(
        unsafe_code,
        reason = "the standalone binary owns the process-level C stdout close"
    )]
    // SAFETY: This binary starts no threads and registers no printing exit
    // handlers. The close is the last output operation. Option parsing and
    // input reads are finished, and the locals that then drop do not print.
    unsafe {
        oxpinyin_utils::segment::spseg::main(std::env::args_os());
    }
}
