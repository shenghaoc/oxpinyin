//! A Berkeley DB system attach whose `DB->open` fails.
//!
//! The pin keeps that handle and calls it again (`chewing_large_table2_bdb.cpp:
//! 87-93` at 074a2219); libdb answers each call with `BDB1565` on stderr.
//! Berkeley DB requires `DB->close` after a failed open and no other call on
//! the handle (`dbopen.html`, `dbclose.html`, 5.3.28), so the store closes it
//! and the later read, suggestion and reset reach nothing. libdb writes to the
//! process's stderr, so the test runs the attachment in a child process and
//! reads that stream.
#![cfg(feature = "bdb")]

use std::path::Path;
use std::process::Command;

use oxpinyin_store::SystemAttachment;

/// Set in the child: the path of the missing system file.
const CHILD: &str = "OXPINYIN_STORE_BDB_FAILED_ATTACHMENT";
const TEST: &str = "a_failed_system_attachment_makes_no_call_on_its_handle";

#[test]
fn a_failed_system_attachment_makes_no_call_on_its_handle() {
    if let Some(path) = std::env::var_os(CHILD) {
        let SystemAttachment::Unopened(db) =
            SystemAttachment::open(Path::new(&path), false).expect("attach")
        else {
            panic!("a missing file opened");
        };
        db.read(&[0; 8]);
        db.suggest(&[0; 8]);
        db.finish();
        assert!(db.is_finished());
        return;
    }
    let missing = std::env::temp_dir().join(format!(
        "oxpinyin-store-bdb-failed-attachment-{}.bin",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&missing);
    let output = Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", TEST, "--test-threads=1"])
        .env(CHILD, &missing)
        .output()
        .expect("child test");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "child failed: {stderr}");
    assert!(
        !stderr.contains("BDB1565"),
        "a call reached the closed handle: {stderr}"
    );
    assert!(!missing.exists(), "the read-only attach created no file");
}
