//! The flexible-bigram (k-mixture-model) container on the compiled backend.
//!
//! The normal tests drive every operation of [`FlexibleBigram`] against the
//! backend this build selected, so the same file is tested under Berkeley
//! DB, Kyoto Cabinet and tkrzw by the three cell builds. The ignored tests
//! cross-read real pin files: they need the pin's own `gen_k_mixture_model`,
//! `export_k_mixture_model` and `validate_k_mixture_model`, built against the
//! same DBM as this build, and `fixtures/w3/{db,kct,tkt}`. Run them with
//! `--include-ignored` and
//!
//! ```text
//! PINYIN_PIN_TOOLS_DIR=/path/to/libpinyin-074a2219/utils/training
//! ```
//!
//! pointing at the matching backend's build. A missing variable or binary is
//! a failure, never a skip.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use oxpinyin_store::{
    ArrayItemWithToken, DefaultFlexibleBigram, DefaultStore, FixedRecord, FlexibleGramSpec,
    MAGIC_KEY, RawReadStore, SingleGram, StoreError, WriteStore, WriteTxn,
};

// ── the k-mixture-model records ────────────────────────────────────

/// `KMixtureModelMagicHeader`: `m_WC`, `m_N`, `m_total_freq`
/// (`utils/training/k_mixture_model.h:111-118`).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Magic {
    wc: u32,
    n: u32,
    total_freq: u32,
}

/// `KMixtureModelArrayHeader`: `m_WC`, `m_freq`
/// (`utils/training/k_mixture_model.h:122-127`).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Header {
    wc: u32,
    freq: u32,
}

/// `KMixtureModelArrayItem`: `m_WC`, `m_N_n_0`, `m_n_1`, `m_Mr`
/// (`utils/training/k_mixture_model.h:129-154`).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Item {
    wc: u32,
    n_n_0: u32,
    n_1: u32,
    mr: u32,
}

impl FixedRecord for Magic {
    const LEN: usize = 12;

    fn zeroed() -> Self {
        Self {
            wc: 0,
            n: 0,
            total_freq: 0,
        }
    }

    fn encode(&self) -> Vec<u8> {
        words(&[self.wc, self.n, self.total_freq])
    }

    fn decode(bytes: &[u8]) -> Result<Self, StoreError> {
        let [wc, n, total_freq] = read_words::<3>(bytes)?;
        Ok(Self { wc, n, total_freq })
    }
}

impl FixedRecord for Header {
    const LEN: usize = 8;

    fn zeroed() -> Self {
        Self { wc: 0, freq: 0 }
    }

    fn encode(&self) -> Vec<u8> {
        words(&[self.wc, self.freq])
    }

    fn decode(bytes: &[u8]) -> Result<Self, StoreError> {
        let [wc, freq] = read_words::<2>(bytes)?;
        Ok(Self { wc, freq })
    }
}

impl FixedRecord for Item {
    const LEN: usize = 16;

    fn zeroed() -> Self {
        Self {
            wc: 0,
            n_n_0: 0,
            n_1: 0,
            mr: 0,
        }
    }

    fn encode(&self) -> Vec<u8> {
        words(&[self.wc, self.n_n_0, self.n_1, self.mr])
    }

    fn decode(bytes: &[u8]) -> Result<Self, StoreError> {
        let [wc, n_n_0, n_1, mr] = read_words::<4>(bytes)?;
        Ok(Self { wc, n_n_0, n_1, mr })
    }
}

/// The native u32 bytes the pin `memcpy`s on the reference host.
fn words(values: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    for value in values {
        bytes.extend_from_slice(&value.to_ne_bytes());
    }
    bytes
}

/// An exact-length decode into `N` native u32s.
fn read_words<const N: usize>(bytes: &[u8]) -> Result<[u32; N], StoreError> {
    if bytes.len() != N * 4 {
        return Err(StoreError::Backend(
            format!("expected {} bytes", N * 4).into(),
        ));
    }
    let mut values = [0u32; N];
    for (value, chunk) in values.iter_mut().zip(bytes.chunks_exact(4)) {
        let mut word = [0u8; 4];
        word.copy_from_slice(chunk);
        *value = u32::from_ne_bytes(word);
    }
    Ok(values)
}

