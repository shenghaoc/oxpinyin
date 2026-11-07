//! The shared comparison and the narrow masks that every frozen case uses.
#![cfg(unix)]
mod common;
use common::expected::*;
use common::*;
use oxpinyin_store::{DefaultStore, WriteStore as _, WriteTxn as _};
use std::os::unix::process::ExitStatusExt as _;
use std::path::Path;
use std::process::{ExitStatus, Output};

#[test]
fn print_time_masks_only_the_clock() {
    let line = b"Spent 12 us for 7 operations, 1.500000 us/op, 666666.000000 times/s.\n\nrest\n";
    assert_eq!(
        mask_print_time(line),
        b"Spent <TIME> us for 7 operations, <TIME> us/op, <TIME> times/s.\n\nrest\n"
    );
    let other = b"Spent 12 us for 7 operations\n";
    assert_eq!(mask_print_time(other), other);
}

#[test]
fn glib_prefix_masks_only_pid_and_time() {
    assert_eq!(
        mask_glib_prefix(b"** (ngseg:123): CRITICAL **: 12:00:01.250: text 99\n"),
        b"** (ngseg:PID): CRITICAL **: TIME: text 99\n"
    );
    assert_eq!(
        mask_glib_prefix(b"(ngseg:123): GLib-WARNING **: 12:00:01.250: x\n"),
        b"(ngseg:PID): GLib-WARNING **: TIME: x\n"
    );
    let other = b"ngseg:123: text\n";
    assert_eq!(mask_glib_prefix(other), other);
}

#[test]
fn pinyin_index_mask_clears_only_odd_item_padding() {
    let mut odd = [1, 2, 3, 4, 5, 6, 0xaa, 0xbb];
    mask_pinyin_index_row(&[9, 9], &mut odd).expect("one key");
    assert_eq!(odd, [1, 2, 3, 4, 5, 6, 0, 0]);
    let mut even = [1, 2, 3, 4, 5, 6, 7, 8];
    mask_pinyin_index_row(&[9, 9, 9, 9], &mut even).expect("two keys");
    assert_eq!(even, [1, 2, 3, 4, 5, 6, 7, 8]);
    assert!(mask_pinyin_index_row(&[9], &mut []).is_err());
    assert!(mask_pinyin_index_row(&[9, 9], &mut [0; 7]).is_err());
}

#[test]
fn only_the_harness_directory_returns_to_tmp() {
    assert_eq!(
        undo_tmp_override(
            b"write /run/case/tmp/user.conf failed.\n/run/other/tmp/x\n",
            Path::new("/run/case/tmp")
        ),
        b"write /tmp/user.conf failed.\n/run/other/tmp/x\n"
    );
}

#[test]
fn tkrzw_mask_clears_only_the_clock_of_a_hash_container() {
    let mut data = vec![7; 128];
    data[..9].copy_from_slice(b"TkrzwHDB\n");
    let mut masked = data.clone();
    mask_tkrzw_timestamp(&mut masked);
    data[48..56].fill(0);
    assert_eq!(masked, data);
    let mut other = vec![7; 128];
    mask_tkrzw_timestamp(&mut other);
    assert_eq!(other, vec![7; 128]);
}

#[test]
fn tree_and_hash_rows_keep_keys_values_and_empty_values() {
    for hash in [false, true] {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("records.db");
        let store = if hash {
            DefaultStore::create_hash(&path).expect("hash store")
        } else {
            DefaultStore::create(&path).expect("tree store")
        };
        store
            .write(|txn| {
                txn.put_raw(&[2, 0], &[5, 6])?;
                txn.put_raw(&[1, 0], &[1, 2, 3, 4])?;
                txn.put_raw(&[3, 0], &[])
            })
            .expect("write");
        drop(store);
        assert_eq!(
            rows(&path).expect("rows"),
            [
                (vec![1, 0], vec![1, 2, 3, 4]),
                (vec![2, 0], vec![5, 6]),
                (vec![3, 0], vec![])
            ]
        );
    }
}

/// A run that leaves its tree alone and prints these bytes.
const PROBE: Run = Run {
    status: Exit(0),
    stdout: Exact(b"a\0"),
    stderr: b"\xffb",
    files: &[],
};

fn output(status: i32, stdout: &[u8], stderr: &[u8]) -> Output {
    Output {
        status: ExitStatus::from_raw(status),
        stdout: stdout.to_vec(),
        stderr: stderr.to_vec(),
    }
}

/// Whether `check` refuses the run.
fn refuses(output: Output, files: &str, before: &Snapshot, expected: &Run) -> bool {
    std::panic::catch_unwind(|| check(output, files, before, expected)).is_err()
}

/// A scratch tree holding `kept` (bytes `kept`) and the directory `held`.
fn tree() -> (tempfile::TempDir, Snapshot) {
    let directory = tempfile::tempdir().expect("temp dir");
    std::fs::write(directory.path().join("kept"), b"kept").expect("file");
    std::fs::create_dir(directory.path().join("held")).expect("directory");
    let before = snapshot_inputs(directory.path());
    (directory, before)
}

