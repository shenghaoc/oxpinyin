//! The fixed `/tmp` names of the pin's test programs.
//!
//! The pin's programs open absolute `/tmp` paths, and the ports open the same
//! names. An override lets the case harness give each run a private directory.
//! Only the never-shipped `test_*` programs call this.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Names the directory that replaces `/tmp`.
pub const OVERRIDE: &str = "OXPINYIN_TEST_TMP";

/// The program's fixed file `name`: under `/tmp`, unless [`OVERRIDE`] is set.
#[must_use]
pub fn path(name: &str) -> PathBuf {
    under(std::env::var_os(OVERRIDE), name)
}

fn under(directory: Option<OsString>, name: &str) -> PathBuf {
    directory
        .map_or_else(|| Path::new("/tmp").to_path_buf(), PathBuf::from)
        .join(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_the_pins_tmp() {
        assert_eq!(under(None, "test.db"), Path::new("/tmp/test.db"));
    }

    #[test]
    fn the_override_replaces_only_the_directory() {
        assert_eq!(
            under(Some("/private/run".into()), "test.db"),
            Path::new("/private/run/test.db")
        );
    }
}
