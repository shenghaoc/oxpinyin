//! System and user directories whose names are not UTF-8 (#587). Generated from the frozen pin observations.

#[cfg(any(target_os = "linux", windows))]
use crate::bodies::non_utf8_directories as b;
#[cfg(any(target_os = "linux", windows))]
use crate::support::{Expect, NO_LOGS, run};

// non-utf8-directory-system: MATCH.
// Linux and Windows only: APFS refuses a directory name that is not UTF-8. On Windows no
// byte string spells a UTF-16 name, so that arm is the file system's refusal, not a pin
// observation.
#[cfg(any(target_os = "linux", windows))]
#[test]
fn non_utf8_directory_system() {
    run(
        "non_utf8_directories::non_utf8_directory_system",
        Expect::RESULT,
        |k| b::non_utf8_dirs(k, true, false),
        if cfg!(windows) {
            None
        } else {
            Some(include!(
                "fixtures/non_utf8_directories/d172c050e709a1ae.rs"
            ))
        },
        Some(NO_LOGS),
    );
}

// non-utf8-directory-user: MATCH.
// Linux and Windows only: APFS refuses a directory name that is not UTF-8. On Windows no
// byte string spells a UTF-16 name, so that arm is the file system's refusal, not a pin
// observation.
#[cfg(any(target_os = "linux", windows))]
#[test]
fn non_utf8_directory_user() {
    run(
        "non_utf8_directories::non_utf8_directory_user",
        Expect::RESULT,
        |k| b::non_utf8_dirs(k, false, true),
        if cfg!(windows) {
            None
        } else {
            Some(include!(
                "fixtures/non_utf8_directories/d172c050e709a1ae.rs"
            ))
        },
        Some(NO_LOGS),
    );
}
