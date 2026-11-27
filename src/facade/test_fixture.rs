//! The committed `fixtures/w3` contexts and instances the instance, parse
//! and cursor tests share.

use crate::facade::{
    ContextCore, InstanceCore, PINYIN_DEFAULT_OPTION_WORD, ZHUYIN_DEFAULT_OPTION_WORD,
};
use crate::user::UserConfLaw;
use std::path::PathBuf;

/// The committed fixture directory the compiled backend opens.
pub(crate) fn fixture_dir() -> PathBuf {
    native_fixture::system_dir()
}

/// Opens the committed fixture set the compiled backend reads.
pub(crate) fn open_context(option_word: crate::OptionBits) -> ContextCore {
    ContextCore::open(&fixture_dir(), None, option_word, UserConfLaw::Pinyin)
        .expect("committed backend fixtures")
}

pub(crate) fn pinyin_instance() -> (ContextCore, InstanceCore) {
    let context = open_context(PINYIN_DEFAULT_OPTION_WORD);
    let instance = context.alloc_instance();
    (context, instance)
}

pub(crate) fn zhuyin_instance() -> (ContextCore, InstanceCore) {
    let context = open_context(ZHUYIN_DEFAULT_OPTION_WORD);
    let instance = context.alloc_instance();
    (context, instance)
}

#[cfg(feature = "tkrzw")]
pub(crate) use crate::user::contract_data;
pub(crate) use crate::user::native_fixture;

use crate::OptionBits;
use crate::engine::{CandidateKind, RowText, Selection};

pub(crate) fn native_context(
    system: &std::path::Path,
    user: &std::path::Path,
    law: UserConfLaw,
) -> ContextCore {
    let options = match law {
        UserConfLaw::Pinyin => PINYIN_DEFAULT_OPTION_WORD,
        UserConfLaw::Zhuyin => ZHUYIN_DEFAULT_OPTION_WORD,
    };
    ContextCore::open(system, Some(user), options, law).unwrap()
}

pub(crate) fn native_keys(reading: &str) -> Vec<crate::user::PinyinKey> {
    if !reading.is_ascii() {
        return crate::parser::parse_zhuyin_direct(reading, OptionBits::USE_TONE)
            .into_iter()
            .map(|(key, tone)| crate::user::toned_key(key.index(), tone).unwrap())
            .collect();
    }
    let graph =
        crate::graph::SegmentGraph::build_with_options(reading.as_bytes(), OptionBits::USE_TONE)
            .unwrap();
    assert_eq!(graph.consumed(), reading.len());
    graph
        .fewest_keys(false)
        .iter()
        .map(|edge| crate::user::toned_key(edge.key().index(), edge.tone()).unwrap())
        .collect()
}

pub(crate) fn plan(
    context: &ContextCore,
    library: u8,
    text: &str,
    reading: &str,
) -> crate::facade::ImportPlan {
    context.plan_add(library, text.into(), native_keys(reading))
}

pub(crate) fn import_native(
    context: &mut ContextCore,
    library: u8,
    rows: &[(&str, &str, u64)],
) -> Vec<bool> {
    let result = rows
        .iter()
        .map(|&(text, reading, count)| {
            let crate::facade::ImportPlan::Add {
                phrase,
                keys,
                original,
            } = plan(context, library, text, reading)
            else {
                return false;
            };
            context.commit_import(library, &phrase, &keys, original, Some(count))
        })
        .collect();
    context.end_import();
    result
}

pub(crate) fn tokens(context: &ContextCore, text: &str) -> Vec<u32> {
    let instance = context.alloc_instance();
    instance.read(&context.runtime).phrase_tokens(&text.into())
}

pub(crate) fn unigram(context: &ContextCore, token: u32) -> Option<u64> {
    context
        .alloc_instance()
        .read(&context.runtime)
        .token_unigram_frequency(token)
}

pub(crate) fn guess(context: &ContextCore, instance: &mut InstanceCore) -> bool {
    let mut view = instance.bind(&context.runtime);
    let has_keys = view.mode_has_keys();
    view.guess_sentence_with_matrix(has_keys).unwrap()
}

pub(crate) fn sentence(context: &ContextCore, instance: &InstanceCore, rank: u8) -> Option<String> {
    instance
        .read(&context.runtime)
        .sentence_phrases(rank)
        .map(|parts| {
            parts
                .filter_map(|part| part.text())
                .map(|part| part.as_str().unwrap().to_owned())
                .collect()
        })
}

/// The stored phrase/sentence rows; the C fallback conversion has its own tests.
pub(crate) fn native_window(
    context: &ContextCore,
    instance: &mut InstanceCore,
    offset: usize,
) -> Vec<crate::engine::Candidate> {
    let offset = native_offset(context, instance, offset);
    let mut view = instance.bind(&context.runtime);
    let options = view.options();
    view.session.set_options(options).unwrap();
    view.session.set_sort_options(0x1e).unwrap();
    view.session
        .candidates_at_for_abi(offset, false)
        .unwrap()
        .iter()
        .filter(|row| row.kind() != CandidateKind::Fallback)
        .cloned()
        .collect()
}

