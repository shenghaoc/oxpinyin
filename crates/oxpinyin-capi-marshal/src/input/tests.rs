use std::path::PathBuf;

use super::filename;

#[test]
fn filenames_keep_null_empty_and_lossy_utf8_handling() {
    assert_eq!(filename(None), None);
    assert_eq!(filename(Some(c"")), Some(PathBuf::new()));
    let input = c"\xff";
    assert_eq!(filename(Some(input)), Some(PathBuf::from("\u{fffd}")));
}
