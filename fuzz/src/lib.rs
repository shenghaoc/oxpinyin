//! The entry point of every fuzz target.
//!
//! A target is an ordinary executable whose `main` prepares the state that
//! every input shares, then calls [`fuzz`] with its body. The body takes one
//! input and returns when that input is done. A panic aborts the process (the
//! release profile of this workspace sets `panic = "abort"`), so AFL++ records
//! it as a crash.
//!
//! With the `afl` feature, which `just fuzz` and `just fuzz-minimize` select,
//! [`fuzz`] hands the body to the `afl` crate. Under afl-fuzz, the target runs
//! in persistent mode: the forkserver forks after the shared state exists, and
//! each child runs up to its loop count of inputs, which AFL++ delivers in
//! shared memory. Each target states its loop count; `AFL_FUZZER_LOOPCOUNT`
//! overrides it. Run on its own, the same executable reads one input from
//! standard input, and takes the same path as under afl-fuzz. Without the
//! feature, [`fuzz`] reads standard input itself, and the build needs no AFL++
//! runtime.
//!
//! With a loop count above 1, the body first runs on the empty input and on
//! the target's warm-up inputs, before the forkserver starts, so state that
//! the code builds on first use exists in every child; the persistent loop
//! itself is left uninstrumented, so the first input of a child covers the
//! same edges as the others. An input must leave no other state that the next
//! input of the same child can observe: the body removes every file and
//! directory it creates before it returns. In an AddressSanitizer build,
//! LeakSanitizer checks the heap after each input, on both paths, and aborts
//! on a leak. The finding is then the input that leaked, not a later input of
//! the same child.
//!
//! With a loop count of 1, each child runs one input and exits: the first
//! pass of `__afl_persistent_loop` returns 1, and its next call returns 0
//! without stopping the child. No input follows another in a process, so no
//! warm-up runs, and LeakSanitizer's check at exit names the one input; the
//! per-input check is skipped.

#![feature(cfg_sanitize)]

/// Runs `target` on each input, then checks for leaks (see the module).
///
/// `loop_count` is the number of inputs that one child runs, unless
/// `AFL_FUZZER_LOOPCOUNT` overrides it. `warm_up` holds inputs that reach the
/// state a dependency builds on first use, such as `memmap2`'s cached page
/// size behind a valid chunk file. `main` calls this function before it starts
/// a thread.
pub fn fuzz<T>(loop_count: usize, warm_up: &[&[u8]], mut target: T)
where
    T: FnMut(&[u8]) + std::panic::RefUnwindSafe,
{
    // Lazily built state would otherwise be built by the first input of each
    // child that reaches it, which would then cover edges that later inputs
    // do not: AFL++ counts that as instability. The empty input and the
    // warm-up inputs build it before the forkserver starts.
    #[cfg(feature = "afl")]
    {
        let per_input = set_loop_count(loop_count) != 1;
        if per_input {
            target(&[]);
            for input in warm_up {
                target(input);
            }
        }
        persistent(target, per_input);
    }
    #[cfg(not(feature = "afl"))]
    {
        use std::io::Read;

        let _ = (loop_count, warm_up);

        let mut input = Vec::new();
        std::io::stdin()
            .lock()
            .read_to_end(&mut input)
            .expect("read the input from standard input");
        run_one(&mut target, &input, true);
    }
}

/// The loop count that the `afl` crate will use: `AFL_FUZZER_LOOPCOUNT` when
/// it is set, else `loop_count`, which this function then sets as
/// `AFL_FUZZER_LOOPCOUNT`. The crate reads the count only from that variable
/// (`afl-0.18.2/src/lib.rs:302-308`), so standalone replay without any
/// environment and a run under afl-fuzz take the same path.
#[cfg(feature = "afl")]
fn set_loop_count(loop_count: usize) -> usize {
    const VARIABLE: &str = "AFL_FUZZER_LOOPCOUNT";
    if let Some(value) = std::env::var_os(VARIABLE) {
        // The crate refuses a value that is not a number when its loop
        // starts; until then, treat it as more than one input.
        return value
            .to_str()
            .and_then(|value| value.parse().ok())
            .unwrap_or(usize::MAX);
    }
    #[allow(unsafe_code)]
    // SAFETY: `fuzz` runs in `main` before the process starts a thread (its
    // contract), so no other thread reads or writes the environment.
    unsafe {
        std::env::set_var(VARIABLE, loop_count.to_string());
    }
    loop_count
}

/// The `afl` crate's loop. The first input enters it from its first call of
/// `__afl_persistent_loop`, the later inputs from the call at its end, so
/// its edges differ between the first input of a child and the others. The
/// recipe leaves this function uninstrumented (`AFL_LLVM_DENYLIST`,
/// `fuzz/instrument-denylist.txt`); [`run_one`] is a separate function, so the
/// target stays instrumented.
#[cfg(feature = "afl")]
#[inline(never)]
fn persistent<T>(mut target: T, leak_check_each: bool)
where
    T: FnMut(&[u8]) + std::panic::RefUnwindSafe,
{
    afl::fuzz(true, move |input| {
        run_one(&mut target, input, leak_check_each)
    });
}

/// Runs `target` on one input, then checks for leaks when `leak_check_each`.
#[inline(never)]
fn run_one<T>(target: &mut T, input: &[u8], leak_check_each: bool)
where
    T: FnMut(&[u8]),
{
    target(input);
    if leak_check_each {
        leak_check();
    }
}

/// Aborts when LeakSanitizer finds memory that nothing reaches. The check is
/// the repeatable one, so a clean input lets the next one run.
fn leak_check() {
    #[cfg(sanitize = "address")]
    {
        // SAFETY: the LeakSanitizer interface of the AddressSanitizer runtime
        // that this build links (`sanitizer/lsan_interface.h`); it takes no
        // argument and is safe to call at any time.
        #[allow(unsafe_code)]
        unsafe extern "C" {
            fn __lsan_do_recoverable_leak_check() -> std::os::raw::c_int;
        }
        #[allow(unsafe_code)]
        // SAFETY: as above.
        if unsafe { __lsan_do_recoverable_leak_check() } != 0 {
            std::process::abort();
        }
    }
}
