//! The pin's training utilities on the committed `fixtures/w3` data set
//! (`utils/training/*.cpp` at libpinyin 074a2219).
//!
//! Every test copies the compiled backend's directory
//! (`fixtures/w3/<DEFAULT_STORE_EXT>`) to a private temporary directory and
//! runs the binary in it, so the cwd-relative names are the pin's. The
//! expectations are derived from the pin's source and the frozen fixture
//! values; the ignored live differential at the end compares the same runs
//! with the pin-built binaries.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use oxpinyin_data::phrase_library::PhraseLibrary;
use oxpinyin_data::row_format::encode_token_key;
use oxpinyin_data::single_gram::decode_single_gram;
use oxpinyin_store::{DefaultStore, RawReadStore};

/// The repository root.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The compiled backend's fixture directory.
fn fixture() -> PathBuf {
    root()
        .join("fixtures/w3")
        .join(oxpinyin_data::DEFAULT_STORE_EXT)
}

/// A private copy of [`fixture`].
fn fixture_copy() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("temp dir");
    for entry in std::fs::read_dir(fixture()).expect("fixture dir") {
        let entry = entry.expect("fixture entry");
        if entry.file_type().expect("fixture file type").is_file() {
            std::fs::copy(entry.path(), temp.path().join(entry.file_name())).expect("copy fixture");
        }
    }
    temp
}

/// The binary named `name`, built for this feature set.
fn binary(name: &str) -> PathBuf {
    match name {
        "gen_unigram" => env!("CARGO_BIN_EXE_gen_unigram").into(),
        "gen_ngram" => env!("CARGO_BIN_EXE_gen_ngram").into(),
        "gen_deleted_ngram" => env!("CARGO_BIN_EXE_gen_deleted_ngram").into(),
        "estimate_interpolation" => env!("CARGO_BIN_EXE_estimate_interpolation").into(),
        "eval_correction_rate" => env!("CARGO_BIN_EXE_eval_correction_rate").into(),
        other => panic!("no such utility: {other}"),
    }
}

/// Runs `binary` in `dir` with `args` and `stdin`.
fn run(name: &str, dir: &Path, args: &[&str], stdin: &[u8]) -> Output {
    let mut child = Command::new(binary(name))
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn utility");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(stdin)
        .expect("write stdin");
    child.wait_with_output().expect("utility output")
}

/// `outcome.exit` as a status word: the exit code, or `-signal` for a
/// signal (the pin's shell status).
fn exit_word(outcome: &Output) -> i32 {
    use std::os::unix::process::ExitStatusExt as _;
    outcome
        .status
        .code()
        .unwrap_or_else(|| -outcome.status.signal().unwrap_or(0))
}

/// The one `table.conf` the success tests use: the fixture's four default
/// `SYSTEM_FILE` rows and the one addon dictionary the mini set ships.
const TABLE_CONF: &str = "\
binary format version:7
model data version:14
lambda parameter:0.312699

source table format:pinyin
database format:Tkrzw

default RESERVED NULL NULL NULL NOT_USED
default GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE
default GBK_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin SYSTEM_FILE
default OPENGRAM_DICTIONARY opengram.table opengram.bin opengram.dbin SYSTEM_FILE
default MERGED_DICTIONARY merged.table merged.bin merged.dbin SYSTEM_FILE
default ADDON_DICTIONARY NULL NULL addon.bin USER_FILE
default NETWORK_DICTIONARY NULL NULL network.bin USER_FILE
default USER_DICTIONARY NULL NULL user.bin USER_FILE

addon 4 art.table art.bin NULL DICTIONARY
";

/// Writes [`TABLE_CONF`] into `dir`.
fn write_table_conf(dir: &Path) {
    std::fs::write(dir.join("table.conf"), TABLE_CONF).expect("table.conf");
}

/// Writes a `table.conf` whose first default chunk is absent.
fn write_missing_chunk_conf(dir: &Path) {
    std::fs::write(
        dir.join("table.conf"),
        "binary format version:7\n\
         model data version:14\n\
         lambda parameter:0.312699\n\
         \n\
         source table format:pinyin\n\
         database format:Tkrzw\n\
         \n\
         default RESERVED NULL NULL NULL NOT_USED\n\
         default GB_DICTIONARY gb_char.table absent.bin gb_char.dbin SYSTEM_FILE\n\
         default GBK_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin SYSTEM_FILE\n",
    )
    .expect("table.conf");
}