fn native_offset(context: &ContextCore, instance: &InstanceCore, offset: usize) -> usize {
    let view = instance.read(&context.runtime);
    let offset = view.validate_lookup_offset(offset).unwrap();
    if let Some(parse) = view.double_parse() {
        crate::facade::double_session_offset(parse, offset)
    } else if let Some(parse) = view.zhuyin_parse() {
        crate::facade::zhuyin_session_offset(parse, offset)
    } else {
        offset
    }
}

pub(crate) fn native_instance(context: &ContextCore, law: UserConfLaw) -> InstanceCore {
    let mut instance = context.alloc_instance();
    if law == UserConfLaw::Zhuyin {
        let mut view = instance.bind(&context.runtime);
        view.session.set_law(crate::engine::Law::Zhuyin);
    }
    instance
}

pub(crate) fn published(
    context: &ContextCore,
    instance: &mut InstanceCore,
    offset: usize,
    text: &str,
) -> crate::facade::PublishedRow {
    let window = native_window(context, instance, offset);
    let row = window
        .iter()
        .find(|row| row.kind() == CandidateKind::Phrase && row.text().as_str() == Some(text))
        .expect("normal row");
    let target = crate::engine::SelectionTarget::of_candidate(
        row,
        native_offset(context, instance, offset),
        None,
    );
    let mapper = instance.bind(&context.runtime).publication_bound_mapper();
    crate::facade::PublishedRow {
        begin: u16::try_from(offset).unwrap(),
        end: u16::try_from(mapper(target.span_end)).unwrap(),
        token: target.token.unwrap(),
        text: RowText::Resolve,
    }
}

pub(crate) fn choose(
    context: &ContextCore,
    instance: &mut InstanceCore,
    row: &crate::facade::PublishedRow,
) -> Selection {
    instance
        .bind(&context.runtime)
        .select_published(row)
        .unwrap()
}

pub(crate) fn export_native(context: &ContextCore, library: u32) -> Vec<(String, String, u64)> {
    let mut cursor = context.phrase_export_cursor(library);
    let mut result = Vec::new();
    while cursor.has_next() {
        let crate::facade::ExportStep::Row(row) = cursor.get_next_with(context, |row| {
            let text = row
                .with_text_bytes(|bytes| String::from_utf8(bytes.to_vec()).unwrap())
                .unwrap();
            let pinyin = row
                .pinyin_strings()
                .map(|(syllable, tone)| {
                    tone.map_or_else(|| syllable.to_owned(), |tone| format!("{syllable}{tone}"))
                })
                .collect::<Vec<_>>()
                .join("'");
            (text, pinyin, row.count())
        }) else {
            panic!("readable export")
        };
        result.push(row);
    }
    result
}

pub(crate) fn saved_user_phrase(user: &std::path::Path) -> u32 {
    let mut context = native_context(&fixture_dir(), user, UserConfLaw::Pinyin);
    assert_eq!(
        import_native(&mut context, 7, &[("龘龘", "da2'da2", 5)]),
        [true]
    );
    let token = tokens(&context, "龘龘")[0];
    assert_eq!(context.save_user(), crate::facade::SaveOutcome::Saved);
    context.finish();
    token
}

#[cfg(feature = "tkrzw")]
pub(crate) fn train_once(context: &mut ContextCore) {
    let mut instance = context.alloc_instance();
    assert_eq!(instance.bind(&context.runtime).parse_full_more("nihao"), 5);
    assert!(guess(context, &mut instance));
    assert!(context.train(&instance, 0).unwrap());
}

pub(crate) fn files_of(dir: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().unwrap().is_file())
        .map(|entry| {
            (
                entry.file_name().to_str().unwrap().to_owned(),
                std::fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

/// The caller keeps the scratch directory while it checks child output and files.
pub(crate) fn child(
    name: &str,
    body: impl FnOnce(&std::path::Path),
) -> Option<(tempfile::TempDir, std::path::PathBuf, std::process::Output)> {
    const KEY: &str = "OXPINYIN_FACADE_NATIVE_CHILD";
    if let Some(user) = std::env::var_os(KEY) {
        body(std::path::Path::new(&user));
        return None;
    }
    let scratch = tempfile::tempdir().unwrap();
    let user = scratch.path().join("user");
    std::fs::create_dir(&user).unwrap();
    let output = crate::facade::test_child::run_exact(name, [(KEY, user.as_os_str())]);
    assert!(output.status.success(), "{output:?}");
    Some((scratch, user, output))
}
