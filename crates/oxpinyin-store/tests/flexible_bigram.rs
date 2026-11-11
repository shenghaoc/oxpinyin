//! The flexible-bigram (k-mixture-model) container on the compiled backend.
//!
//! The normal tests drive every operation of [`FlexibleBigram`] against the
//! backend this build selected, so the same file is tested under Berkeley
//! DB, Kyoto Cabinet and tkrzw by the three cell builds. The last cases read
//! the committed `fixtures/w3/{db,kct,tkt}/k_mixture_model.db` and check them
//! against the pin's export of the same file.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use oxpinyin_store::{
    ArrayItemWithToken, AttachMode, DefaultFlexibleBigram, DefaultStore, FixedRecord,
    FlexibleGramSpec, MAGIC_KEY, RawReadStore, SingleGram, StoreError, SystemAttachment,
    WriteStore, WriteTxn,
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
    for (value, chunk) in values.iter_mut().zip(bytes.as_chunks::<4>().0) {
        *value = u32::from_ne_bytes(*chunk);
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

/// `attach(path, ATTACH_READWRITE | ATTACH_CREATE)`, which must answer true.
fn open(path: &Path) -> Model {
    let model = Model::attach(Some(path), AttachMode::ReadWriteCreate).expect("attach");
    assert!(model.attached(), "{} attaches", path.display());
    model
}

/// `attach(path, ATTACH_READONLY)`, which must answer true.
fn open_read_only(path: &Path) -> Model {
    let model = Model::attach(Some(path), AttachMode::ReadOnly).expect("attach");
    assert!(model.attached(), "{} attaches read-only", path.display());
    model
}

/// Whether `path` holds `key` as a raw record, read through a fresh handle.
fn raw_has(path: &Path, key: &[u8]) -> bool {
    let raw = DefaultStore::open_hash_read_only(path).expect("raw open");
    let mut value = Vec::new();
    raw.get_raw(key, &mut value).expect("raw read")
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
        let bigram = open(&path);
        assert_eq!(bigram.get_magic_header().expect("magic read"), None);
        assert_eq!(bigram.get_all_items(), Some(Vec::new()));
        assert_eq!(bigram.load(7).expect("missing load"), None);
        assert_eq!(bigram.get_array_header(7).expect("missing header"), None);
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
        let bigram = open(&path);
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
    }

    let read_only = open_read_only(&path);
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
fn an_invalid_signature_answers_false_and_keeps_the_opened_object() {
    // Every backend keeps the opened object when the signature check fails
    // (`flexible_ngram_bdb.h:134-152`, `flexible_ngram_kyotodb.h:165-180`,
    // `flexible_ngram_tkrzwdb.h:170-187` at 074a2219).
    let scratch = Scratch::new("signature");

    let foreign = scratch.path().join("foreign.db");
    {
        let db = DefaultStore::create_hash(&foreign).expect("hash create");
        db.write(|txn| txn.put_raw(&MAGIC_KEY, b"BAAD"))
            .expect("foreign magic");
    }
    for mode in [AttachMode::ReadWriteCreate, AttachMode::ReadOnly] {
        let model = Model::attach(Some(&foreign), mode).expect("attach");
        assert!(!model.attached(), "{mode:?}: wrong magic number");
        assert!(
            model.get_all_items().is_some(),
            "{mode:?}: the opened object stays"
        );
    }

    let signatureless = scratch.path().join("signatureless.db");
    {
        let db = DefaultStore::create_hash(&signatureless).expect("hash create");
        db.write(|txn| txn.put_raw(&7u32.to_ne_bytes(), b"not a model"))
            .expect("foreign row");
    }
    let model = Model::attach(Some(&signatureless), AttachMode::ReadWriteCreate).expect("attach");
    assert!(!model.attached(), "a non-empty file has no magic");
    assert!(model.get_all_items().is_some());
}

#[test]
fn a_retained_object_takes_the_later_writes() {
    // The pin's tools ignore `attach`'s answer and keep writing through
    // the object it left (`import_k_mixture_model.cpp:293`).
    let scratch = Scratch::new("retained");
    let path = scratch.path().join("foreign.db");
    {
        let db = DefaultStore::create_hash(&path).expect("hash create");
        db.write(|txn| txn.put_raw(&MAGIC_KEY, b"BAAD"))
            .expect("foreign magic");
    }
    {
        let model = Model::attach(Some(&path), AttachMode::ReadWriteCreate).expect("attach");
        assert!(!model.attached());
        assert!(model.store(11, &gram_with_header()).expect("store"));
    }
    assert!(
        raw_has(&path, &11u32.to_ne_bytes()),
        "the write reached the file"
    );
}

#[test]
fn a_missing_file_is_created_only_with_create() {
    let scratch = Scratch::new("missing");

    let created = scratch.path().join("created.db");
    let model = Model::attach(Some(&created), AttachMode::ReadWriteCreate).expect("attach");
    assert!(model.attached() && model.get_all_items().is_some());
    drop(model);
    assert!(
        raw_has(&created, &MAGIC_KEY),
        "the CREATE retry wrote the signature"
    );

    // `prune_k_mixture_model` attaches without ATTACH_CREATE
    // (`prune_k_mixture_model.cpp:129`): a missing file stays missing.
    for (name, mode) in [
        ("rw.db", AttachMode::ReadWrite),
        ("ro.db", AttachMode::ReadOnly),
    ] {
        let path = scratch.path().join(name);
        let model = Model::attach(Some(&path), mode).expect("attach");
        assert!(!model.attached(), "{mode:?}");
        assert!(model.get_all_items().is_none(), "{mode:?}: no object stays");
        assert!(!path.exists(), "{mode:?}: no file is created");
    }

    let model = Model::attach(None, AttachMode::ReadWriteCreate).expect("attach");
    assert!(
        !model.attached() && model.get_all_items().is_none(),
        "a NULL filename"
    );
}

#[test]
fn an_existing_empty_file_follows_the_backends_open() {
    // Berkeley DB refuses a zero-length file without DB_CREATE
    // (`fop_util.c:381-393`, 5.3.28), so the CREATE retry initializes it
    // and writes the signature. Kyoto Cabinet and tkrzw initialize a
    // writable empty file on the first open (`kchashdb.h:835-850`,
    // `tkrzw_dbm_hash.cc:1173`), so the signature check fails.
    let scratch = Scratch::new("empty");
    let path = scratch.path().join("empty.db");
    fs::write(&path, b"").expect("empty file");
    let model = Model::attach(Some(&path), AttachMode::ReadWriteCreate).expect("attach");
    assert!(model.get_all_items().is_some());
    if cfg!(feature = "bdb") {
        assert!(model.attached());
        drop(model);
        assert!(raw_has(&path, &MAGIC_KEY));
    } else {
        assert!(!model.attached());
        drop(model);
        assert!(!raw_has(&path, &MAGIC_KEY), "no signature was written");
    }
}

#[test]
fn a_failed_create_retry_follows_the_backends_rule() {
    // Kyoto Cabinet keeps the object whose CREATE retry failed
    // (`flexible_ngram_kyotodb.h:154-155`); tkrzw deletes it
    // (`flexible_ngram_tkrzwdb.h:154-158`); Berkeley DB closes the handle.
    let scratch = Scratch::new("retry");
    let path = scratch.path().join("no-such-directory").join("model.db");
    let model = Model::attach(Some(&path), AttachMode::ReadWriteCreate).expect("attach");
    assert!(!model.attached());
    if cfg!(feature = "kyotocabinet") {
        assert!(model.store(11, &gram_with_header()).is_err(), "INVALID");
        assert_eq!(
            model.get_all_items(),
            Some(Vec::new()),
            "the unopened HashDB stays"
        );
    } else {
        assert!(model.get_all_items().is_none());
        assert!(!model.store(11, &gram_with_header()).expect("closed store"));
    }
}

#[test]
fn the_deferred_removal_flag_aggregates_every_removal() {
    let scratch = Scratch::new("removal-aggregate");
    let db = DefaultStore::create_hash(&scratch.path().join("raw.db")).expect("hash create");
    let put = |key: &'static [u8]| {
        db.write_deferred(|txn| txn.put_raw(key, b"v"))
            .expect("put");
    };
    let remove_two = |first: &'static [u8], second: &'static [u8]| {
        db.write_deferred(|txn| {
            txn.remove_raw(first)?;
            txn.remove_raw(second)
        })
        .expect("remove")
        .1
    };
    put(b"a");
    put(b"b");
    assert!(remove_two(b"a", b"b"), "both records found");
    put(b"b");
    // The buffer applies in key order: the missing `a` first, the found
    // `b` last, so a last-answer-wins flag would answer true here.
    assert!(!remove_two(b"a", b"b"), "one of two missing");
    assert!(!remove_two(b"y", b"z"), "both missing");
    let ((), found) = db
        .write_deferred(|txn| txn.put_raw(b"c", b"v"))
        .expect("put");
    assert!(found, "no removal at all");
}