/// One chunk's items as `(slot, unigram, total)`.
fn chunk_items(path: &Path) -> Vec<(u32, u32, u32)> {
    let library = PhraseLibrary::open(path).expect("chunk opens");
    let mut items: Vec<(u32, u32, u32)> = library
        .items()
        .map(|(slot, item)| (slot, item.unigram(), item.phrase_length() as u32))
        .collect();
    items.sort_by_key(|(slot, _, _)| *slot);
    items
}

/// One chunk's stored total and item count.
fn chunk_total_and_count(path: &Path) -> (u32, usize) {
    let library = PhraseLibrary::open(path).expect("chunk opens");
    (library.total_freq(), library.items().count())
}

/// One `SingleGram` row as `(total, records)`; the row must exist.
fn bigram_row(path: &Path, token: u32) -> (u32, Vec<(u32, u32)>) {
    let store = DefaultStore::open_hash_read_only(path).expect("bigram opens");
    let mut value = Vec::new();
    assert!(
        store
            .get_bigram_raw(&encode_token_key(token), &mut value)
            .expect("bigram read"),
        "bigram row {token} is present"
    );
    decode_single_gram(&value).expect("single gram")
}

/// Whether the row exists at all.
fn has_bigram_row(path: &Path, token: u32) -> bool {
    let store = DefaultStore::open_hash_read_only(path).expect("bigram opens");
    let mut value = Vec::new();
    store
        .get_bigram_raw(&encode_token_key(token), &mut value)
        .expect("bigram read")
}

const A: u32 = 0x0100_0003; // 阿
const AH: u32 = 0x0100_0004; // 啊
const START: u32 = 1;

#[test]
fn gen_unigram_adds_one_and_saves_system_and_dictionary_chunks() {
    let dir = fixture_copy();
    write_table_conf(dir.path());
    let before_gb = chunk_items(&fixture().join("gb_char.bin"));
    let before_art = chunk_items(&fixture().join("art.bin"));

    let outcome = run("gen_unigram", dir.path(), &[], b"");
    assert_eq!(exit_word(&outcome), 0, "stderr: {:?}", outcome.stderr);
    assert!(outcome.stdout.is_empty() && outcome.stderr.is_empty());

    let after_gb = chunk_items(&dir.path().join("gb_char.bin"));
    assert_eq!(after_gb.len(), before_gb.len());
    for (after, before) in after_gb.iter().zip(&before_gb) {
        assert_eq!(after.0, before.0);
        assert_eq!(after.1, before.1 + 1, "each default item gains one");
    }
    let after_art = chunk_items(&dir.path().join("art.bin"));
    for (after, before) in after_art.iter().zip(&before_art) {
        assert_eq!(after.0, before.0);
        assert_eq!(after.1, before.1 + 1, "each addon item gains one");
    }
    // `SubPhraseIndex::add_unigram_frequency` raises the stored total with
    // every unit.
    let (before_total, before_count) = chunk_total_and_count(&fixture().join("gb_char.bin"));
    let (after_total, after_count) = chunk_total_and_count(&dir.path().join("gb_char.bin"));
    assert_eq!(after_count, before_count);
    assert_eq!(after_total, before_total + before_count as u32);
}

#[test]
fn gen_unigram_reports_a_missing_chunk_and_exits_enoent() {
    let dir = fixture_copy();
    write_missing_chunk_conf(dir.path());
    let outcome = run("gen_unigram", dir.path(), &[], b"");
    assert_eq!(exit_word(&outcome), 2);
    assert_eq!(outcome.stdout, b"");
    assert_eq!(outcome.stderr, b"load absent.bin failed!\n");
}

