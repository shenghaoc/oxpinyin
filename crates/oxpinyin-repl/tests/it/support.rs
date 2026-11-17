//! Scratch fixtures, invocation and byte-exact comparisons shared by REPL modes.
use crate::common::{
    CELL,
    expected::{File, NO_ROWS, Run, Sha256, check_repl},
    inventory, mask_glib_prefix, mask_print_time, replace_directory, snapshot_inputs,
};
use assert_cmd::cargo::cargo_bin_cmd;
use assert_fs::{TempDir, prelude::*};
use std::{path::Path, process::Output, time::Duration};

const EMPTY_PHRASE_CHUNK: File = Sha256(
    "4aeb3f46357bead0db5afe0f8eecd1ca263b389042546aa7fa724249e08ca697",
    27,
);
const KC_EMPTY_STASH_DB: File = Sha256(
    "574563c29dbe594eafe2e30ff1ac1e3fb92fed42bbffc40fa1090fb5270b2f05",
    7,
);
pub(super) const OPEN_DATA_USER_CONF_FAILED: &[u8] = b"open ../data/user.conf failed.";
const SESSION_GBK_CHAR_DBIN: File = Sha256(
    "c8399d91646c86e97b1cb2020c0668279e9ab2a674ae56a25016b72d36d19c6b",
    26,
);
const SESSION_GB_CHAR_DBIN: File = Sha256(
    "3b56b0634e338a0482092a117849be21ab4f39133201bf1fb2183d9485502537",
    26,
);
const SESSION_MERGED_DBIN: File = Sha256(
    "30923d4c45a569a8cfcb0a220f3e91b83904ce40af34145848ea46f0ef0f029a",
    26,
);
const SESSION_OPENGRAM_DBIN: File = Sha256(
    "2693d260ef5db322d24b8b3c2fd1776f10c497775a5855cde349202f41b96f61",
    26,
);
const USER_CONF_BERKELEY_DB: File = Sha256(
    "fbcac8c49db56a5a5a6ae546bf18bbbdc381a51629e4efdfd2c24b5dd4c7ec8a",
    88,
);
const USER_CONF_KYOTO_CABINET: File = Sha256(
    "ec73ecc4d02a875e4448a47d7051e5383bf43107b3386ecec12a82c94c5891f7",
    90,
);
const USER_CONF_TKRZW: File = Sha256(
    "38f302279f5500e5cd049f0d0230eaabc4c1b23ecdbb7b8bb57a088e203231f9",
    83,
);
pub(super) fn session_user_conf_files() -> &'static [(&'static str, File)] {
    match CELL {
        "bdb" => &[("data/user.conf", USER_CONF_BERKELEY_DB)],
        "kc" => &[("data/user.conf", USER_CONF_KYOTO_CABINET)],
        "tkrzw" => &[("data/user.conf", USER_CONF_TKRZW)],
        _ => unreachable!("unknown cell {CELL}"),
    }
}
pub(super) fn session_user_files() -> &'static [(&'static str, File)] {
    match CELL {
        "bdb" => &[
            ("data/addon.bin", EMPTY_PHRASE_CHUNK),
            ("data/gb_char.dbin", SESSION_GB_CHAR_DBIN),
            ("data/gbk_char.dbin", SESSION_GBK_CHAR_DBIN),
            ("data/merged.dbin", SESSION_MERGED_DBIN),
            ("data/network.bin", EMPTY_PHRASE_CHUNK),
            ("data/opengram.dbin", SESSION_OPENGRAM_DBIN),
            ("data/user.bin", EMPTY_PHRASE_CHUNK),
            ("data/user.conf", USER_CONF_BERKELEY_DB),
            ("data/user_bigram.db", NO_ROWS),
            ("data/user_phrase_index.bin", NO_ROWS),
            ("data/user_pinyin_index.bin", NO_ROWS),
        ],
        "kc" => &[
            ("data/addon.bin", EMPTY_PHRASE_CHUNK),
            ("data/gb_char.dbin", SESSION_GB_CHAR_DBIN),
            ("data/gbk_char.dbin", SESSION_GBK_CHAR_DBIN),
            ("data/merged.dbin", SESSION_MERGED_DBIN),
            ("data/network.bin", EMPTY_PHRASE_CHUNK),
            ("data/opengram.dbin", SESSION_OPENGRAM_DBIN),
            ("data/user.bin", EMPTY_PHRASE_CHUNK),
            ("data/user.conf", USER_CONF_KYOTO_CABINET),
            ("data/user_bigram.db", KC_EMPTY_STASH_DB),
            ("data/user_phrase_index.bin", KC_EMPTY_STASH_DB),
            ("data/user_pinyin_index.bin", KC_EMPTY_STASH_DB),
        ],
        "tkrzw" => &[
            ("data/addon.bin", EMPTY_PHRASE_CHUNK),
            ("data/gb_char.dbin", SESSION_GB_CHAR_DBIN),
            ("data/gbk_char.dbin", SESSION_GBK_CHAR_DBIN),
            ("data/merged.dbin", SESSION_MERGED_DBIN),
            ("data/network.bin", EMPTY_PHRASE_CHUNK),
            ("data/opengram.dbin", SESSION_OPENGRAM_DBIN),
            ("data/user.bin", EMPTY_PHRASE_CHUNK),
            ("data/user.conf", USER_CONF_TKRZW),
            ("data/user_bigram.db", NO_ROWS),
            ("data/user_phrase_index.bin", NO_ROWS),
            ("data/user_pinyin_index.bin", NO_ROWS),
        ],
        _ => unreachable!("unknown cell {CELL}"),
    }
}

