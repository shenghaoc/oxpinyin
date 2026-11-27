//! Nonshipping C-ABI workloads shared by the measurement recipes.
#![deny(clippy::multiple_unsafe_ops_per_block)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unreachable
)]

#[expect(
    unsafe_code,
    reason = "Loads the measured C ABI and borrows its returned strings"
)]
mod abi;
#[expect(
    unsafe_code,
    reason = "Stable profiling anchor symbols use unsafe no_mangle attributes"
)]
mod choose;
#[forbid(unsafe_code)]
mod import;
#[forbid(unsafe_code)]
mod output;
#[forbid(unsafe_code)]
mod perf;
mod platform;
#[expect(
    unsafe_code,
    reason = "Stable profiling anchor symbols use unsafe no_mangle attributes"
)]
mod sentence;
#[expect(
    unsafe_code,
    reason = "Stable profiling anchor symbols use unsafe no_mangle attributes"
)]
mod workload;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Run the typing, sentence, diagnostic or native measurement entry point.
#[must_use]
pub fn bisect() -> i32 {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str);
    // A target refuses the modes it cannot measure with the usage status, before any work.
    let result = if let Some(reason) = platform::refusal(mode) {
        Err(Usage(reason.into()).into())
    } else {
        match mode {
            Some("--perf") => perf::run(&args),
            Some("--perf-sentence" | "--perf-typing-sequence") => sentence::run(&args),
            Some("--perf-choose") => choose::run(&args),
            Some("--measure") => platform::native(&args),
            _ => Err(format!("Usage: {} --perf <path-to-so> <systemdir>\n       {} --perf-sentence|--perf-typing-sequence|--perf-choose ...", args[0], args[0]).into()),
        }
    };
    finish(result)
}

/// Run the import, fixture, export, cold-open or retained-heap entry point.
#[must_use]
pub fn system_import_profile() -> i32 {
    finish(import::run(&std::env::args().collect::<Vec<_>>()))
}

fn finish(result: Result<()>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(error) => {
            if let Some(usage) = error.downcast_ref::<Usage>() {
                if !usage.0.is_empty() {
                    eprintln!("{}", usage.0);
                }
                2
            } else {
                eprintln!("{error}");
                1
            }
        }
    }
}

#[derive(Debug)]
struct Usage(String);
impl std::fmt::Display for Usage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Usage {}

fn require(ok: bool, message: &'static str) -> Result<()> {
    if ok { Ok(()) } else { Err(message.into()) }
}
