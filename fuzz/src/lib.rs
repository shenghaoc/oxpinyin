//! The entry point of every fuzz target.
//!
//! A target is an ordinary executable. AFL++ starts it once per input through
//! its forkserver and writes the input to its standard input. The target reads
//! the input, runs its body once and exits. A panic aborts the process (the
//! release profile of this workspace sets `panic = "abort"`), so AFL++ records
//! it as a crash.

use std::io::Read;

/// Reads standard input to its end, then runs `target` once on the bytes.
///
/// The body runs under a scratch directory of its own, which this function
/// removes when the body returns. `TMPDIR` names it, so every temporary file
/// and directory of the body is inside it, also the ones that a static holds.
/// Each input is one process, so without it each input would leave its files
/// in the system temporary directory. An input that aborts leaves its scratch
/// directory.
pub fn run_once(target: impl FnOnce(&[u8])) {
    let mut input = Vec::new();
    std::io::stdin()
        .lock()
        .read_to_end(&mut input)
        .expect("read the input from standard input");
    let scratch = tempfile::Builder::new()
        .prefix("oxpinyin-fuzz-")
        .tempdir()
        .expect("a scratch directory");
    #[allow(unsafe_code)]
    // SAFETY: `main` calls this function first, before any thread of the
    // process exists, so no other thread reads the environment meanwhile.
    unsafe {
        std::env::set_var("TMPDIR", scratch.path());
    }
    target(&input);
    drop(scratch);
}
