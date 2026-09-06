// Single source of truth for the frozen pin reference.
//
// `build.rs` includes this file directly so the link-time prefix check and the
// run-time context check can never drift apart. Keep it free of `use`
// statements and of anything that depends on crate items.

/// Frozen pin reference from `docs/testing/oracle-environment.md`.
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

/// Expected (`pin_ref`, manifest `dbm=` name) for a bench-only oracle prefix:
/// the frozen ref with its DBM suffix replaced, so the tag, commit and model
/// stay pinned and only the backend differs. `None` for any name other than
/// `"kc"` and `"bdb"` — the tkrzw oracle is the frozen ref itself and needs
/// no relaxed form. Shared by `build.rs` and `pin.rs` so the link-time and
/// run-time checks accept exactly the same strings.
#[must_use]
pub fn bench_pin_ref(dbm: &str) -> Option<(String, &'static str)> {
    let base = EXPECTED_PIN_REF.strip_suffix("+dbm-tkrzw")?;
    match dbm {
        "kc" => Some((format!("{base}+dbm-kc"), "KyotoCabinet")),
        "bdb" => Some((format!("{base}+dbm-bdb"), "BerkeleyDB")),
        _ => None,
    }
}