pub(super) fn w3() -> TempDir {
    let data = crate::fixtures::w3();
    let directory = TempDir::new().expect("scratch directory");
    std::fs::rename(data.path(), directory.path().join("data")).expect("nested data directory");
    directory
}

pub(super) fn table_config() -> TempDir {
    let directory = TempDir::new().expect("scratch directory");
    directory
        .child("data")
        .create_dir_all()
        .expect("input directory");
    directory.child("data/table.conf").write_binary(b"binary format version:7\nmodel data version:14\nlambda parameter:0.5\nsource table format:pinyin\ndatabase format:Tkrzw\ndefault GB_DICTIONARY gb.table gb_char.bin gb_char.dbin SYSTEM_FILE\ndefault GBK_DICTIONARY gbk.table gbk_char.bin gbk_char.dbin SYSTEM_FILE\n").expect("table configuration");
    directory
        .child("data/gbk.table")
        .write_binary(b"ni3 \xe4\xbd\xa0 33554433 200\nhao3 \xe5\xa5\xbd 33554434 100\n")
        .expect("GBK table");
    directory
}

pub(super) fn tables() -> TempDir {
    let directory = table_config();
    directory.child("data/gb.table").write_binary(b"ni3 \xe4\xbd\xa0 16777217 100\nni2 \xe4\xbd\xa0 16777217 50\nni3'hao3 \xe4\xbd\xa0\xe5\xa5\xbd 16777218 80\nni3'hao3'ma5 \xe4\xbd\xa0\xe5\xa5\xbd\xe5\x90\x97 16777219 20\nni3 \xe7\xa5\xa2 16777221 40\n").expect("GB table");
    directory
}

fn capture(root: &Path, mode: &str, cwd: &Path, args: &[&str], stdin: &[u8]) -> Output {
    let data = root.join("data");
    let data_argument = if mode == "parser2" { root } else { &data };
    let mut output = cargo_bin_cmd!("oxpinyin-repl")
        .arg(mode)
        .arg(data_argument)
        .current_dir(cwd)
        .env("LC_ALL", "C")
        .args(args)
        .write_stdin(stdin)
        .timeout(Duration::from_secs(300))
        .output()
        .unwrap_or_else(|error| panic!("run {mode}: {error}"));
    if matches!(
        mode,
        "parser2" | "phrase_table" | "chewing_table" | "matrix" | "pinyin_lookup"
    ) {
        output.stdout = mask_print_time(&output.stdout);
    }
    // The REPL takes an explicit isolated data directory. Match the pin's
    // path spelling while preserving the diagnostic payload.
    output.stderr = replace_directory(&mask_glib_prefix(&output.stderr), &data, b"../data");
    output
}

pub(super) fn run_user_session(
    mode: &str,
    directory: &TempDir,
    args: &[&str],
    stdin: &[u8],
    expected: &Run,
) {
    let root = directory.path();
    let cwd = directory.child("tests");
    cwd.create_dir_all().expect("working directory");
    let inputs = snapshot_inputs(root);
    let output = capture(root, mode, cwd.path(), args, stdin);
    let dbm: &[&str] = if ["bdb", "tkrzw"].contains(&CELL) {
        &[
            "data/user_bigram.db",
            "data/user_pinyin_index.bin",
            "data/user_phrase_index.bin",
        ]
    } else {
        &[]
    };
    check_repl(
        output,
        &inventory(root, &inputs, dbm, &[], &[]),
        &inputs,
        expected,
    );
}

pub(super) fn run_table_or_lookup(
    mode: &str,
    directory: &TempDir,
    args: &[&str],
    stdin: &[u8],
    directories: &[&str],
    files: &[(&str, &[u8])],
    expected: &Run,
) {
    let working_directory = if mode.ends_with("_lookup") {
        "tests/lookup"
    } else {
        "tests/storage"
    };
    let cwd = directory.child(working_directory);
    cwd.create_dir_all().expect("working directory");
    for name in directories {
        directory
            .child(name)
            .create_dir_all()
            .expect("input directory");
    }
    for (name, bytes) in files {
        directory
            .child(name)
            .write_binary(bytes)
            .expect("input bytes");
    }
    let root = directory.path();
    let inputs = snapshot_inputs(root);
    let output = capture(root, mode, cwd.path(), args, stdin);
    check_repl(
        output,
        &inventory(root, &inputs, &[], &[], &[]),
        &inputs,
        expected,
    );
}

pub(super) fn run_parser(args: &[&str], stdin: &[u8], expected: &Run) {
    let directory = TempDir::new().expect("scratch directory");
    let root = directory.path();
    let inputs = snapshot_inputs(root);
    let output = capture(root, "parser2", root, args, stdin);
    check_repl(
        output,
        &inventory(root, &inputs, &[], &[], &[]),
        &inputs,
        expected,
    );
}
