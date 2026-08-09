// Single source of truth for the frozen pin reference.
//
// `build.rs` includes this file directly so the link-time prefix check and the
// run-time context check can never drift apart. Keep it free of `use`
// statements and of anything that depends on crate items.

/// Frozen pin reference from `docs/findings/oracle-environment.md`.
///
/// Identical to `EXPECTED_PIN_REF` in `tools/capture/run-capture.sh`: the
/// capture harness and the differential harness must agree on the subject.
pub const EXPECTED_PIN_REF: &str = concat!(
    "libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c",
    "+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155",
    "+dbm-tkrzw"
);

/// File the build recipe writes into the prefix.
pub const MANIFEST_FILE_NAME: &str = "oracle-pin.txt";