#[test]
fn gen_ngram_adds_unigram_and_bigram_counts_then_saves_chunks() {
    let dir = fixture_copy();
    let before_start = path_row(&fixture().join("bigram.db"), START).unwrap_or((0, Vec::new()));
    let before_a = path_row(&fixture().join("bigram.db"), A).unwrap_or((0, Vec::new()));
    let before_unigrams = chunk_items(&fixture().join("gb_char.bin"));

    let outcome = run(
        "gen_ngram",
        dir.path(),
        &[],
        "16777219 阿\n16777220 啊\n\n16777219 阿\n".as_bytes(),
    );
    assert_eq!(exit_word(&outcome), 0, "stderr: {:?}", outcome.stderr);
    assert!(outcome.stdout.is_empty() && outcome.stderr.is_empty());

    // The uni-gram sweep of `gen_ngram.cpp:90`: 阿 twice, 啊 once.
    let after_unigrams = chunk_items(&dir.path().join("gb_char.bin"));
    for (after, before) in after_unigrams.iter().zip(&before_unigrams) {
        let added = match after.0 {
            3 => 2, // 阿's library-local slot
            4 => 1, // 啊's
            _ => 0,
        };
        assert_eq!(after.1, before.1 + added, "slot {}", after.0);
    }

    // The first token and the one after the blank line both follow
    // `sentence_start`: `sentence_start → 阿` twice, `阿 → 啊` once,
    // `啊 → 阿` once.
    let bigram = dir.path().join("bigram.db");
    let (start_total, start_records) = bigram_row(&bigram, START);
    assert_eq!(start_total, before_start.0 + 2);
    assert_eq!(record(&start_records, A), record(&before_start.1, A) + 2);
    let (a_total, a_records) = bigram_row(&bigram, A);
    assert_eq!(a_total, before_a.0 + 1);
    assert_eq!(record(&a_records, AH), record(&before_a.1, AH) + 1);
    // 啊 is only ever a successor in this corpus.
    assert_eq!(
        path_row(&bigram, AH),
        path_row(&fixture().join("bigram.db"), AH)
    );
}

/// One successor's stored count, 0 where the record is absent.
fn record(records: &[(u32, u32)], token: u32) -> u32 {
    records
        .iter()
        .find(|(next, _)| *next == token)
        .map_or(0, |(_, count)| *count)
}

#[test]
fn gen_ngram_skip_pi_gram_training_omits_the_sentence_start_row() {
    let dir = fixture_copy();
    let before_start = path_row(&fixture().join("bigram.db"), START);
    let before_a = path_row(&fixture().join("bigram.db"), A).unwrap_or((0, Vec::new()));
    let before_unigrams = chunk_items(&fixture().join("gb_char.bin"));

    let outcome = run(
        "gen_ngram",
        dir.path(),
        &["--skip-pi-gram-training"],
        "16777219 阿\n".as_bytes(),
    );
    assert_eq!(exit_word(&outcome), 0);

    // The uni-gram count is added before the pi-gram skip.
    let after_unigrams = chunk_items(&dir.path().join("gb_char.bin"));
    for (after, before) in after_unigrams.iter().zip(&before_unigrams) {
        let added = u32::from(after.0 == 3);
        assert_eq!(after.1, before.1 + added);
    }
    assert_eq!(
        path_row(&dir.path().join("bigram.db"), START),
        before_start,
        "--skip-pi-gram-training leaves the sentence_start row alone"
    );
    // 阿 is the first token, so no row is written for it either.
    assert_eq!(path_row(&dir.path().join("bigram.db"), A), Some(before_a),);
}

#[test]
fn gen_deleted_ngram_writes_rows_and_leaves_the_chunks_alone() {
    let dir = fixture_copy();
    let before_chunks: Vec<(String, Vec<u8>)> = [
        "gb_char.bin",
        "gbk_char.bin",
        "opengram.bin",
        "merged.bin",
        "art.bin",
    ]
    .into_iter()
    .map(|name| {
        (
            name.to_owned(),
            std::fs::read(fixture().join(name)).expect("chunk"),
        )
    })
    .collect();

    let outcome = run(
        "gen_deleted_ngram",
        dir.path(),
        &["--deleted-bigram-file", "deleted.db"],
        "16777219 阿\n16777220 啊\n".as_bytes(),
    );
    assert_eq!(exit_word(&outcome), 0, "stderr: {:?}", outcome.stderr);

    let deleted = dir.path().join("deleted.db");
    assert_eq!(bigram_row(&deleted, START).0, 1);
    assert_eq!(bigram_row(&deleted, START).1, vec![(A, 1)]);
    assert_eq!(bigram_row(&deleted, A).0, 1);
    assert_eq!(bigram_row(&deleted, A).1, vec![(AH, 1)]);

    for (name, bytes) in before_chunks {
        assert_eq!(
            std::fs::read(dir.path().join(&name)).expect("saved chunk"),
            bytes,
            "{name} is not saved by gen_deleted_ngram"
        );
    }
}

