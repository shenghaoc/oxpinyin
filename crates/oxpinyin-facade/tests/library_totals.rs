//! The facade total that the model divides by, across the library
//! transitions the context owns: `mask_out`, `unload_phrase_library` and
//! the import iterator's close step (`end_import`).
use std::path::{Path, PathBuf};

use oxpinyin_facade::{ContextCore, MaskOutOutcome, PINYIN_DEFAULT_OPTION_WORD};
use oxpinyin_user::UserConfLaw;

const GBK: u8 = 2;

/// The committed fixture set the compiled backend opens.
fn fixture_dir() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/w3");
    root.join(oxpinyin_data::DEFAULT_STORE_EXT)
}

/// A private system directory under `parent`: every fixture file copied in,
/// so the test needs no symbolic-link privilege on any host.
fn private_system(parent: &Path) -> PathBuf {
    let dir = parent.join("system");
    std::fs::create_dir_all(&dir).expect("private system dir");
    for entry in std::fs::read_dir(fixture_dir()).expect("fixture dir") {
        let entry = entry.expect("fixture entry");
        std::fs::copy(entry.path(), dir.join(entry.file_name())).expect("copy a fixture file");
    }
    dir
}

fn open(system: &Path, user: &Path) -> ContextCore {
    ContextCore::open(
        system,
        Some(user),
        PINYIN_DEFAULT_OPTION_WORD,
        UserConfLaw::Pinyin,
    )
    .expect("open")
}

fn facade_total(context: &ContextCore) -> u64 {
    context.runtime.lm().amplified_total()
}

/// A context on a private system under `scratch` whose `gbk_char.bin`
/// (library 2) is a checksum-valid, in-bounds chunk that
/// `SubPhraseIndex::load` refuses at its second separator
/// (`phrase_index.cpp:355`). Its local total is T, the facade total of the
/// other libraries, which the facade does not add (`:242-246`). Answers the
/// context and T.
fn layout_failed_gbk_context(scratch: &Path) -> (ContextCore, u64) {
    let system = private_system(scratch);
    let user = scratch.join("user");
    std::fs::create_dir(&user).expect("user directory");
    let library = system.join("gbk_char.bin");
    let write = |total: u32| {
        let mut payload = [0_u8; 32];
        for (at, value) in [(0, total), (4, 17), (8, 20), (12, 24)] {
            payload[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        payload[16] = b'#';
        payload[19] = b'!';
        let chunk = oxpinyin_data::chunk_format::build_memory_chunk(&payload).expect("frame");
        std::fs::write(&library, chunk).expect("write gbk_char.bin");
    };
    write(0);
    let total = facade_total(&open(&system, &user));
    assert!(total > 0, "the other libraries hold a total");
    write(u32::try_from(total).expect("a guint32 total"));
    let context = open(&system, &user);
    assert_eq!(
        facade_total(&context),
        total,
        "the refused library adds nothing"
    );
    (context, total)
}

/// `mask_out` maps library 2 again: the failed load takes the old local
/// total T out of the facade total (`phrase_index.cpp:243-245`), and the
/// closing compact makes the reread local total zero (`pinyin.cpp:1294`).
/// The unload then takes out nothing, and the facade total stays T - T.
#[test]
fn an_unload_after_mask_out_takes_out_the_compacted_total() {
    let scratch = tempfile::tempdir().expect("scratch");
    let (mut context, _) = layout_failed_gbk_context(scratch.path());
    assert_eq!(context.mask_out(0xFFFF_FFFF, 0), MaskOutOutcome::Done(true));
    assert!(context.unload_phrase_library(GBK));
    assert_eq!(facade_total(&context), 0);
}

/// `pinyin_end_add_phrases` compacts the phrase index
/// (`pinyin.cpp:655-660`), and `compact` skips a missing sub-index
/// (`phrase_index.cpp:651-652`): the unloaded library's total does not move
/// into the facade's.
#[test]
fn closing_an_import_leaves_the_unloaded_librarys_total_alone() {
    let scratch = tempfile::tempdir().expect("scratch");
    let system = private_system(scratch.path());
    let user = scratch.path().join("user");
    std::fs::create_dir(&user).expect("user directory");
    let mut context = open(&system, &user);
    assert!(context.unload_phrase_library(GBK));
    let total = facade_total(&context);

    context.end_import();

    assert_eq!(facade_total(&context), total);
}
