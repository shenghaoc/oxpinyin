//! System and user directories whose names are not UTF-8 (#587).

use crate::facade;
use crate::readers::c;
use crate::support::Kit;

#[derive(Debug, PartialEq)]
pub struct Session {
    pub init: bool,
    pub guess: Option<bool>,
    pub train: Option<bool>,
    pub save: Option<bool>,
    pub user_files: Option<Vec<String>>,
}

/// The directories with a `0xFF` byte in their names, created under the
/// scratch directory, or `None` where the file system refuses such a name:
/// macOS's file systems require UTF-8 names (`EILSEQ`), and Windows names
/// are UTF-16, which no byte string spells.
#[cfg(unix)]
fn bad_dir(k: &Kit, prefix: &str, link_to: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    use std::os::unix::ffi::OsStrExt as _;
    let name = [
        prefix.as_bytes(),
        b"\xff",
        std::process::id().to_string().as_bytes(),
    ]
    .concat();
    let path = k.scratch.join(std::ffi::OsStr::from_bytes(&name));
    let made = match link_to {
        Some(target) => std::os::unix::fs::symlink(target, &path),
        None => std::fs::create_dir(&path),
    };
    match made {
        Ok(()) => Some(path),
        Err(error) if cfg!(target_os = "macos") && error.raw_os_error() == Some(libc::EILSEQ) => {
            None
        }
        Err(error) => panic!("create {}: {error}", path.display()),
    }
}

#[cfg(not(unix))]
fn bad_dir(
    _k: &Kit,
    _prefix: &str,
    _link_to: Option<&std::path::Path>,
) -> Option<std::path::PathBuf> {
    None
}

/// Opens the facade on the bad directories, then parses, guesses, trains and
/// saves; `None` where the platform cannot name such a directory.
pub fn non_utf8_dirs(k: &mut Kit, bad_system: bool, bad_user: bool) -> Option<Session> {
    let system = if bad_system {
        bad_dir(k, "system-", Some(&k.data.clone()))?
    } else {
        k.data.clone()
    };
    let user = if bad_user {
        bad_dir(k, "user-", None)?
    } else {
        k.user.clone()
    };
    let ctx = k.init(Some(&system), Some(&user));
    if ctx.is_null() {
        return Some(Session {
            init: false,
            guess: None,
            train: None,
            save: None,
            user_files: None,
        });
    }
    let inst = facade::alloc_instance(ctx);
    if facade::ZHUYIN {
        facade::parse_more_chewings(inst, c("su3cl3").as_ptr());
    } else {
        facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
    }
    let guess = facade::guess_sentence(inst);
    let train = facade::train_top(inst);
    let save = facade::save(ctx);
    let mut files: Vec<String> = std::fs::read_dir(&user)
        .expect("user directory")
        .map(|entry| {
            entry
                .expect("user entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| !name.starts_with('.'))
        .collect();
    files.sort();
    Some(Session {
        init: true,
        guess: Some(guess),
        train: Some(train),
        save: Some(save),
        user_files: Some(files),
    })
}