#[test]
fn attach_bigram_answers_a_failed_open_apart_from_an_earlier_failure() {
    // `Bigram::attach`: a failed native open is an unopened attachment,
    // whose object Kyoto Cabinet and tkrzw keep; a failure before the open
    // is an error on every backend.
    let attach = |path: &Path, read_only| SystemAttachment::attach_bigram(path, read_only);
    let scratch = Scratch::new("attach-bigram");
    let missing = scratch.path().join("missing.db");
    assert!(matches!(
        attach(&missing, true).expect("attach"),
        SystemAttachment::Unopened(_)
    ));
    assert!(!missing.exists());
    let created = scratch.path().join("bigram.db");
    for read_only in [false, true] {
        assert!(
            matches!(
                attach(&created, read_only).expect("attach"),
                SystemAttachment::Open(_)
            ),
            "read only: {read_only}"
        );
    }
    let nul = Path::new(std::ffi::OsStr::new("bad\0name.db"));
    assert!(attach(nul, true).is_err(), "a rejected path");
}

#[test]
fn a_failed_deferred_closure_applies_nothing() {
    let scratch = Scratch::new("deferred-err");
    let path = scratch.path().join("raw.db");
    let db = DefaultStore::create_hash(&path).expect("hash create");
    let result = db.write_deferred(|txn| {
        txn.put_raw(b"k", b"v")?;
        Err::<(), _>(StoreError::InvalidInput("closure failed"))
    });
    assert!(result.is_err());
    let mut value = Vec::new();
    assert!(
        !db.get_raw(b"k", &mut value).expect("read"),
        "no earlier write stays"
    );
}

