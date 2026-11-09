//! The shared backend w3 fixture.
use assert_fs::{TempDir, prelude::*};
use rstest::fixture;
use std::path::Path;

/// Copy the compiled backend's w3 into a private temporary directory.
#[fixture]
pub fn w3() -> TempDir {
    let directory = TempDir::new().expect("scratch directory");
    directory
        .copy_from(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/w3")
                .join(oxpinyin_data::DEFAULT_STORE_EXT),
            &["**"],
        )
        .expect("backend w3 fixture");
    directory
}
