use super::*;

fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/w3")
        .join(crate::store::DEFAULT_STORE_EXT)
}
fn opened() -> Runtime {
    Runtime::open(&fixture(), None).unwrap()
}
fn token(runtime: &Runtime) -> u32 {
    runtime.dict().system().tokens_for_text("你").unwrap()[0]
}
fn path(runtime: &Runtime, spelling: &str) -> PathPricing {
    runtime
        .dict()
        .path_pricing(&[SyllableKey::from_text(spelling).unwrap()], &[])
        .unwrap()
}
fn price(runtime: &Runtime, path: &PathPricing, token: u32, facts: &mut SentenceFacts) {
    let old = runtime.dict().priced_on_path(path, token, true);
    let new = runtime.dict().priced_with_facts(path, token, true, facts);
    assert_eq!(
        old.as_ref()
            .map(|p| (p.item.chars, p.item.pronunciation, p.text.clone())),
        new.as_ref()
            .map(|p| (p.item.chars, p.item.pronunciation, p.text.clone()))
    );
}
fn score(runtime: &Runtime, token: u32, facts: &mut SentenceFacts) {
    let lm = runtime.lm();
    for item in [None, Some((0, 0)), Some((3, 7)), Some((5, 7))] {
        let old = lm
            .nbest_item_costs_in_row(&1, &token, item, &mut Default::default())
            .unwrap();
        let new = lm
            .nbest_item_costs_with_facts(&1, &token, item, &mut Default::default(), facts)
            .unwrap();
        assert_eq!(
            (
                old.blended.map(f64::to_bits),
                old.unigram.map(f64::to_bits),
                old.unigram_lambda.to_bits()
            ),
            (
                new.blended.map(f64::to_bits),
                new.unigram.map(f64::to_bits),
                new.unigram_lambda.to_bits()
            )
        );
    }
}
#[test]
fn pricing_and_scoring_share_one_item_on_two_paths() {
    let runtime = opened();
    let token = token(&runtime);
    let mut facts = SentenceFacts::default();
    for spelling in ["ni", "hao", "ni"] {
        price(&runtime, &path(&runtime, spelling), token, &mut facts);
        score(&runtime, token, &mut facts);
        let saved = facts.tokens.get(&token).copied().flatten().unwrap();
        assert_eq!(facts.tokens.len(), 1);
        assert_eq!(
            facts.bytes.len(),
            saved.len,
            "one copy despite different paths and transitions"
        );
        assert!(facts.total.is_some());
    }
    let ni = runtime
        .dict()
        .priced_with_facts(&path(&runtime, "ni"), token, false, &mut facts)
        .unwrap();
    let hao = runtime
        .dict()
        .priced_with_facts(&path(&runtime, "hao"), token, false, &mut facts)
        .unwrap();
    assert_ne!(ni.item.pronunciation, hao.item.pronunciation);
    assert_eq!(std::mem::size_of::<TokenFacts>(), 32);
}
#[test]
fn next_guess_reads_committed_training() {
    let mut runtime = opened();
    let token = token(&runtime);
    let mut first = SentenceFacts::default();
    price(&runtime, &path(&runtime, "ni"), token, &mut first);
    let before = first.tokens[&token].unwrap().unigram;
    runtime
        .user_store_mut()
        .observe_selection(1, token)
        .unwrap();
    let mut next = SentenceFacts::default();
    price(&runtime, &path(&runtime, "ni"), token, &mut next);
    assert!(next.tokens[&token].unwrap().unigram > before);
    score(&runtime, token, &mut next);
    // Even a mistakenly reused context retires rather than serving old facts.
    score(&runtime, token, &mut first);
    assert!(first.tokens.is_empty());
}

#[test]
fn direct_unigram_updates_retire_held_sentence_facts() {
    let mut runtime = opened();
    let token = token(&runtime);
    let path = path(&runtime, "ni");
    let mut facts = SentenceFacts::default();
    price(&runtime, &path, token, &mut facts);
    score(&runtime, token, &mut facts);
    let before = facts.tokens[&token].unwrap().unigram;
    let total = facts.total.unwrap();

    assert!(runtime.add_unigram_outcome(token, 7).is_accepted());
    score(&runtime, token, &mut facts);
    assert!(facts.tokens.is_empty());
    assert!(facts.total.is_none());

    let mut facts = SentenceFacts::default();
    price(&runtime, &path, token, &mut facts);
    score(&runtime, token, &mut facts);
    assert_eq!(facts.tokens[&token].unwrap().unigram, before + 7);
    assert_eq!(facts.total, Some(total + 7));

    // A missing slot in a loaded library still changes the facade total.
    let missing = (token & 0x0f00_0000) | 0x00ff_ffff;
    assert_eq!(
        runtime.add_unigram_outcome(missing, 11),
        crate::user::UnigramAdd::Missing
    );
    score(&runtime, token, &mut facts);
    assert!(facts.tokens.is_empty());
    assert!(facts.total.is_none());

    let mut facts = SentenceFacts::default();
    price(&runtime, &path, token, &mut facts);
    score(&runtime, token, &mut facts);
    assert_eq!(facts.tokens[&token].unwrap().unigram, before + 7);
    assert_eq!(facts.total, Some(total + 18));
}

