//! The glib_path_construction family's case bodies: each answers the result document the old case returned.
//!
//! GLib path construction (inventory row 53, #643, #922): `g_build_filename`
//! drops empty elements and stops at NULL; repeated, leading, nested and
//! dotted separators in the directory arguments and the `table.conf` file
//! names reach the pin's joins unchanged.

use std::path::{Path, PathBuf};

use crate::facade;
use crate::fixtures::{link_file, private_system};
use crate::readers::c;
use crate::support::{J, Kit, P, os_text};

/// The case body for `id`.
pub fn case(k: &mut Kit, id: &str) -> J {
    let suffix = if facade::ZHUYIN { "-zhuyin" } else { "-pinyin" };
    let stem = id
        .strip_suffix(suffix)
        .unwrap_or_else(|| panic!("{id} is not a case of this facade"));
    if let Some(form) = stem.strip_prefix("init-system-") {
        return init_system_argument(k, form);
    }
    match stem {
        "glib-922-dir-system-dup-trailing" => dir_spelling_session(k, Some("dup-trailing"), None),
        "glib-922-dir-both-leading-dup" => {
            dir_spelling_session(k, Some("leading-dup"), Some("leading-dup"))
        }
        "glib-922-dir-both-dot-segments" => {
            dir_spelling_session(k, Some("dot-segments"), Some("dot-segments"))
        }
        "glib-922-dir-both-dotdot" => dir_spelling_session(k, Some("dotdot"), Some("dotdot")),
        "glib-922-dir-user-dup-trailing" => dir_spelling_session(k, None, Some("dup-trailing")),
        "glib-922-save-write-failed-dup-trailing" => save_write_failed(k),
        "glib-922-fini-write-failed-dup-trailing" => fini_write_failed(k),
        "glib-922-row-missing-leading-slash" => row_spelling_missing(k, "leading-slash", None),
        "glib-922-row-missing-nested" => row_spelling_missing(k, "nested", Some("dup-trailing")),
        "glib-922-row-missing-dup-trailing-dir" => {
            row_spelling_missing(k, "leading-slash", Some("dup-trailing"))
        }
        "glib-922-table-conf-rejected-dup-trailing" => table_conf_rejected(k),
        other => {
            if let Some(form) = other.strip_prefix("glib-922-row-") {
                return row_spelling_session(k, form);
            }
            let label = other
                .strip_prefix("glib-922-table-conf-missing-")
                .unwrap_or_else(|| panic!("no glib_path_construction case {id}"));
            let system: &[u8] = match label {
                "slash" => b"/",
                "slashes" => b"//",
                "many-slashes" => b"////",
                "missing-dup-trailing" => b"/nonexistent-922-system-dir///",
                _ => panic!("no glib_path_construction case {id}"),
            };
            table_conf_line(k, system)
        }
    }
}

/// A path's bytes (the old harness's `os.fsencode`).
fn bytes_of(path: &Path) -> Vec<u8> {
    path.to_str().expect("a UTF-8 path").as_bytes().to_vec()
}

/// A path spelled by `bytes`.
fn path_of(bytes: &[u8]) -> PathBuf {
    PathBuf::from(std::str::from_utf8(bytes).expect("a UTF-8 spelling"))
}

/// `spell_dir`: the bytes of `path` spelled as `form` names it; `None` leaves them as they are.
fn spell_dir(form: Option<&str>, path: &Path) -> PathBuf {
    let bytes = bytes_of(path);
    let spelled = match form {
        None => bytes,
        Some("dup-trailing") => [&bytes[..], b"///"].concat(),
        Some("leading-dup") => [&b"/"[..], &bytes].concat(),
        Some("dot-segments") => [&bytes[..], b"/./"].concat(),
        Some("dotdot") => {
            let base = path.file_name().expect("a base name");
            [&bytes[..], b"/../", os_text(base).as_bytes()].concat()
        }
        Some(other) => panic!("no directory spelling {other}"),
    };
    path_of(&spelled)
}

