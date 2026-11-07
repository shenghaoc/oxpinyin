//! Context finalization resets the retained failed system attachments while
//! an instance still holds the runtime (compatibility row 42): the pin's
//! `pinyin_fini` deletes the tables' objects (`pinyin.cpp:1205-1215`) and a
//! retained instance keeps only the context's remaining storage.
use std::path::PathBuf;

use oxpinyin_facade::{ContextCore, PINYIN_DEFAULT_OPTION_WORD};
use oxpinyin_user::UserConfLaw;

fn fixture_dir() -> PathBuf {
    let extension = oxpinyin_data::DEFAULT_STORE_EXT;
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("fixtures")
        .join("w3")
        .join(extension)
}

#[test]
fn context_finish_finalizes_failed_attachments_while_an_instance_holds_the_runtime() {
    let dir = tempfile::Builder::new()
        .prefix("oxpinyin-facade-missing-index-")
        .tempdir()
        .expect("temp dir");
    for entry in std::fs::read_dir(fixture_dir()).expect("w3 fixture") {
        let entry = entry.expect("w3 entry");
        // Everything but the main pinyin index (`SystemDbm::PinyinIndex`).
        if entry.file_name().as_os_str() != std::ffi::OsStr::new("pinyin_index.bin") {
            std::fs::copy(entry.path(), dir.path().join(entry.file_name())).expect("copy");
        }
    }
    let mut context = ContextCore::open(
        dir.path(),
        None,
        PINYIN_DEFAULT_OPTION_WORD,
        UserConfLaw::Pinyin,
    )
    .expect("the pin ignores a failed system attach");
    let instance = context.alloc_instance().expect("instance");
    {
        let runtime = &context.runtime;
        assert!(runtime.pinyin_attachment_failed());
        assert!(!runtime.failed_attachments_finished());
    }
    // Public context fini, with the instance still alive.
    context.finish();
    assert!(context.runtime.failed_attachments_finished());
    // A second fini is inert.
    context.finish();
    assert!(context.runtime.failed_attachments_finished());
    drop(instance);
    drop(context);
}