#[test]
fn gen_deleted_ngram_ends_a_missing_chunk_at_enodata() {
    let dir = fixture_copy();
    write_missing_chunk_conf(dir.path());
    let outcome = run("gen_deleted_ngram", dir.path(), &[], b"");
    assert_eq!(exit_word(&outcome), 61, "the ENODATA the pin uses");
    assert_eq!(outcome.stderr, b"load absent.bin failed!\n");
}

#[test]
fn gen_ngram_ends_a_missing_chunk_at_enoent() {
    let dir = fixture_copy();
    write_missing_chunk_conf(dir.path());
    let outcome = run("gen_ngram", dir.path(), &[], b"");
    assert_eq!(exit_word(&outcome), 2, "the ENOENT gen_ngram uses");
    assert_eq!(outcome.stderr, b"load absent.bin failed!\n");
}

#[test]
fn a_segmented_line_that_does_not_split_aborts() {
    for utility in ["gen_ngram", "gen_deleted_ngram"] {
        let dir = fixture_copy();
        let outcome = run(utility, dir.path(), &[], b"abc\n");
        assert_eq!(exit_word(&outcome), -6, "{utility}: the macro's abort()");
        assert!(outcome.stderr.is_empty());
    }
}

#[test]
fn a_phrase_that_differs_from_the_token_aborts() {
    let dir = fixture_copy();
    // Token 0x01000006 is 呵, not 啊.
    let outcome = run("gen_ngram", dir.path(), &[], "16777222 啊\n".as_bytes());
    assert_eq!(exit_word(&outcome), -6);
    // The check macro's own text is a policy-accepted language artefact.
    assert!(outcome.stderr.is_empty());
}

#[test]
fn estimate_interpolation_fits_the_committed_deleted_bigram() {
    let dir = fixture_copy();
    let outcome = run("estimate_interpolation", dir.path(), &[], b"");
    assert_eq!(exit_word(&outcome), 0, "stderr: {:?}", outcome.stderr);
    assert!(outcome.stderr.is_empty());
    let text = String::from_utf8(outcome.stdout).expect("stdout");
    let mut token_lines: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("token:"))
        .collect();
    token_lines.sort_unstable();
    assert_eq!(
        token_lines,
        ["token:1 lambda:0.000000", "token:16777219 lambda:0.999892",]
    );
    assert!(text.ends_with("average lambda:0.499946\n"), "{text}");
}

#[test]
fn estimate_interpolation_prints_the_host_nan_without_a_deleted_bigram() {
    let dir = fixture_copy();
    std::fs::remove_file(dir.path().join("deleted_bigram.db")).expect("remove deleted bigram");
    let outcome = run("estimate_interpolation", dir.path(), &[], b"");
    assert_eq!(exit_word(&outcome), 0);
    assert_eq!(outcome.stdout, b"average lambda:-nan\n");
    assert!(outcome.stderr.is_empty());
}

#[test]
fn estimate_interpolation_ignores_arguments() {
    let dir = fixture_copy();
    let plain = run("estimate_interpolation", dir.path(), &[], b"");
    let with_arguments = run("estimate_interpolation", dir.path(), &["--help", "x"], b"");
    assert_eq!(plain.stdout, with_arguments.stdout);
    assert_eq!(exit_word(&with_arguments), 0);
}

