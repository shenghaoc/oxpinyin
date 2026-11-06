//! The contract of the differential suites' shared pin-run helpers.

#[path = "common/differential.rs"]
mod differential;

use std::collections::BTreeSet;
use std::path::Path;

use differential::PinDir;

/// A source data dir shaped like a live trainer dir: raw tables plus
/// stale outputs and noise that `PinDir::fresh` must leave behind.
fn live_data_dir() -> tempfile::TempDir {
    let data = tempfile::Builder::new()
        .prefix("oxpinyin-pin-src-")
        .tempdir()
        .expect("source temp dir");
    for (name, body) in [
        ("table.conf", "conf"),
        ("gb_char.table", "gb"),
        ("merged.table", "merged"),
        ("phrase_index.bin", "stale"),
        ("bigram.db", "stale"),
        ("notes.txt", "noise"),
    ] {
        std::fs::write(data.path().join(name), body).expect("source file");
    }
    // A directory whose name passes the extension filter is not a file.
    std::fs::create_dir(data.path().join("nested.table")).expect("source subdir");
    data
}

fn names(dir: &Path) -> BTreeSet<String> {
    dir.read_dir()
        .expect("read pin dir")
        .map(|entry| {
            entry
                .expect("pin dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

#[test]
fn fresh_copies_exactly_the_tables_and_table_conf() {
    let data = live_data_dir();
    let pin = PinDir::fresh(data.path(), "select").expect("pin dir");
    assert_eq!(
        names(pin.dir.path()),
        BTreeSet::from(["gb_char.table", "merged.table", "table.conf"].map(String::from)),
    );
    for name in ["gb_char.table", "merged.table", "table.conf"] {
        assert_eq!(
            std::fs::read(pin.dir.path().join(name)).expect("copied file"),
            std::fs::read(data.path().join(name)).expect("source file"),
            "{name} is copied byte-for-byte",
        );
    }
}

#[test]
fn same_tag_dirs_are_distinct_while_live_and_removed_on_drop() {
    let data = live_data_dir();
    let first = PinDir::fresh(data.path(), "same").expect("first pin dir");
    let second = PinDir::fresh(data.path(), "same").expect("second pin dir");
    let first_path = first.dir.path().to_path_buf();
    let second_path = second.dir.path().to_path_buf();

    assert_ne!(first_path, second_path, "same tag, same process, two dirs");
    for path in [&first_path, &second_path] {
        let name = path.file_name().expect("dir name").to_string_lossy();
        assert!(name.starts_with("oxpinyin-pin-same-"), "{name}");
        assert!(path.join("table.conf").is_file(), "live dir keeps its copy");
    }

    drop(first);
    assert!(!first_path.exists(), "drop removes the dir");
    assert!(
        second_path.join("table.conf").is_file(),
        "the other survives"
    );
    drop(second);
    assert!(!second_path.exists(), "drop removes the dir");
}

#[cfg(unix)]
#[test]
fn run_executes_in_the_pin_dir_with_stdin_and_reports_failures() {
    let data = live_data_dir();
    let pin = PinDir::fresh(data.path(), "run").expect("pin dir");
    let sh = Path::new("/bin/sh");

    let stdout = pin
        .run(sh, &["-c", "cat table.conf; cat"], Some(b"|stdin"))
        .expect("command succeeds");
    assert_eq!(stdout, b"conf|stdin");

    let error = pin
        .run(sh, &["-c", "echo oops >&2; exit 3"], None)
        .expect_err("nonzero exit is an error");
    assert!(error.starts_with("/bin/sh exited "), "{error}");
    assert!(error.ends_with(": oops\n"), "{error}");
}
