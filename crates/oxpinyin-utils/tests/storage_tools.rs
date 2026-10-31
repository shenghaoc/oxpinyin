//! The three storage utilities on the committed W3 mini data
//! (`fixtures/w3/tkt/`): the files the pin opens from its cwd, byte
//! expectations derived from the data layer's written formats, and the
//! pin's messages, exit codes and stdin framing.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture() -> tempfile::TempDir {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/w3/tkt");
    let dir = tempfile::Builder::new()
        .prefix("oxpinyin-storage-tools-")
        .tempdir()
        .expect("scratch dir");
    for entry in fs::read_dir(&src).expect("fixture dir") {
        let entry = entry.expect("fixture entry");
        if entry.path().is_file() {
            fs::copy(entry.path(), dir.path().join(entry.file_name())).expect("copy fixture");
        }
    }
    dir
}

fn run(binary: &str, dir: &Path, args: &[&str], stdin: &[u8]) -> Output {
    use std::io::Write as _;
    use std::process::Stdio;
    let mut child = Command::new(binary)
        .current_dir(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn utility");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(stdin)
        .expect("write stdin");
    child.wait_with_output().expect("utility output")
}

fn export_bin() -> &'static str {
    env!("CARGO_BIN_EXE_export_interpolation")
}

fn gen_bin() -> &'static str {
    env!("CARGO_BIN_EXE_gen_binary_files")
}

fn import_bin() -> &'static str {
    env!("CARGO_BIN_EXE_import_interpolation")
}

/// The minimal `table.conf` these tests use: one system library and no
/// addon rows, so the run touches only the files the case names.
const TABLE_CONF: &str = "binary format version:7\n\
model data version:14\n\
lambda parameter:0.312699\n\
\n\
source table format:pinyin\n\
database format:Tkrzw\n\
\n\
default RESERVED NULL NULL NULL NOT_USED\n\
default GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE\n\
default GBK_DICTIONARY NULL NULL gbk_char.dbin USER_FILE\n\
default OPENGRAM_DICTIONARY NULL NULL opengram.dbin USER_FILE\n\
default MERGED_DICTIONARY NULL NULL merged.dbin USER_FILE\n";

const GB_CHAR: &str = "a\t吖\t16777217\t104\na\t啊\t16777218\t5\n\
ni\t你\t16777219\t7\nni'hao\t你好\t16777220\t9\n\
zhong'guo\t中国\t16777221\t3\n";

#[test]
fn export_mini_fixture_writes_the_model_and_reads_only() {
    let dir = fixture();
    let before: Vec<(PathBuf, Vec<u8>)> = fixture_files(dir.path());
    let output = run(export_bin(), dir.path(), &[], b"");
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    let text = String::from_utf8(output.stdout).expect("UTF-8 model");
    assert!(text.starts_with("\\data model interpolation\n\\1-gram\n"));
    assert!(text.ends_with("\\end\n"));
    let (head, rest) = text.split_once("\\2-gram\n").expect("bigram section");
    // Every unigram line is `\item <token> <phrase> count <count>`.
    for line in head.lines().skip(2) {
        let fields: Vec<&str> = line.split(' ').collect();
        assert_eq!(fields.len(), 5, "{line}");
        assert_eq!(fields[0], "\\item");
        assert_eq!(fields[3], "count");
        assert!(fields[1].parse::<u32>().is_ok(), "{line}");
        assert!(fields[4].parse::<u32>().is_ok(), "{line}");
    }
    // The bigram section prints only resolved pairs.
    assert!(
        rest.lines()
            .all(|line| line.starts_with("\\item ") || line.is_empty() || line == "\\end")
    );
    // Read-only: every input file is unchanged.
    assert_eq!(fixture_files(dir.path()), before);
}

fn fixture_files(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files: Vec<(PathBuf, Vec<u8>)> = fs::read_dir(dir)
        .expect("read dir")
        .map(|entry| {
            let entry = entry.expect("entry");
            (
                entry.file_name().into(),
                fs::read(entry.path()).expect("read"),
            )
        })
        .collect();
    files.sort();
    files
}

#[test]
fn export_missing_table_conf_is_enoent() {
    let dir = tempfile::tempdir().expect("scratch");
    let output = run(export_bin(), dir.path(), &[], b"");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        output.stderr,
        b"open table.conf failed.\nload table.conf failed.\n"
    );
    assert!(output.stdout.is_empty());
}

#[test]
fn import_empty_stdin_is_enodata() {
    let dir = fixture();
    let before: Vec<(PathBuf, Vec<u8>)> = chunk_files(dir.path());
    let output = run(import_bin(), dir.path(), &[], b"");
    assert_eq!(output.status.code(), Some(61));
    assert_eq!(output.stderr, b"empty file input.\n");
    assert_eq!(chunk_files(dir.path()), before);
}

#[test]
fn import_bad_header_is_enodata() {
    let dir = fixture();
    let output = run(import_bin(), dir.path(), &[], b"garbage\n");
    assert_eq!(output.status.code(), Some(61));
    assert_eq!(output.stderr, b"error: interpolation model expected.\n");
}

#[test]
fn import_missing_phrase_index_is_enoent() {
    let dir = tempfile::tempdir().expect("scratch");
    fs::write(dir.path().join("table.conf"), TABLE_CONF).expect("write conf");
    let output = run(import_bin(), dir.path(), &[], b"");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output.stderr, b"open phrase_index.bin failed!\n");
}