/// The pin's k-mixture model as `KMixtureModelBigram` names it
/// (`utils/training/k_mixture_model.h:110-166`).
struct Kmm;

impl FlexibleGramSpec for Kmm {
    const MAGIC_NUMBER: [u8; 4] = *b"KMMP";
    type MagicHeader = Magic;
    type ArrayHeader = Header;
    type ArrayItem = Item;
}

type Model = DefaultFlexibleBigram<Kmm>;
type ItemsByToken = BTreeMap<u32, Vec<ArrayItemWithToken<Item>>>;

// ── fixtures and scratch directories ───────────────────────────────

/// The fixture directory for the backend this build selected.
fn cell_dir() -> &'static str {
    if cfg!(feature = "bdb") {
        "db"
    } else if cfg!(feature = "kyotocabinet") {
        "kct"
    } else {
        "tkt"
    }
}

fn w3_cell() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/w3")
        .join(cell_dir())
}

/// A private directory removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-store-flexible-{}-{}-{tag}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("scratch directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("copy destination");
    for entry in fs::read_dir(from).expect("fixture directory") {
        let entry = entry.expect("fixture entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("fixture file type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("fixture file");
        }
    }
}

/// The pin's `utils/training` directory for this backend, or a failure:
/// these tests never skip a missing input.
fn pin_tool(name: &str) -> PathBuf {
    let Some(dir) = std::env::var_os("PINYIN_PIN_TOOLS_DIR") else {
        panic!(
            "missing input: set PINYIN_PIN_TOOLS_DIR to the pin 074a2219 build's \
             utils/training directory for the {} cell (needs pin tools)",
            cell_dir()
        );
    };
    let path = PathBuf::from(dir).join(name);
    assert!(
        path.is_file(),
        "missing input: {} is not a runnable pin tool (needs pin tools)",
        path.display()
    );
    path
}

fn run_pin(tool: &Path, scratch: &Path, args: &[&OsStr]) -> Output {
    Command::new(tool)
        .current_dir(scratch)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("cannot run {}: {error}", tool.display()))
}