fn fallback(runtime: &Runtime, token: u32) {
    let mut facts = SentenceFacts::default();
    price(runtime, &path(runtime, "ni"), token, &mut facts);
    score(runtime, token, &mut facts);
    assert!(facts.tokens.get(&token).copied().flatten().is_none());
    assert!(facts.bytes.is_empty());
}
#[test]
fn user_imported_masked_missing_and_detached_keep_old_readers() {
    let mut runtime = opened();
    let ni = SyllableKey::from_text("ni").unwrap();
    let keys = [crate::user::toned_key(ni.index(), 0).unwrap()];
    let user = runtime
        .user_store_mut()
        .add_phrase_in(7, &"词".into(), &keys, Some(5))
        .unwrap();
    fallback(&runtime, user);
    let imported = runtime
        .user_store_mut()
        .add_system_phrase_in(1, None, &"龘".into(), &keys, Some(0))
        .unwrap();
    fallback(&runtime, imported);
    fallback(&runtime, 0x01ff_fffe);
    let ordinary = token(&runtime);
    runtime.library_mask.set(1 << (ordinary >> 24));
    fallback(&runtime, ordinary);
    runtime.library_mask.set(0);
    // A session detached from this runtime's system libraries.
    runtime.user = UserStore::open_transient(
        crate::user::SystemOriginals::new(
            Default::default(),
            crate::data::user_files::UserFileLayout::stock(),
        ),
        crate::data::user_files::SystemVersions::for_this_build(7, 14),
        crate::user::UserConfLaw::Pinyin,
    )
    .unwrap();
    fallback(&runtime, ordinary);
    let (_, libraries) = SystemDictionary::open_with(&fixture(), runtime.table_conf())
        .unwrap()
        .into_parts();
    runtime.install_baselines(crate::user::system_originals(
        libraries,
        UserFileLayout::from_conf(runtime.table_conf()),
    ));
    assert_eq!(
        runtime
            .dict()
            .live_items()
            .read(ordinary, |item| item.origin())
            .found(),
        Some(ItemOrigin::Shipped)
    );
    fallback(&runtime, ordinary);
}
fn replayed(
    change: impl FnOnce(u32, Vec<u8>) -> crate::data::user_files::LogRecord,
) -> (tempfile::TempDir, Runtime, u32) {
    let runtime = opened();
    let token = token(&runtime);
    let old = runtime
        .dict()
        .live_items()
        .read(token, |item| item.as_bytes().to_vec())
        .found()
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let payload = crate::data::user_files::encode_log_records(&[change(
        token & crate::vocab::PHRASE_MASK,
        old,
    )])
    .unwrap();
    std::fs::write(
        dir.path().join("gb_char.dbin"),
        crate::data::chunk_format::build_memory_chunk(&payload).unwrap(),
    )
    .unwrap();
    assert!(
        crate::data::user_files::UserTableInfo::conform_to(&SystemVersions::from_conf(
            runtime.table_conf(),
        ))
        .save(dir.path().join("user.conf").as_path())
    );
    drop(runtime);
    let runtime = Runtime::open(&fixture(), Some(dir.path())).unwrap();
    (dir, runtime, token)
}
#[test]
fn present_zero_unigram_is_a_live_fact() {
    let (_dir, runtime, token) = replayed(|token, old_item| {
        let mut new_item = old_item.clone();
        new_item[2..6].copy_from_slice(&0u32.to_le_bytes());
        crate::data::user_files::LogRecord::Modify {
            token,
            old_item,
            new_item,
        }
    });
    let mut facts = SentenceFacts::default();
    price(&runtime, &path(&runtime, "ni"), token, &mut facts);
    assert_eq!(facts.tokens[&token].unwrap().unigram, 0);
    score(&runtime, token, &mut facts);
}
#[test]
fn removed_and_unreadable_items_keep_old_resolution() {
    let (_dir, runtime, token) =
        replayed(|token, old_item| crate::data::user_files::LogRecord::Remove { token, old_item });
    fallback(&runtime, token);
    let (_dir, runtime, token) = replayed(|token, old_item| {
        let mut new_item = old_item.clone();
        new_item[1] = 255;
        // Force remove-and-append, so the declared readings run past
        // the live buffer rather than into the next ordinary item.
        new_item.push(0);
        crate::data::user_files::LogRecord::Modify {
            token,
            old_item,
            new_item,
        }
    });
    fallback(&runtime, token);
}
