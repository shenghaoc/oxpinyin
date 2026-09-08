//! Crash-consistency and hostile-file laws for the user store — G3 of
//! the testing-strategy assessment (`testing-strategy.md` §2): the
//! semantics suite pins the algebra, but nothing exercised the store
//! under failure.
//!
//! What is tested, and what deliberately is not:
//!
//! - **Abort mid-batch.** A child process writes a fixed prefix of a
//!   known batch through the public API (each `add_phrase` is one
//!   backend transaction) and then `abort()`s partway — no graceful
//!   drop, no `save()`.
//!   The parent reopens the file and asserts the crash-consistency law
//!   the architecture actually promises: the visible rows are exactly a
//!   *prefix* of the batch, every row is internally complete (token,
//!   text, and pronunciation index agree both ways), and no torn or
//!   foreign row survived. Backends may lose any suffix of unflushed
//!   transactions; they may never tear one. The assessment's sketch
//!   ("one transaction of 1000 phrases, abort after 500") is not
//!   expressible through the public API — there is no multi-phrase
//!   transaction — so the batch-level equivalent is asserted instead.
//!
//! - **Garbage and truncated files.** Opening a store file that is not
//!   a store must be a typed [`UserStoreError`] (or, on a backend that
//!   recovers, a working empty store) — never a panic. The assessment's
//!   "format version +1" case has no counterpart by ruling: the user
//!   store carries **no format-version field**, matching libpinyin's
//!   unversioned user files (a `user_meta` row was added and reverted
//!   on 2026-09-08; `docs/findings/user-store.md` §4 records the
//!   ruling). The backend's own file header is the only stamp, so the
//!   garbage-file law is the honest equivalent at this seam.
//!
//! The child runs this same test binary under `--exact` with
//! `OXPINYIN_USER_ACID_CHILD` naming the store path; the test itself
//! branches on the variable, so the child never recurses.

use std::path::PathBuf;
use std::process::Command;

use oxpinyin_core::SyllableKey;
use oxpinyin_user::{PinyinKey, UserStore};

const CHILD_VAR: &str = "OXPINYIN_USER_ACID_CHILD";
const BATCH: usize = 64;
/// The child dies after this many writes, so the crash lands mid-batch
/// and the survivors are bounded by a prefix shorter than the batch.
const ABORT_AT: usize = 32;

fn temp_path(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "oxpinyin-user-acid-{tag}-{}.store",
        std::process::id()
    ));
    cleanup(&path);
    path
}

fn cleanup(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir_all(path);
    let lock = format!("{}-lock", path.display());
    let _ = std::fs::remove_file(&lock);
    let _ = std::fs::remove_dir_all(&lock);
}

fn key(text: &str) -> PinyinKey {
    let index = SyllableKey::from_text(text)
        .unwrap_or_else(|| panic!("{text:?} must be a frozen syllable"))
        .index();
    PinyinKey::try_from(index).expect("frozen syllable inventory fits u16")
}

/// Phrase `i` of the batch: deterministic text and keys, regenerable by
/// the parent for the prefix comparison.
fn batch_phrase(index: usize) -> (String, Vec<PinyinKey>) {
    let syllables = ["shi", "yan", "zheng", "kao", "mi", "wen"];
    let text = format!("试{index:03}");
    let keys = (0..text.chars().count())
        .map(|slot| key(syllables[(index + slot) % syllables.len()]))
        .collect();
    (text, keys)
}

/// The child role: write a fixed prefix of the batch, then die without
/// unwinding.
fn run_child(path: &std::path::Path) {
    let mut store = UserStore::create_standalone(path).expect("child store opens");
    for index in 0..ABORT_AT {
        let (text, keys) = batch_phrase(index);
        store.add_phrase(&text, &keys, None).expect("child add");
    }
    // The crash under test: no drop, no save, no unwinding.
    std::process::abort();
}

