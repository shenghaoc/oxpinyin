//! Contract of the model-cache locator.
//!
//! The fetcher's own contract is `tools/dev/model.test.py`. These tests pin
//! the locator only. They do not open the language model, the scorer, or a
//! [`oxpinyin_engine::Session`].

use std::path::Path;

use pinyin_oracle::{
    EXPECTED_MODEL_FILES, ModelDirError, model_dir_is_complete, verified_model_dir,
};

/// A uniquely named scratch dir, removed on drop (including unwind).
struct Scratch(tempfile::TempDir);

impl Scratch {
    fn new(label: &str) -> Self {
        let dir = tempfile::Builder::new()
            .prefix(&format!("oxpinyin-model-{label}-"))
            .tempdir()
            .expect("scratch dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        self.0.path()
    }
}

fn write_complete_export(dir: &Path) {
    std::fs::create_dir_all(dir).expect("export dir");
    for name in EXPECTED_MODEL_FILES {
        std::fs::write(dir.join(name), b"placeholder").expect("export file");
    }
}

#[test]
fn verified_model_dir_accepts_the_eighteen_expected_files() {
    let scratch = Scratch::new("complete");
    write_complete_export(scratch.path());
    let path = verified_model_dir(scratch.path()).expect("complete export verifies");
    assert!(model_dir_is_complete(&path));
}

#[test]
fn verified_model_dir_rejects_a_partial_export() {
    let scratch = Scratch::new("partial");
    write_complete_export(scratch.path());
    std::fs::remove_file(scratch.path().join("interpolation2.text")).expect("remove model file");
    match verified_model_dir(scratch.path()) {
        Err(ModelDirError::Incomplete { missing, .. }) => {
            assert_eq!(missing, vec!["interpolation2.text"]);
        }
        other => panic!("expected Incomplete, got {other:?}"),
    }
}
