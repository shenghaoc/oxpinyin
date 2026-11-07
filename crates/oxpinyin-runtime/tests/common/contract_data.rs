//! The pinned libpinyin-data package (`tools/oracle/libpinyin-data.json`), in
//! tkrzw format. A missing package is a failure, not a skip.

use std::path::PathBuf;

use oxpinyin_data::SystemDbm;

/// Where `just fetch-contract-data` extracts the pinned version.
pub fn contract_data_dir() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let pin = std::fs::read_to_string(root.join("tools/oracle/libpinyin-data.json"))
        .expect("the data pin is committed");
    let version = pin
        .lines()
        .find_map(|line| line.trim().strip_prefix("\"version\": \""))
        .and_then(|rest| rest.strip_suffix("\","))
        .expect("the data pin names a version");
    let dir = root
        .join("target/contract-data")
        .join(version)
        .join("libpinyin/data");
    assert!(
        dir.join(SystemDbm::Bigram.file_name()).is_file(),
        "pinned system data missing at {}; run just fetch-contract-data",
        dir.display()
    );
    dir
}
