//! Typing and native measurement driver.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unreachable
)]

fn main() {
    std::process::exit(oxpinyin_measurement::bisect());
}
