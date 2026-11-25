use super::*;

fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/w3")
        .join(crate::__internal::DEFAULT_STORE_EXT)
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
    let old = lm.nbest_token_costs(&token).unwrap();
    let new = lm.nbest_token_costs_with_facts(&token, facts).unwrap();
    let bits = |costs: crate::model::NbestTokenCosts| {
        (
            costs.elem.map(f64::to_bits),
            costs.lambda.to_bits(),
            costs.unigram_lambda.to_bits(),
        )
    };
    assert_eq!(bits(old), bits(new));
    for item in [None, Some((3_u32, 7_u32)), Some((5, 7))] {
        let transition = item.map(|(freq, total)| (u64::from(freq), u64::from(total), true));
        let old_step = old.costs(transition);
        let new_step = new.costs(transition);
        assert_eq!(
            (
                old_step.blended.map(f64::to_bits),
                old_step.unigram.map(f64::to_bits),
                old_step.unigram_lambda.to_bits()
            ),
            (
                new_step.blended.map(f64::to_bits),
                new_step.unigram.map(f64::to_bits),
                new_step.unigram_lambda.to_bits()
            )
        );
        // The n-best step reads each branch from the memoised token costs.
        assert_eq!(
            item.and_then(|(freq, total)| new.blended(u64::from(freq), u64::from(total)))
                .map(f64::to_bits),
            new_step.blended.map(f64::to_bits)
        );
        assert_eq!(
            new.unigram().map(f64::to_bits),
            new_step.unigram.map(f64::to_bits)
        );
    }
}

// Basis: phonetic_lookup.h:643-697 at 074a2219 reads one item while each path supplies its pronunciation.
#[test]
fn pricing_and_scoring_share_one_item_on_two_paths() {
    let runtime = opened();
    let token = token(&runtime);
    let mut facts = SentenceFacts::default();
    for spelling in ["ni", "hao"] {
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
}

// Basis: each sentence invocation reads the committed training counts (Rust contract).
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
}

fn fallback(runtime: &Runtime, token: u32) {
    let mut facts = SentenceFacts::default();
    price(runtime, &path(runtime, "ni"), token, &mut facts);
    score(runtime, token, &mut facts);
    assert!(facts.tokens.get(&token).copied().flatten().is_none());
    assert!(facts.bytes.is_empty());
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
#[derive(Clone, Copy)]
enum Fallback {
    User,
    Imported,
    Missing,
    Removed,
    Unreadable,
}

// Basis: ordinary overlay items supply cached facts; other origins and failed reads use live resolution (Rust contract).
#[rstest::rstest]
#[case::user(Fallback::User)]
#[case::imported(Fallback::Imported)]
#[case::missing(Fallback::Missing)]
#[case::removed(Fallback::Removed)]
#[case::unreadable(Fallback::Unreadable)]
fn removed_and_unreadable_items_keep_old_resolution(#[case] case: Fallback) {
    match case {
        Fallback::User | Fallback::Imported => {
            let mut runtime = opened();
            let ni = SyllableKey::from_text("ni").unwrap();
            let keys = [crate::user::toned_key(ni.index(), 0).unwrap()];
            let store = runtime.user_store_mut();
            let token = if matches!(case, Fallback::User) {
                store.add_phrase_in(7, &"词".into(), &keys, Some(5))
            } else {
                store.add_system_phrase_in(1, None, &"龘".into(), &keys, Some(0))
            }
            .unwrap();
            fallback(&runtime, token);
        }
        Fallback::Missing => fallback(&opened(), 0x01ff_fffe),
        Fallback::Removed | Fallback::Unreadable => {
            use crate::data::user_files::LogRecord;
            let (_dir, runtime, token) = replayed(|token, old_item| {
                if matches!(case, Fallback::Removed) {
                    return LogRecord::Remove { token, old_item };
                }
                let mut new_item = old_item.clone();
                new_item[1] = 255;
                new_item.push(0);
                LogRecord::Modify {
                    token,
                    old_item,
                    new_item,
                }
            });
            fallback(&runtime, token);
        }
    }
}