fn assert_pin_success(what: &str, output: &Output) {
    assert!(
        output.status.success(),
        "pin {what} failed with {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

// ── export text ────────────────────────────────────────────────────

/// What `export_k_mixture_model` printed, in its own native order.
#[derive(Debug)]
struct Export {
    magic: Magic,
    headers: Vec<(u32, Header)>,
    items: ItemsByToken,
}

impl Export {
    fn tokens(&self) -> Vec<u32> {
        self.headers.iter().map(|(token, _)| *token).collect()
    }
}

fn parse_export(text: &str) -> Export {
    let mut magic = None;
    let mut headers = Vec::new();
    let mut items: ItemsByToken = BTreeMap::new();
    let mut section = "";
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("\\data model \"k mixture model\" count ") {
            let fields: Vec<&str> = rest.split_whitespace().collect();
            assert_eq!(fields.len(), 5, "magic header line: {line:?}");
            assert_eq!(fields[1], "N", "magic header line: {line:?}");
            assert_eq!(fields[3], "total_freq", "magic header line: {line:?}");
            magic = Some(Magic {
                wc: fields[0].parse().expect("magic count"),
                n: fields[2].parse().expect("magic N"),
                total_freq: fields[4].parse().expect("magic total_freq"),
            });
        } else if line == "\\1-gram" {
            section = "1";
        } else if line == "\\2-gram" {
            section = "2";
        } else if line == "\\end" {
            section = "";
        } else if let Some(rest) = line.strip_prefix("\\item ") {
            let fields: Vec<&str> = rest.split_whitespace().collect();
            match section {
                "1" => {
                    assert_eq!(fields.len(), 6, "1-gram line: {line:?}");
                    assert_eq!(fields[2], "count");
                    assert_eq!(fields[4], "freq");
                    headers.push((
                        fields[0].parse().expect("token"),
                        Header {
                            wc: fields[3].parse().expect("array header WC"),
                            freq: fields[5].parse().expect("array header freq"),
                        },
                    ));
                }
                "2" => {
                    assert_eq!(fields.len(), 14, "2-gram line: {line:?}");
                    assert_eq!(fields[4], "count");
                    assert_eq!(fields[6], "T");
                    assert_eq!(fields[8], "N_n_0");
                    assert_eq!(fields[10], "n_1");
                    assert_eq!(fields[12], "Mr");
                    let token: u32 = fields[0].parse().expect("previous token");
                    let item = ArrayItemWithToken {
                        token: fields[2].parse().expect("next token"),
                        item: Item {
                            wc: fields[5].parse().expect("item WC"),
                            n_n_0: fields[9].parse().expect("item N_n_0"),
                            n_1: fields[11].parse().expect("item n_1"),
                            mr: fields[13].parse().expect("item Mr"),
                        },
                    };
                    items.entry(token).or_default().push(item);
                }
                _ => panic!("item outside a section: {line:?}"),
            }
        }
    }
    Export {
        magic: magic.expect("magic header"),
        headers,
        items,
    }
}

// ── ordinary tests on every backend ────────────────────────────────

#[test]
fn every_operation_round_trips() {
    let scratch = Scratch::new("round-trip");
    let path = scratch.path().join("model.db");
    let magic = Magic {
        wc: 2,
        n: 1,
        total_freq: 2,
    };
    let header = Header { wc: 0, freq: 1 };
    let other = Header { wc: 9, freq: 4 };

    {
        let mut bigram = Model::open(&path).expect("open a fresh model");
        assert!(bigram.is_open());
        assert_eq!(bigram.get_magic_header().expect("magic read"), None);
        assert_eq!(bigram.get_all_items(), Some(Vec::new()));
        assert_eq!(bigram.load(7).expect("missing load"), None);
        assert_eq!(bigram.get_array_header(7).expect("missing header"), None);
        assert_eq!(bigram.retrieve_all(7).expect("missing items"), None);
        bigram.close();
        assert!(!bigram.is_open());
    }

    // A fresh container carries the four-byte magic number and no header,
    // exactly as the pin's create path writes it.
    {
        let raw = DefaultStore::open_hash_read_only(&path).expect("raw open");
        let mut value = Vec::new();
        assert!(raw.get_raw(&MAGIC_KEY, &mut value).expect("magic read"));
        assert_eq!(value, b"KMMP");
    }

    {
        let mut bigram = Model::open(&path).expect("reopen an existing model");
        assert!(bigram.set_magic_header(&magic).expect("magic write"));
        assert_eq!(
            bigram.get_magic_header().expect("magic read"),
            Some(magic.clone())
        );

        // set_array_header creates a header-only record when the token has
        // no row, as all three pin backends do.
        assert!(bigram.set_array_header(7, &header).expect("header write"));
        assert_eq!(
            bigram.get_array_header(7).expect("header read"),
            Some(header.clone())
        );
        assert_eq!(
            bigram.load(7).expect("header-only load"),
            Some(SingleGram::from_parts(header.clone(), Vec::new()))
        );

        // The in-memory single gram: ascending tokens, unique inserts.
        let mut gram: SingleGram<Header, Item> = SingleGram::new();
        gram.set_header(Header { wc: 2, freq: 2 });
        assert!(gram.insert_array_item(
            9,
            Item {
                wc: 1,
                n_n_0: 1,
                n_1: 1,
                mr: 1,
            }
        ));
        assert!(gram.insert_array_item(
            3,
            Item {
                wc: 7,
                n_n_0: 2,
                n_1: 1,
                mr: 3,
            }
        ));
        assert_eq!(
            gram.items()
                .iter()
                .map(|entry| entry.token)
                .collect::<Vec<_>>(),
            vec![3, 9]
        );
        assert!(!gram.insert_array_item(3, Item::zeroed()));
        assert_eq!(
            gram.get_array_item(3),
            Some(Item {
                wc: 7,
                n_n_0: 2,
                n_1: 1,
                mr: 3,
            })
        );
        assert!(gram.set_array_item(
            3,
            Item {
                wc: 8,
                n_n_0: 2,
                n_1: 1,
                mr: 3,
            }
        ));
        assert!(!gram.set_array_item(4, Item::zeroed()));
        assert_eq!(
            gram.remove_array_item(9),
            Some(Item {
                wc: 1,
                n_n_0: 1,
                n_1: 1,
                mr: 1,
            })
        );
        assert_eq!(gram.remove_array_item(9), None);

        assert!(bigram.store(7, &gram).expect("gram write"));
        assert_eq!(
            bigram.load(7).expect("gram read"),
            Some(gram.clone()),
            "the stored value is the whole single gram"
        );
        assert_eq!(
            bigram.retrieve_all(7).expect("item read"),
            Some(gram.items().to_vec())
        );

        // A header write changes only the header: every item byte survives.
        assert!(bigram.set_array_header(7, &other).expect("header rewrite"));
        let loaded = bigram.load(7).expect("after header rewrite").unwrap();
        assert_eq!(loaded.header(), &other);
        assert_eq!(loaded.items(), gram.items());

        assert!(bigram.store(11, &gram).expect("second gram"));
        let mut tokens = bigram.get_all_items().expect("token walk");
        tokens.sort_unstable();
        assert_eq!(tokens, vec![7, 11], "only the four-byte token keys");
        assert!(!tokens.contains(&0), "the eight-byte magic key is skipped");

        assert!(bigram.remove(7).expect("gram remove"));
        assert!(!bigram.remove(7).expect("second remove"));
        assert_eq!(bigram.load(7).expect("removed load"), None);
        assert_eq!(bigram.get_array_header(7).expect("removed header"), None);
        assert_eq!(bigram.retrieve_all(7).expect("removed items"), None);

        bigram.close();
    }

    let mut read_only = Model::open_read_only(&path).expect("read-only open");
    assert_eq!(
        read_only.load(11).expect("read-only load"),
        Some(gram_with_header())
    );
    assert!(
        !read_only
            .store(12, &gram_with_header())
            .expect("read-only store")
    );
    assert!(!read_only.remove(11).expect("read-only remove"));
    assert!(
        !read_only
            .set_magic_header(&magic)
            .expect("read-only magic write")
    );
    assert!(
        !read_only
            .set_array_header(11, &other)
            .expect("read-only header write")
    );
    assert_eq!(
        read_only.get_all_items(),
        Some(vec![11]),
        "the read-only walk sees the remaining token"
    );
    read_only.close();
}

/// The single gram `every_operation_round_trips` stores under token 11.
fn gram_with_header() -> SingleGram<Header, Item> {
    let mut gram = SingleGram::new();
    gram.set_header(Header { wc: 2, freq: 2 });
    assert!(gram.insert_array_item(
        3,
        Item {
            wc: 8,
            n_n_0: 2,
            n_1: 1,
            mr: 3,
        }
    ));
    gram
}

#[test]
fn a_foreign_or_signatureless_container_is_refused() {
    let scratch = Scratch::new("signature");

    let foreign = scratch.path().join("foreign.db");
    {
        let db = DefaultStore::create_hash(&foreign).expect("hash create");
        db.write(|txn| txn.put_raw(&MAGIC_KEY, b"BAAD"))
            .expect("foreign magic");
    }
    assert!(Model::open(&foreign).is_err(), "wrong magic number");
    assert!(
        Model::open_read_only(&foreign).is_err(),
        "wrong magic number read-only"
    );

    let missing = scratch.path().join("missing.db");
    {
        let db = DefaultStore::create_hash(&missing).expect("hash create");
        db.write(|txn| txn.put_raw(&7u32.to_ne_bytes(), b"not a model"))
            .expect("foreign row");
    }
    assert!(
        Model::open(&missing).is_err(),
        "a non-empty file has no magic"
    );
    assert!(
        Model::open_read_only(&missing).is_err(),
        "a non-empty file has no magic read-only"
    );
}

#[test]
#[should_panic(expected = "shorter than the pin's")]
fn get_array_header_asserts_the_records_header_length() {
    let scratch = Scratch::new("short-header");
    let path = scratch.path().join("short.db");
    {
        let db = DefaultStore::create_hash(&path).expect("hash create");
        db.write(|txn| {
            txn.put_raw(&MAGIC_KEY, b"KMMP")?;
            txn.put_raw(&7u32.to_ne_bytes(), &[0u8; 4])?;
            Ok(())
        })
        .expect("short record");
    }
    let bigram = Model::open_read_only(&path).expect("open");
    let _ = bigram.get_array_header(7);
}

#[test]
fn remove_raw_deletes_the_bare_key() {
    let scratch = Scratch::new("remove-raw");
    let path = scratch.path().join("raw.db");
    {
        let db = DefaultStore::create_hash(&path).expect("hash create");
        db.write(|txn| txn.put_raw(b"raw", b"value"))
            .expect("raw write");
        db.write(|txn| txn.remove_raw(b"raw")).expect("raw remove");
    }
    let db = DefaultStore::open_hash_read_only(&path).expect("reopen");
    let mut value = Vec::new();
    assert!(!db.get_raw(b"raw", &mut value).expect("raw read"));
}

// ── pin cross-reads ────────────────────────────────────────────────

/// The one-document corpus these tests train: two words, so the model has
/// the start-token gram and the first word's gram, plus the second word's
/// header-only row.
const DOC: &str = "16786821 中\n16780275 国\n";

/// The pins' expectation for [`DOC`], derived from
/// `utils/training/gen_k_mixture_model.cpp:252-318`:
///
/// * magic `WC` counts every bigram (start→中, 中→国) = 2, `N` = 1,
///   `total_freq` counts every unigram (中, 国) = 2;
/// * token 1 (`sentence_start`) holds 中 with one occurrence;
/// * 中 holds 国 with one occurrence, and its unigram frequency is 1;
/// * 国 never starts a bigram, but `post_processing_unigram` creates its
///   header-only row with WC 0 and its unigram frequency 1.
fn expected() -> (Magic, BTreeMap<u32, Header>, ItemsByToken) {
    let magic = Magic {
        wc: 2,
        n: 1,
        total_freq: 2,
    };
    let headers = BTreeMap::from([
        (1, Header { wc: 1, freq: 0 }),
        (16_786_821, Header { wc: 1, freq: 1 }),
        (16_780_275, Header { wc: 0, freq: 1 }),
    ]);
    let item = ArrayItemWithToken {
        token: 16_786_821,
        item: Item {
            wc: 1,
            n_n_0: 1,
            n_1: 1,
            mr: 1,
        },
    };
    let second = ArrayItemWithToken {
        token: 16_780_275,
        item: Item {
            wc: 1,
            n_n_0: 1,
            n_1: 1,
            mr: 1,
        },
    };
    let items = BTreeMap::from([(1, vec![item]), (16_786_821, vec![second])]);
    (magic, headers, items)
}

#[test]
#[ignore = "needs pin tools (PINYIN_PIN_TOOLS_DIR); run with --include-ignored"]
fn a_pin_generated_file_is_read_in_the_pins_native_order() {
    let generator = pin_tool("gen_k_mixture_model");
    let export = pin_tool("export_k_mixture_model");
    let scratch = Scratch::new("pin-read");
    copy_tree(&w3_cell(), scratch.path());
    fs::write(scratch.path().join("doc.txt"), DOC).expect("document");

    let generated = run_pin(
        &generator,
        scratch.path(),
        &[
            OsStr::new("--k-mixture-model-file"),
            OsStr::new("out.db"),
            OsStr::new("doc.txt"),
        ],
    );
    assert_pin_success("gen_k_mixture_model", &generated);

    let exported = run_pin(
        &export,
        scratch.path(),
        &[OsStr::new("--k-mixture-model-file"), OsStr::new("out.db")],
    );
    assert_pin_success("export_k_mixture_model", &exported);
    let text = String::from_utf8(exported.stdout.clone()).expect("export UTF-8");
    let parsed = parse_export(&text);

    let (magic, headers, items) = expected();
    assert_eq!(parsed.magic, magic, "source-derived magic header");
    assert_eq!(
        parsed.headers.iter().cloned().collect::<BTreeMap<_, _>>(),
        headers,
        "source-derived array headers"
    );
    assert_eq!(parsed.items, items, "source-derived array items");

    let bigram = Model::open_read_only(&scratch.path().join("out.db")).expect("layer open");
    assert_eq!(
        bigram.get_magic_header().expect("magic read"),
        Some(magic),
        "the layer reads the pin's magic header"
    );
    assert_eq!(
        bigram.get_all_items().expect("token walk"),
        parsed.tokens(),
        "get_all_items is the pin's own export order"
    );
    for (token, header) in &parsed.headers {
        assert_eq!(
            bigram.get_array_header(*token).expect("array header"),
            Some(header.clone()),
            "array header of token {token}"
        );
        assert_eq!(
            bigram.retrieve_all(*token).expect("items"),
            Some(parsed.items.get(token).cloned().unwrap_or_default()),
            "items of token {token}"
        );
        let gram = bigram.load(*token).expect("load").expect("stored gram");
        assert_eq!(gram.header(), header);
        assert_eq!(
            gram.items(),
            parsed
                .items
                .get(token)
                .map(Vec::as_slice)
                .unwrap_or_default(),
            "loaded gram of token {token}"
        );
    }
}

#[test]
#[ignore = "needs pin tools (PINYIN_PIN_TOOLS_DIR); run with --include-ignored"]
fn a_layer_written_file_is_read_by_pin_validate_and_export() {
    let validate = pin_tool("validate_k_mixture_model");
    let export = pin_tool("export_k_mixture_model");
    let scratch = Scratch::new("pin-write");
    copy_tree(&w3_cell(), scratch.path());
    let path = scratch.path().join("out.db");
    let (magic, headers, items) = expected();

    {
        let mut bigram = Model::open(&path).expect("layer create");
        assert!(bigram.set_magic_header(&magic).expect("magic write"));
        for (token, header) in &headers {
            let gram = SingleGram::from_parts(
                header.clone(),
                items.get(token).cloned().unwrap_or_default(),
            );
            assert!(bigram.store(*token, &gram).expect("gram write"));
        }
        bigram.close();
    }

    let validated = run_pin(&validate, scratch.path(), &[OsStr::new("out.db")]);
    assert_pin_success("validate_k_mixture_model", &validated);
    assert!(
        validated.stderr.is_empty(),
        "validate stderr: {}",
        String::from_utf8_lossy(&validated.stderr)
    );

    let exported = run_pin(
        &export,
        scratch.path(),
        &[OsStr::new("--k-mixture-model-file"), OsStr::new("out.db")],
    );
    assert_pin_success("export_k_mixture_model", &exported);
    let text = String::from_utf8(exported.stdout.clone()).expect("export UTF-8");
    assert!(
        text.contains("\\data model \"k mixture model\" count 2 N 1 total_freq 2\n"),
        "export magic line:\n{text}"
    );
    let lines: BTreeSet<&str> = text.lines().collect();
    for line in [
        "\\item 1 <start> count 1 freq 0",
        "\\item 16786821 中 count 1 freq 1",
        "\\item 16780275 国 count 0 freq 1",
        "\\item 1 <start> 16786821 中 count 1 T 1 N_n_0 1 n_1 1 Mr 1",
        "\\item 16786821 中 16780275 国 count 1 T 1 N_n_0 1 n_1 1 Mr 1",
    ] {
        assert!(lines.contains(line), "export lacks {line:?}:\n{text}");
    }
    let parsed = parse_export(&text);
    assert_eq!(parsed.magic, magic);
    assert_eq!(
        parsed.headers.iter().cloned().collect::<BTreeMap<_, _>>(),
        headers
    );
    assert_eq!(parsed.items, items);
}
