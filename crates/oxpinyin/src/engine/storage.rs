//! Storage locations, injected as data.
//!
//! The engine never discovers a path. It does not read `XDG_DATA_HOME`,
//! `%APPDATA%` or `NSSearchPathForDirectoriesInDomains`, and it does not
//! depend on a directories crate. A shell knows where its platform puts
//! things; it says so here.

use std::path::PathBuf;

/// Where a session reads system data.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StoragePaths {
    system_data_dirs: Vec<PathBuf>,
}

impl StoragePaths {
    /// Builds paths with the system data directories, searched in the
    /// given order.
    #[must_use]
    pub fn new(system_data_dirs: Vec<PathBuf>) -> Self {
        Self { system_data_dirs }
    }

    /// System data directories, in search order.
    #[must_use]
    pub(crate) fn system_data_dirs(&self) -> &[PathBuf] {
        &self.system_data_dirs
    }
}