/// The sorted names in `directory` (`sorted(os.listdir(...))`).
fn listing(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(directory)
        .expect("the listed directory")
        .map(|entry| os_text(&entry.expect("a listed entry").file_name()))
        .collect();
    names.sort();
    names
}

/// `LIBRARY_FACADES`' parse call.
fn parse(inst: P, text: &[u8]) -> usize {
    if facade::ZHUYIN {
        facade::parse_more_chewings(inst, c(text).as_ptr())
    } else {
        facade::parse_more_full_pinyins(inst, c(text).as_ptr())
    }
}

/// `LIBRARY_FACADES`' text for the parse call.
fn library_text() -> &'static [u8] {
    if facade::ZHUYIN { b"su3cl3" } else { b"nihao" }
}

/// `train_once`: a fresh instance of `ctx`, parsed and guessed, trained once
/// at rank zero; the instance is left live.
fn train_once(ctx: P) -> bool {
    let inst = facade::alloc_instance(ctx);
    parse(inst, library_text());
    facade::guess_sentence(inst);
    facade::train_top(inst)
}

/// `init_system_argument`: initialise with a literal empty, NULL or absolute
/// system argument from a temporary working directory under the scratch
/// directory (`empty-missing` links no system files into it).
fn init_system_argument(k: &mut Kit, form: &str) -> J {
    let cwd = k.scratch.join("sys-X");
    std::fs::create_dir(&cwd).expect("the temporary working directory");
    if form != "empty-missing" {
        for entry in std::fs::read_dir(&k.data).expect("system data") {
            let entry = entry.expect("system entry");
            if entry.path().is_file() {
                link_file(&entry.path(), &cwd.join(entry.file_name()));
            }
        }
    }
    std::env::set_current_dir(&cwd).expect("chdir");
    let system = match form {
        "empty" | "empty-missing" => Some(PathBuf::new()),
        "null" => None,
        "absolute" => Some(cwd.clone()),
        other => panic!("no system argument form {other}"),
    };
    let ctx = k.init(system.as_deref(), Some(&k.user));
    let mut out = obj! { "init" => !ctx.is_null() };
    if !ctx.is_null() {
        let inst = facade::alloc_instance(ctx);
        assert!(!inst.is_null(), "alloc_instance failed");
        let text: &[u8] = if facade::ZHUYIN {
            b"su3cl3"
        } else {
            b"ni3hao3"
        };
        out.set("parse", parse(inst, text));
        out.set("guess", facade::guess_sentence(inst));
        facade::free_instance(inst);
        facade::fini(ctx);
    }
    out.set("files", listing(&k.user));
    std::env::set_current_dir(&k.scratch).expect("chdir");
    std::fs::remove_dir_all(&cwd).expect("remove the temporary working directory");
    out
}

/// `dir_spelling_session`: open, train, save and reopen with the system and
/// user directories spelled as asked.
fn dir_spelling_session(k: &mut Kit, system_form: Option<&str>, user_form: Option<&str>) -> J {
    let system = spell_dir(system_form, &k.data);
    let user = spell_dir(user_form, &k.user);
    let ctx = k.init(Some(&system), Some(&user));
    let mut out = obj! { "init" => !ctx.is_null() };
    if !ctx.is_null() {
        out.set("train", train_once(ctx));
        out.set("save", facade::save(ctx));
        facade::fini(ctx);
        let again = k.init(Some(&system), Some(&user));
        out.set("reopen", !again.is_null());
        if !again.is_null() {
            facade::fini(again);
        }
    }
    out.set("files", listing(&k.user));
    out
}

/// `fini_write_failed`: settle a profile, reopen it with the user directory
/// ending in `///`, remove it, then finalise.
fn fini_write_failed(k: &mut Kit) -> J {
    let first = k.init_default();
    assert!(!first.is_null(), "init failed");
    facade::fini(first);
    let ctx = k.init(
        Some(&k.data),
        Some(&spell_dir(Some("dup-trailing"), &k.user)),
    );
    let out = obj! { "init" => !ctx.is_null() };
    std::fs::remove_dir_all(&k.user).expect("remove the user directory");
    facade::fini(ctx);
    out
}