#[test]
fn eval_correction_rate_reports_the_correction_rate() {
    let dir = fixture_copy();
    let corpus = dir.path().join("evals2.text");

    std::fs::write(&corpus, "16777219 阿\n").expect("corpus");
    let outcome = run("eval_correction_rate", dir.path(), &[], b"");
    assert_eq!(exit_word(&outcome), 0);
    assert_eq!(outcome.stdout, b"correction rate:1.000000\n");
    assert!(outcome.stderr.is_empty());

    std::fs::write(&corpus, "16777219 阿\n16777220 啊\n\n").expect("corpus");
    let outcome = run("eval_correction_rate", dir.path(), &[], b"");
    assert_eq!(exit_word(&outcome), 0);
    assert_eq!(outcome.stdout, b"correction rate:0.000000\n");
    assert_eq!(
        outcome.stderr,
        "test sentence:阿啊\nguessed sentence:阿阿\nthe result mis-matches.\n".as_bytes()
    );

    std::fs::write(&corpus, "").expect("corpus");
    let outcome = run("eval_correction_rate", dir.path(), &[], b"");
    assert_eq!(exit_word(&outcome), 0);
    assert_eq!(outcome.stdout, b"correction rate:-nan\n");
}

#[test]
fn eval_correction_rate_reports_a_missing_corpus() {
    let dir = fixture_copy();
    // The committed set carries no evaluation corpus.
    assert!(!dir.path().join("evals2.text").exists());
    let outcome = run("eval_correction_rate", dir.path(), &[], b"");
    assert_eq!(exit_word(&outcome), 2);
    assert_eq!(outcome.stdout, b"");
    assert_eq!(outcome.stderr, b"Can't open file:evals2.text\n");
}

#[test]
fn eval_correction_rate_aborts_on_a_phrase_token_mismatch() {
    let dir = fixture_copy();
    // Token 0x01000006 is 呵, not 啊.
    std::fs::write(dir.path().join("evals2.text"), "16777222 啊\n").expect("corpus");
    let outcome = run("eval_correction_rate", dir.path(), &[], b"");
    assert_eq!(exit_word(&outcome), -6);
    assert!(outcome.stderr.is_empty());
}

#[test]
fn eval_correction_rate_refuses_an_unknown_token() {
    let dir = fixture_copy();
    std::fs::write(dir.path().join("evals2.text"), "999999 阿\n").expect("corpus");
    let outcome = run("eval_correction_rate", dir.path(), &[], b"");
    // The pin's `strcmp(NULL, phrase)` reads the NULL (class (b)); oxpinyin
    // writes the helper's own line and exits 1.
    assert_eq!(exit_word(&outcome), 1);
    assert_eq!(outcome.stderr, b"error: unknown token:999999.\n");
}

#[test]
fn the_option_help_names_the_pins_options() {
    let dir = fixture_copy();
    let outcome = run("gen_ngram", dir.path(), &["--help"], b"");
    assert_eq!(exit_word(&outcome), 0);
    let text = String::from_utf8(outcome.stdout).expect("help");
    assert!(text.contains("gen_ngram [OPTION"), "{text}");
    assert!(text.contains("--skip-pi-gram-training"), "{text}");
    assert!(text.contains("--bigram-file"), "{text}");
    assert!(text.contains("- generate n-gram"), "{text}");

    let outcome = run("gen_unigram", dir.path(), &["--help"], b"");
    let text = String::from_utf8(outcome.stdout).expect("help");
    assert!(text.contains("--table-dir"), "{text}");
    assert!(text.contains("- increase uni-gram"), "{text}");
}

#[test]
fn a_bad_option_is_glibs_refusal() {
    let dir = fixture_copy();
    let outcome = run("gen_unigram", dir.path(), &["--bogus"], b"");
    assert_eq!(exit_word(&outcome), 22);
    assert_eq!(
        outcome.stdout,
        b"option parsing failed:Unknown option --bogus\n"
    );
    assert!(outcome.stderr.is_empty());
}

