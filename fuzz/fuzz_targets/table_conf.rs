#![no_main]
//! Hostile bytes through `table.conf` as `pinyin_init` reads it: the pin's
//! own scanner calls (`oxpinyin-input`) and `TableConf::load`'s decoding of
//! what they answered. The load must answer on every input, never panic,
//! and be deterministic; a refusal is a legitimate answer, not a failure.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use oxpinyin_data::table_info::TableConf;

/// One scratch file per fuzzing process: the scanner reads a `FILE *`.
fn scratch() -> &'static Path {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        std::env::temp_dir().join(format!("oxpinyin-fuzz-table-conf-{}", std::process::id()))
    })
}

fuzz_target!(|data: &[u8]| {
    let path = scratch();
    if std::fs::write(path, data).is_err() {
        return;
    }
    let first = TableConf::load_path(path);
    let second = TableConf::load_path(path);
    // Compared through `Debug`: a NaN λ is not equal to itself.
    assert_eq!(
        format!("{first:?}"),
        format!("{second:?}"),
        "load must be deterministic"
    );
    if let Some(lambda) = first.ok().and_then(|conf| conf.lambda_unit()) {
        // A rational λ is in [0, 1]; the accessors are total.
        let value = lambda.as_f64();
        assert!((0.0..=1.0).contains(&value), "λ out of range: {value}");
    }
});
