//! Storage locations, injected as data.
//!
//! The engine never discovers a path. It does not read `XDG_DATA_HOME`,
//! `%APPDATA%` or `NSSearchPathForDirectoriesInDomains`, and it does not
//! depend on a directories crate. A shell knows where its platform puts
//! things; it says so here.

use std::path::{Path, PathBuf};

/// Where a session reads system data and reads or writes user data.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StoragePaths {
    user_data_dir: PathBuf,
    system_data_dirs: Vec<PathBuf>,
}

impl StoragePaths {
    /// Builds paths with a user data directory and no system directories.
    #[must_use]
    pub fn new(user_data_dir: impl Into<PathBuf>) -> Self {
        Self {
            user_data_dir: user_data_dir.into(),
            system_data_dirs: Vec::new(),
        }
    }

    /// Adds system data directories, searched in the given order.
    #[must_use]
    pub fn with_system_dirs<I, P>(mut self, dirs: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.system_data_dirs = dirs.into_iter().map(Into::into).collect();
        self
    }

    /// Directory holding this user's data.
    #[must_use]
    pub fn user_data_dir(&self) -> &Path {
        &self.user_data_dir
    }

    /// System data directories, in search order.
    #[must_use]
    pub fn system_data_dirs(&self) -> &[PathBuf] {
        &self.system_data_dirs
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::StoragePaths;

    #[test]
    fn paths_are_kept_exactly_as_supplied() {
        let paths = StoragePaths::new("user").with_system_dirs(["a", "b"]);
        assert_eq!(paths.user_data_dir(), PathBuf::from("user"));
        assert_eq!(
            paths.system_data_dirs(),
            [PathBuf::from("a"), PathBuf::from("b")]
        );
    }

    #[test]
    fn the_default_has_no_directories() {
        let paths = StoragePaths::default();
        assert_eq!(paths.user_data_dir(), PathBuf::new());
        assert!(paths.system_data_dirs().is_empty());
    }
}