/// `save_write_failed`: open with the user directory ending in `///`, train,
/// remove it, then save and finalise.
fn save_write_failed(k: &mut Kit) -> J {
    let ctx = k.init(
        Some(&k.data),
        Some(&spell_dir(Some("dup-trailing"), &k.user)),
    );
    let mut out = obj! { "init" => !ctx.is_null() };
    out.set("train", train_once(ctx));
    std::fs::remove_dir_all(&k.user).expect("remove the user directory");
    out.set("save", facade::save(ctx));
    facade::fini(ctx);
    out
}

/// `ROW_SPELLINGS`: a `table.conf` file name spelled as `form` names it.
fn row_spelling(form: &str, name: &str) -> String {
    match form {
        "leading-slash" => format!("/{name}"),
        "double-leading-slash" => format!("//{name}"),
        "nested" => format!("sub//{name}"),
        "dotdot" => format!("sub/../{name}"),
        other => panic!("no row spelling {other}"),
    }
}

/// `spelled_system`: a private system directory whose `table.conf` names
/// library `name` as `form` spells it; `nested` also links it under `sub`.
fn spelled_system(k: &Kit, form: &str, name: &str) -> PathBuf {
    let system = private_system(k);
    let conf = system.join("table.conf");
    let text = std::fs::read_to_string(&conf).expect("table.conf");
    let mut words: Vec<String> = text.split(' ').map(str::to_owned).collect();
    let at = words
        .iter()
        .position(|word| word == name)
        .unwrap_or_else(|| panic!("table.conf names no {name}"));
    words[at] = row_spelling(form, name);
    std::fs::create_dir(system.join("sub")).expect("sub");
    if form == "nested" {
        link_file(&k.data.join(name), &system.join("sub").join(name));
    }
    std::fs::remove_file(&conf).expect("unlink table.conf");
    std::fs::write(&conf, words.join(" ")).expect("write table.conf");
    system
}

/// `row_spelling_session`: initialise against a library named as `form`
/// spells it and parse one sentence.
fn row_spelling_session(k: &mut Kit, form: &str) -> J {
    let system = spelled_system(k, form, "gb_char.bin");
    let ctx = k.init(Some(&system), Some(&k.user));
    let mut out = obj! { "init" => !ctx.is_null() };
    if !ctx.is_null() {
        let inst = facade::alloc_instance(ctx);
        out.set("parse", parse(inst, library_text()));
        out.set("guess", facade::guess_sentence(inst));
        facade::free_instance(inst);
        facade::fini(ctx);
    }
    out
}

/// `row_spelling_missing`: initialise where the library `table.conf` spells as
/// `form` is missing, the directory spelled as `system_form`.
fn row_spelling_missing(k: &mut Kit, form: &str, system_form: Option<&str>) -> J {
    let name = "merged.bin";
    let system = spelled_system(k, form, name);
    let spelled = row_spelling(form, name);
    std::fs::remove_file(system.join(spelled.trim_start_matches('/'))).expect("unlink the library");
    let ctx = k.init(Some(&spell_dir(system_form, &system)), Some(&k.user));
    obj! { "ctx" => !ctx.is_null() }
}

/// `table_conf_line`: initialise with the literal system argument `system`.
fn table_conf_line(k: &mut Kit, system: &[u8]) -> J {
    let ctx = k.init(Some(&path_of(system)), Some(&k.user));
    obj! { "ctx" => !ctx.is_null() }
}

/// `table_conf_line(..., rejected=True)`: a scratch system directory holding a
/// `table.conf` that is not one, named with a repeated trailing separator.
fn table_conf_rejected(k: &mut Kit) -> J {
    let directory = k.scratch.join("sys-X");
    std::fs::create_dir(&directory).expect("the system directory");
    std::fs::write(directory.join("table.conf"), "not a table.conf\n").expect("table.conf");
    table_conf_line(k, &bytes_of(&spell_dir(Some("dup-trailing"), &directory)))
}