fn after(directory: &Path, before: &Snapshot) -> String {
    inventory(directory, before, &["rows.db"], &[], &[], &[])
}

#[rstest::rstest]
#[case::exact(0, b"a\0", b"\xffb", false)]
#[case::exit_code(7 << 8, b"a\0", b"\xffb", true)]
#[case::signal_instead_of_exit(6, b"a\0", b"\xffb", true)]
#[case::stdout_byte(0, b"a\x01", b"\xffb", true)]
#[case::stderr_byte(0, b"a\0", b"\xfeb", true)]
fn status_and_streams_are_exact(
    #[case] status: i32,
    #[case] stdout: &[u8],
    #[case] stderr: &[u8],
    #[case] rejects: bool,
) {
    let (directory, before) = tree();
    let files = after(directory.path(), &before);
    assert_eq!(
        refuses(output(status, stdout, stderr), &files, &before, &PROBE),
        rejects
    );
}

#[rstest::rstest]
#[case::exact_signal(6, false)]
#[case::wrong_signal(15, true)]
#[case::exit_instead_of_signal(6 << 8, true)]
fn signals_are_exact(#[case] status: i32, #[case] rejects: bool) {
    let expected = Run {
        status: Signal(6),
        ..PROBE
    };
    let (directory, before) = tree();
    let files = after(directory.path(), &before);
    assert_eq!(
        refuses(output(status, b"a\0", b"\xffb"), &files, &before, &expected),
        rejects
    );
}

/// Each way a run can change a tree that its expectation leaves alone.
#[rstest::rstest]
#[case::changed_file(|at: &Path| std::fs::write(at.join("kept"), b"kepT"), true)]
#[case::removed_file(|at: &Path| std::fs::remove_file(at.join("kept")), true)]
#[case::removed_directory(|at: &Path| std::fs::remove_dir(at.join("held")), true)]
#[case::new_file(|at: &Path| std::fs::write(at.join("held/new"), b""), true)]
#[case::new_directory(|at: &Path| std::fs::create_dir(at.join("new")), true)]
#[case::new_link(|at: &Path| std::os::unix::fs::symlink("kept", at.join("link")), true)]
fn a_read_only_case_keeps_the_whole_tree(
    #[case] change: fn(&Path) -> std::io::Result<()>,
    #[case] rejects: bool,
) {
    let (directory, before) = tree();
    change(directory.path()).expect("change");
    let files = after(directory.path(), &before);
    assert_eq!(
        refuses(output(0, b"a\0", b"\xffb"), &files, &before, &PROBE),
        rejects
    );
}

#[test]
fn a_changed_file_matches_only_its_digest_and_length() {
    let (directory, before) = tree();
    std::fs::write(directory.path().join("kept"), b"").expect("truncate");
    let files = after(directory.path(), &before);
    let empty = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    for (state, rejects) in [
        (Sha256(empty, 0), false),
        (Sha256(empty, 1), true),
        (Sha256(&empty[1..], 0), true),
    ] {
        let expected = Run {
            files: Box::leak(Box::new([("kept", state)])),
            ..PROBE
        };
        assert_eq!(
            refuses(output(0, b"a\0", b"\xffb"), &files, &before, &expected),
            rejects
        );
    }
}

#[test]
fn changed_dbm_rows_are_rejected() {
    let (directory, before) = tree();
    let path = directory.path().join("rows.db");
    let store = DefaultStore::create(&path).unwrap();
    store.write(|txn| txn.put_raw(b"key", b"original")).unwrap();
    drop(store);
    let files = after(directory.path(), &before);
    let digest = files
        .lines()
        .find_map(|line| line.strip_prefix("rows.db rows "))
        .expect("rows line");
    let expected = Run {
        files: Box::leak(Box::new([("rows.db", Rows(digest.to_owned().leak()))])),
        ..PROBE
    };
    assert!(!refuses(
        output(0, b"a\0", b"\xffb"),
        &files,
        &before,
        &expected
    ));
    let store = DefaultStore::create(&path).unwrap();
    store.write(|txn| txn.put_raw(b"key", b"originaL")).unwrap();
    drop(store);
    let files = after(directory.path(), &before);
    assert!(refuses(
        output(0, b"a\0", b"\xffb"),
        &files,
        &before,
        &expected
    ));
}

#[rstest::rstest]
#[case::frozen("Usage: [OPTION?] x\0", false)]
#[case::ellipsis("Usage: [OPTION\u{2026}] x\0", false)]
#[case::dots("Usage: [OPTION...] x\0", false)]
#[case::other_placeholder("Usage: [OPTIONS] x\0", true)]
#[case::changed_text("Usage: [OPTION...] y\0", true)]
fn glib_alternatives_are_exact_bytes(#[case] stdout: &str, #[case] rejects: bool) {
    let expected = Run {
        stdout: GlibOption("Usage: [OPTION?] x\0"),
        ..PROBE
    };
    let (directory, before) = tree();
    let files = after(directory.path(), &before);
    assert_eq!(
        refuses(
            output(0, stdout.as_bytes(), b"\xffb"),
            &files,
            &before,
            &expected
        ),
        rejects
    );
}