/// The pin asserts a record shorter than its array header
/// (`flexible_ngram_bdb.h:388`, `flexible_ngram_kyotodb.h:371`,
/// `flexible_ngram_tkrzwdb.h:388` at 074a2219); the store reports that fault
/// as its own error, and the K-mixture tools abort on it.
#[test]
fn a_short_array_header_is_a_typed_fault() {
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
    let bigram = open_read_only(&path);
    assert!(matches!(
        bigram.get_array_header(7),
        Err(StoreError::ShortArrayHeader)
    ));
}

// ── pin-written file ───────────────────────────────────────────────

/// One `KMixtureModelArrayItem` with its token; each row of
/// `fixtures/w3/kmm/k_mixture_model.text` counts `count 1 N_n_0 1 n_1 1 Mr 1`.
fn seen_once(token: u32) -> ArrayItemWithToken<Item> {
    ArrayItemWithToken {
        token,
        item: Item {
            wc: 1,
            n_n_0: 1,
            n_1: 1,
            mr: 1,
        },
    }
}

/// `fixtures/w3/{db,tkt}/k_mixture_model.db` is the pin's own
/// `import_k_mixture_model` output (`fixtures/w3/README.md`); the Kyoto
/// Cabinet file is built record by record and keeps the text's unsorted
/// sentence-start row
/// (`docs/findings/eio-flexible-bigram-unsorted-records.md`).
///
/// The tokens, headers and items are the frozen utility case
/// `export_k_mixture_model::kmm-export-good`, the pin's
/// `export_k_mixture_model` of this file for each cell
/// (`utils/training/export_k_mixture_model.cpp:35-110` at libpinyin 074a2219):
/// its `\\1-gram` lines walk `get_all_items`, its `\\2-gram` lines each
/// token's `retrieve_all`.
#[test]
fn a_pin_written_file_is_read_in_the_pins_native_order() {
    const START: u32 = 1;
    const NIHAO: u32 = 16_802_309;
    const ZHONGGUO: u32 = 16_817_937;
    let (order, start_items) = if cfg!(feature = "kyotocabinet") {
        ([START, ZHONGGUO, NIHAO], [ZHONGGUO, NIHAO])
    } else {
        ([NIHAO, START, ZHONGGUO], [NIHAO, ZHONGGUO])
    };

    let model = open_read_only(&w3_cell().join("k_mixture_model.db"));
    assert_eq!(
        model.get_magic_header().expect("magic read"),
        Some(Magic {
            wc: 3,
            n: 1,
            total_freq: 3,
        })
    );
    assert_eq!(model.get_all_items().expect("token walk"), order);

    for (token, header, items) in [
        (
            START,
            Header { wc: 2, freq: 0 },
            start_items.map(seen_once).to_vec(),
        ),
        (NIHAO, Header { wc: 0, freq: 2 }, Vec::new()),
        (ZHONGGUO, Header { wc: 1, freq: 1 }, vec![seen_once(NIHAO)]),
    ] {
        assert_eq!(
            model.get_array_header(token).expect("array header"),
            Some(header.clone()),
            "array header of token {token}"
        );
        let gram = model.load(token).expect("load").expect("stored gram");
        assert_eq!(gram.header(), &header);
        assert_eq!(
            gram.items(),
            items.as_slice(),
            "loaded gram of token {token}"
        );
    }
}
