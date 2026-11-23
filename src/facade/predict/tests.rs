use super::*;
use crate::facade::{ContextCore, PINYIN_DEFAULT_OPTION_WORD};
use crate::user::{UserConfLaw, toned_key};
use crate::vocab::SyllableKey;

// 074a2219 pinyin.cpp:1413–1415,2389–2395 searches live prefix and suggestion tables.
#[test]
fn prediction_reads_imports_after_its_first_query() {
    let user = tempfile::tempdir().unwrap();
    let system = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/w3")
        .join(crate::__internal::DEFAULT_STORE_EXT);
    let mut ctx = ContextCore::open(
        &system,
        Some(user.path()),
        PINYIN_DEFAULT_OPTION_WORD,
        UserConfLaw::Pinyin,
    )
    .unwrap();
    let prefix = PhraseText::from("龘");
    let dict = ctx.runtime.dict();
    assert!(merged_suggestions(&dict, ctx.user_store(), &prefix).is_empty());
    let key = toned_key(SyllableKey::from_text("da").unwrap().index(), 2).unwrap();
    let token = ctx
        .user_store_mut()
        .add_phrase(&PhraseText::from("龘龘"), &[key, key], Some(7))
        .unwrap();
    let rows = merged_suggestions(&ctx.runtime.dict(), ctx.user_store(), &prefix);
    assert!(rows.iter().any(|(found, _)| *found == token), "{rows:?}");
}

mod native {
    use crate::OptionBits;
    use crate::facade::test_fixture::{native_fixture::*, *};
    use crate::facade::{ContextCore, InstanceCore};
    use crate::user::UserConfLaw;

    fn prefix_guess(context: &ContextCore, instance: &mut InstanceCore, prefix: &str) -> bool {
        let prefixes = crate::facade::compute_prefixes(
            &context.runtime.dict(),
            context.user_store(),
            Some(&prefix.into()),
        )
        .unwrap();
        let mut view = instance.bind(&context.runtime);
        let has_keys = view.mode_has_keys();
        view.guess_sentence_with_prefix_and_matrix(&prefixes, has_keys)
            .unwrap()
    }

    // Rust contract: a prefix seeds a sentence guess on an active matrix.
    #[test]
    fn prefix_seeded_sentence_guess() {
        let (context, mut instance) = pinyin_instance();
        assert_eq!(instance.bind(&context.runtime).parse_full_more("nihao"), 5);
        assert!(prefix_guess(&context, &mut instance, "你好"));
        assert!(sentence(&context, &instance, 0).unwrap().contains("你好"));
        let (context, mut fresh) = pinyin_instance();
        assert!(!prefix_guess(&context, &mut fresh, "你好"));
    }

    // 074a2219 zhuyin.cpp:1146–1175,1473–1489 ranks with the longest retained prefix.
    #[test]
    fn dynamic_ranking_uses_the_longest_retained_prefix() {
        let user = TempUserDir::new("prefix-ranking");
        let mut context = native_context(&system_dir(), &user.path, UserConfLaw::Zhuyin);
        let mut instance = native_instance(&context, UserConfLaw::Zhuyin);
        let [short, long, hao, number] =
            ["好", "你好", "好", "号"].map(|text| tokens(&context, text)[0]);
        for (previous, next) in [
            (crate::vocab::SENTENCE_START, hao),
            (short, hao),
            (long, number),
        ] {
            crate::user::seed_bigram(context.user_store_mut(), previous, &[(next, 1_000_000)]);
        }
        context.set_options(OptionBits::DYNAMIC_ADJUST);
        assert!(!prefix_guess(&context, &mut instance, "你好"));
        assert_eq!(
            instance
                .bind(&context.runtime)
                .parse_full_more_zhuyin("hao"),
            3
        );
        let rank = |context: &ContextCore, instance: &mut InstanceCore| {
            let window = native_window(context, instance, 0);
            let position = |text| {
                window
                    .iter()
                    .position(|row| row.text().as_str() == Some(text))
                    .unwrap()
            };
            position("号") < position("好")
        };
        assert!(rank(&context, &mut instance));
        context.set_options(OptionBits::empty());
        assert!(!rank(&context, &mut instance));
        context.set_options(OptionBits::DYNAMIC_ADJUST);
        assert!(rank(&context, &mut instance));
        assert_eq!(
            instance.bind(&context.runtime).parse_full_more_zhuyin(""),
            0
        );
        assert!(!guess(&context, &mut instance));
        assert_eq!(
            instance
                .bind(&context.runtime)
                .parse_full_more_zhuyin("hao"),
            3
        );
        assert!(!rank(&context, &mut instance));
        assert!(prefix_guess(&context, &mut instance, "你好"));
        instance.bind(&context.runtime).full_reset();
        assert_eq!(
            instance
                .bind(&context.runtime)
                .parse_full_more_zhuyin("hao"),
            3
        );
        assert!(!rank(&context, &mut instance));
    }
}