/// A hard crash mid-batch leaves exactly a prefix of the batch, with
/// every surviving row internally consistent.
#[test]
fn abort_mid_batch_leaves_a_consistent_prefix() {
    let path = temp_path("abort");
    if let Some(child_path) = std::env::var_os(CHILD_VAR) {
        run_child(&PathBuf::from(child_path));
        return;
    }

    let exe = std::env::current_exe().expect("test binary path");
    let status = Command::new(exe)
        .args([
            "--exact",
            "abort_mid_batch_leaves_a_consistent_prefix",
            "--test-threads=1",
            "--nocapture",
        ])
        .env(CHILD_VAR, path.as_os_str())
        .status()
        .expect("child spawns");
    assert!(
        !status.success(),
        "the child must die from abort(), not exit cleanly (got {status:?})"
    );

    // Reopen what the crash left behind. A stale lock sidecar belongs
    // to a dead process; a backend that cannot recover it fails here,
    // which is exactly the finding this test exists to surface.
    let store = UserStore::open(&path).expect("the crashed store reopens");
    let visible = store.phrases().expect("the crashed store reads");
    let texts: Vec<String> = visible.iter().map(|row| row.text().to_owned()).collect();
    assert!(
        texts.len() <= ABORT_AT,
        "the crash cannot leave more rows than the child wrote before aborting"
    );

    // The prefix law, order-insensitive over the row iteration: whatever
    // suffix the backend lost, the survivors are exactly the first
    // `texts.len()` phrases of the batch. Torn rows, reordered rows, or
    // foreign rows all fail this.
    let expected: std::collections::BTreeSet<String> =
        (0..BATCH).map(|index| batch_phrase(index).0).collect();
    let prefix: std::collections::BTreeSet<String> =
        expected.into_iter().take(texts.len()).collect();
    let survivors: std::collections::BTreeSet<String> = texts.into_iter().collect();
    assert_eq!(
        survivors, prefix,
        "the crash must leave a batch prefix: torn rows, reordered rows, or \
         foreign rows all fail this"
    );

    // Every surviving row is internally complete: the token and text
    // indexes agree in both directions, and the pronunciation block
    // decodes with the right length.
    for row in &visible {
        let token = row.token();
        let text = row.text();
        assert!(
            oxpinyin_user::is_user_token(token),
            "token {token} must be a user token"
        );
        assert_eq!(
            store.token_for_phrase(text).expect("text index reads"),
            Some(token),
            "text {text:?} must resolve to its own token"
        );
        let index: usize = text["试".len()..]
            .parse()
            .expect("a surviving row carries its batch number");
        let (_, keys) = batch_phrase(index);
        assert_eq!(
            row.pronunciations().len(),
            1,
            "each batch phrase has exactly one pronunciation"
        );
        assert_eq!(
            row.pronunciations()[0].keys(),
            keys.as_slice(),
            "the pronunciation must decode to the written keys"
        );
        assert!(
            store.next_user_token().expect("cursor reads") > token,
            "the allocation cursor must stay past every allocated token"
        );
    }

    cleanup(&path);
}

/// A file of garbage bytes where a store should be must refuse to open
/// (or open clean) — never panic — and the refusal is the typed error.
#[test]
fn a_garbage_store_file_is_a_typed_error() {
    let path = temp_path("garbage");
    let garbage: Vec<u8> = (0..4096_u32).map(|i| (i * 31 % 251) as u8).collect();
    std::fs::write(&path, garbage).expect("garbage writes");

    match UserStore::open(&path) {
        // The expected shape on every current backend: the file fails
        // the backend's header check and open refuses.
        Err(_typed) => {}
        // A backend that recovers instead must still answer reads over
        // the empty store it found.
        Ok(store) => {
            assert!(store.phrases().expect("recovered store reads").is_empty());
        }
    }
    cleanup(&path);
}

/// A valid store truncated to half its size is the same law one step
/// closer to home: real torn writes, not just wrong magic.
#[test]
fn a_truncated_store_file_is_a_typed_error() {
    let path = temp_path("truncated");
    {
        let mut store = UserStore::create_standalone(&path).expect("store creates");
        for index in 0..4 {
            let (text, keys) = batch_phrase(index);
            store.add_phrase(&text, &keys, None).expect("seed add");
        }
        store.save().expect("seed save");
    }
    let full = std::fs::metadata(&path).expect("seeded file stats").len();
    let body = std::fs::read(&path).expect("seeded file reads");
    std::fs::write(&path, &body[..(full as usize / 2)]).expect("truncated write");

    match UserStore::open(&path) {
        Err(_typed) => {}
        Ok(store) => {
            // Whatever survived must still be internally consistent:
            // every row decodes and the index agrees both ways.
            for row in store.phrases().expect("truncated store reads") {
                assert_eq!(
                    store
                        .token_for_phrase(row.text())
                        .expect("text index reads"),
                    Some(row.token()),
                    "a surviving row must stay self-consistent"
                );
            }
        }
    }
    cleanup(&path);
}
