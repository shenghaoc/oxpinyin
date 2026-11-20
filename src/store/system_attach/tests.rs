use std::path::Path;

use super::SystemAttachment;

#[test]
fn attach_bigram_answers_a_failed_open_apart_from_an_earlier_failure() {
    // A failed native open leaves the attachment missing. A failure
    // before the open is an error on every backend.
    let attach = |path: &Path, read_only| SystemAttachment::attach_bigram(path, read_only);
    let scratch = tempfile::tempdir().expect("scratch directory");
    let missing = scratch.path().join("missing.db");
    assert!(matches!(
        attach(&missing, true).expect("attach"),
        SystemAttachment::Unopened
    ));
    assert!(!missing.exists());
    let created = scratch.path().join("bigram.db");
    for read_only in [false, true] {
        assert!(
            matches!(
                attach(&created, read_only).expect("attach"),
                SystemAttachment::Open(_)
            ),
            "read only: {read_only}"
        );
    }
    let nul = Path::new(std::ffi::OsStr::new("bad\0name.db"));
    assert!(attach(nul, true).is_err(), "a rejected path");
}