#[test]
fn import_applies_unigram_and_bigram() {
    let dir = fixture();
    let stdin = "\\data model interpolation\n\
                 \\1-gram\n\
                 \\item 16777217 锕 count 5\n\
                 \\item 1 <start> count 2\n\
                 \\2-gram\n\
                 \\item 16777217 锕 16777218 吖 count 4\n\
                 \\end\n";
    let output = run(import_bin(), dir.path(), &[], stdin.as_bytes());
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);

    let library =
        oxpinyin_data::phrase_library::PhraseLibrary::open(&dir.path().join("gb_char.bin"))
            .expect("chunk");
    // The fixture's committed unigram for 16777217 is 3; import adds 5.
    assert_eq!(library.item(16_777_217).expect("item").unigram(), 8);

    let bigram = oxpinyin_data::BigramTable::open(&dir.path().join("bigram.db")).expect("bigram");
    let row = bigram
        .load_successors(16_777_217)
        .expect("load")
        .expect("group");
    assert_eq!(row.total, 4);
    assert_eq!(row.records, vec![(16_777_218, 4)]);
}

#[test]
fn gen_binary_files_builds_the_indexes_and_chunks() {
    let dir = tempfile::tempdir().expect("scratch");
    fs::write(dir.path().join("table.conf"), TABLE_CONF).expect("write conf");
    fs::write(dir.path().join("gb_char.table"), GB_CHAR).expect("write table");
    let output = run(gen_bin(), dir.path(), &[], b"");
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);

    // The chunk is `SubPhraseIndex::store`'s bytes: unigram 0 at
    // gen_binary_files time, the `.table` count as each pronunciation's
    // frequency, and the phrases in ascending token order.
    let item = |phrase: &[u32], prons: &[(&str, u32)]| oxpinyin_data::chunk_write::ChunkItem {
        phrase: phrase.to_vec(),
        unigram: 0,
        prons: prons
            .iter()
            .map(|(pinyin, freq)| {
                let keys: Vec<u16> = pinyin
                    .split(['\'', ' '])
                    .map(|syllable| {
                        oxpinyin_core::ChewingKey::from_pinyin(syllable)
                            .expect("syllable")
                            .to_packed()
                    })
                    .collect();
                (keys, *freq)
            })
            .collect(),
    };
    let items = vec![
        (1, item(&[0x5416], &[("a", 104)])),
        (2, item(&[0x554a], &[("a", 5)])),
        (3, item(&[0x4f60], &[("ni", 7)])),
        (4, item(&[0x4f60, 0x597d], &[("ni'hao", 9)])),
        (5, item(&[0x4e2d, 0x56fd], &[("zhong'guo", 3)])),
    ];
    let want_chunk = oxpinyin_data::chunk_write::build_chunk(&items).expect("chunk");
    assert_eq!(
        fs::read(dir.path().join("gb_char.bin")).expect("chunk"),
        want_chunk
    );

    // The phrase index stores each phrase's token and every proper prefix
    // as an empty continuation marker.
    use oxpinyin_store::{RawReadStore as _, ReadStore as _};
    let store = oxpinyin_store::DefaultStore::open_read_only(&dir.path().join("phrase_index.bin"))
        .expect("phrase index");
    let mut value = Vec::new();
    let ni_hao = [0x4f60_u32, 0x597d]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect::<Vec<u8>>();
    assert!(store.get_raw(&ni_hao, &mut value).expect("get"));
    assert_eq!(value, 16_777_220_u32.to_le_bytes());
    let ni = 0x4f60_u32.to_le_bytes();
    let mut marker = Vec::new();
    assert!(store.get_raw(&ni, &mut marker).expect("get"));
    assert_eq!(marker, 16_777_219_u32.to_le_bytes());
    drop(store);

    // Re-running appends nothing: the index rows and the chunk are the
    // same, because the records already exist.
    let before_chunk = fs::read(dir.path().join("gb_char.bin")).expect("chunk");
    let output = run(gen_bin(), dir.path(), &[], b"");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        fs::read(dir.path().join("gb_char.bin")).expect("chunk"),
        before_chunk
    );
}

#[test]
fn gen_missing_table_file_is_enoent() {
    let dir = tempfile::tempdir().expect("scratch");
    fs::write(dir.path().join("table.conf"), TABLE_CONF).expect("write conf");
    let output = run(gen_bin(), dir.path(), &[], b"");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output.stderr, b"open gb_char.table failed!\n");
    // The pin attaches both index files before it opens the table.
    assert!(dir.path().join("pinyin_index.bin").is_file());
    assert!(dir.path().join("phrase_index.bin").is_file());
}

#[test]
fn gen_help_lists_the_pin_entries() {
    let dir = tempfile::tempdir().expect("scratch");
    let output = run(gen_bin(), dir.path(), &["--help"], b"");
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    let text = String::from_utf8(output.stdout).expect("help");
    assert!(text.contains("- generate binary files"), "{text}");
    assert!(text.contains("--table-dir"), "{text}");
    assert!(text.contains("--gen-punct-table"), "{text}");
}

fn chunk_files(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files: Vec<(PathBuf, Vec<u8>)> =
        ["gb_char.bin", "gbk_char.bin", "opengram.bin", "merged.bin"]
            .iter()
            .map(|name| {
                (
                    PathBuf::from(name),
                    fs::read(dir.join(name)).expect("chunk"),
                )
            })
            .collect();
    files.sort();
    files
}
