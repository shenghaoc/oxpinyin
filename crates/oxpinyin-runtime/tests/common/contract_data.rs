//! The pinned libpinyin-data package (`tools/oracle/libpinyin-data.json`), in
//! tkrzw format. A missing package is a failure, not a skip.
//!
//! The package tests of `oxpinyin-runtime`, `oxpinyin-data`, `oxpinyin-capi`
//! and `oxpinyin-zhuyin-capi` include this file by path.

use std::path::PathBuf;

/// Where `just fetch-contract-data` extracts the pinned version:
/// `contract-data/<version>` under the cache root `$OXPINYIN_CACHE`, which
/// `just test` exports. A direct cargo run sets the variable itself.
pub fn contract_data_dir() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let pin = std::fs::read_to_string(root.join("tools/oracle/libpinyin-data.json"))
        .expect("the data pin is committed");
    let version = pin
        .lines()
        .find_map(|line| line.trim().strip_prefix("\"version\": \""))
        .and_then(|rest| rest.strip_suffix("\","))
        .expect("the data pin names a version");
    let cache = std::env::var_os("OXPINYIN_CACHE")
        .map(PathBuf::from)
        .filter(|cache| cache.is_absolute())
        .expect("OXPINYIN_CACHE names an absolute cache root; `just test` sets it");
    let dir = cache
        .join("contract-data")
        .join(version)
        .join("libpinyin/data");
    assert!(
        dir.join("bigram.db").is_file(),
        "pinned system data missing at {}; run just fetch-contract-data",
        dir.display()
    );
    dir
}
