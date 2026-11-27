//! Hostile bytes through `table.conf` as `pinyin_init` reads it: the pin's
//! own scanner calls (`oxpinyin`) and `TableConf::load`'s decoding of
//! what they answered. The load must answer on every input, never panic,
//! and be deterministic; a refusal is a legitimate answer, not a failure.

use std::io::Write;

use oxpinyin::facade::TableConf;

/// Inputs per child: AFL++'s suggested start
/// (`instrumentation/README.persistent_mode.md`), which bounds a child's
/// lifetime. `AFL_FUZZER_LOOPCOUNT` overrides it.
const LOOP_COUNT: usize = 1000;

fn main() {
    oxpinyin_fuzz::fuzz(
        LOOP_COUNT,
        &[include_bytes!("../../fixtures/w3/kct/table.conf")],
        run,
    );
}

/// Runs one input. The scanner reads a `FILE *`, so the input is written to
/// a file of its own, which exists only while the input runs.
fn run(data: &[u8]) {
    let mut file = tempfile::Builder::new()
        .prefix("oxpinyin-fuzz-table-conf-")
        .tempfile()
        .expect("scratch file");
    file.write_all(data).expect("scratch write");
    file.flush().expect("scratch flush");
    let path = file.path();
    let first = TableConf::load_path(path);
    let second = TableConf::load_path(path);
    // Compared through `Debug`: a NaN λ is not equal to itself.
    assert_eq!(
        format!("{first:?}"),
        format!("{second:?}"),
        "load must be deterministic"
    );
}