/// The live differential against the pin's own utilities. Requires a build
/// of the pinned source (`PINYIN_TRAINING_PIN_DIR`, its `utils/training`
/// directory), and a data directory the compiled backend can open: the
/// committed `fixtures/w3/<DEFAULT_STORE_EXT>` is used. A missing input
/// fails rather than skipping.
#[test]
#[ignore = "needs PINYIN_TRAINING_PIN_DIR (the pin build's utils/training); run with --include-ignored"]
fn live_runs_match_the_pin() {
    let pin = std::env::var_os("PINYIN_TRAINING_PIN_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("missing input: PINYIN_TRAINING_PIN_DIR"));
    let pin_binary = |name: &str| pin.join(name);

    // gen_unigram over the same file set: every saved chunk byte-identical.
    let pin_dir = fixture_copy();
    let our_dir = fixture_copy();
    write_table_conf(pin_dir.path());
    write_table_conf(our_dir.path());
    let pin_out = Command::new(pin_binary("gen_unigram"))
        .current_dir(pin_dir.path())
        .output()
        .expect("pin gen_unigram");
    let our_out = run("gen_unigram", our_dir.path(), &[], b"");
    assert_eq!(exit_word(&pin_out), exit_word(&our_out));
    assert_eq!(pin_out.stdout, our_out.stdout);
    assert_eq!(pin_out.stderr, our_out.stderr);
    for name in [
        "gb_char.bin",
        "gbk_char.bin",
        "opengram.bin",
        "merged.bin",
        "art.bin",
    ] {
        assert_eq!(
            std::fs::read(pin_dir.path().join(name)).expect("pin chunk"),
            std::fs::read(our_dir.path().join(name)).expect("our chunk"),
            "{name}"
        );
    }

    // gen_ngram / gen_deleted_ngram: the container bytes carry a random
    // file identifier, so the containers are compared row by row.
    let stdin = b"16777219 \xe9\x98\xbf\n16777220 \xe5\x95\x8a\n\n16777219 \xe9\x98\xbf\n";
    for (utility, option, file) in [
        ("gen_ngram", None, "bigram.db"),
        (
            "gen_deleted_ngram",
            Some("--deleted-bigram-file"),
            "deleted.db",
        ),
    ] {
        let pin_dir = fixture_copy();
        let our_dir = fixture_copy();
        let args: &[&str] = match option {
            Some(flag) => &[flag, "deleted.db"],
            None => &[],
        };
        let mut command = Command::new(pin_binary(utility));
        command
            .args(args)
            .current_dir(pin_dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("pin utility");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(stdin)
            .expect("pin stdin");
        let pin_out = child.wait_with_output().expect("pin output");
        let our_out = run(utility, our_dir.path(), args, stdin);
        assert_eq!(exit_word(&pin_out), exit_word(&our_out), "{utility}");
        assert_eq!(pin_out.stdout, our_out.stdout, "{utility}");
        assert_eq!(pin_out.stderr, our_out.stderr, "{utility}");
        for token in [START, A, AH] {
            let pin_row = path_row(&pin_dir.path().join(file), token);
            let our_row = path_row(&our_dir.path().join(file), token);
            assert_eq!(pin_row, our_row, "{utility}: row {token}");
        }
        if utility == "gen_ngram" {
            for name in [
                "gb_char.bin",
                "gbk_char.bin",
                "opengram.bin",
                "merged.bin",
                "art.bin",
            ] {
                assert_eq!(
                    std::fs::read(pin_dir.path().join(name)).expect("pin chunk"),
                    std::fs::read(our_dir.path().join(name)).expect("our chunk"),
                    "{name}"
                );
            }
        }
    }

    // estimate_interpolation and eval_correction_rate: byte-identical streams.
    let pin_dir = fixture_copy();
    let our_dir = fixture_copy();
    for corpus in [
        "",
        "16777219 阿\n",
        "16777219 阿\n16777220 啊\n\n16777219 阿\n",
    ] {
        std::fs::write(pin_dir.path().join("evals2.text"), corpus).expect("corpus");
        std::fs::write(our_dir.path().join("evals2.text"), corpus).expect("corpus");
        for utility in ["estimate_interpolation", "eval_correction_rate"] {
            let pin_out = Command::new(pin_binary(utility))
                .current_dir(pin_dir.path())
                .output()
                .expect("pin utility");
            let our_out = run(utility, our_dir.path(), &[], b"");
            assert_eq!(exit_word(&pin_out), exit_word(&our_out), "{utility}");
            assert_eq!(pin_out.stdout, our_out.stdout, "{utility}");
            assert_eq!(pin_out.stderr, our_out.stderr, "{utility}");
        }
    }
}

/// [`bigram_row`] for a path that may or may not exist.
fn path_row(path: &Path, token: u32) -> Option<(u32, Vec<(u32, u32)>)> {
    if !has_bigram_row(path, token) {
        return None;
    }
    Some(bigram_row(path, token))
}
