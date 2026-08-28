//! Exact-scheme-key regression tests: keys parsed by the zhuyin and
//! double-pinyin parsers must reach the decoder verbatim, never
//! re-segmented by the pinyin inventory
//! (`docs/findings/bopomofo-spec.md`, the exact seam).
//!
//! The discriminator is the mini tables' deliberate `xian` / `xi'an`
//! pair: bare full-pinyin `xian` legitimately enumerates the `xi`+`an`
//! segmentation as well (that ambiguity is why the `'` format exists),
//! so 西安/锡安/西岸 appear in its window — while the zhuyin key ㄒㄧㄢ
//! resolves to exactly one key whose lookup is `xian` alone. Before the
//! exact seam, the zhuyin path drove the decoder with the joined text
//! and offered the `xi'an` phrases too.

use std::os::raw::c_uint;

use pinyin_capi::{
    LookupCandidate, PinyinInstance, pinyin_get_candidate, pinyin_get_candidate_string,
    pinyin_get_n_candidate, pinyin_guess_candidates, pinyin_parse_more_chewings,
    pinyin_parse_more_full_pinyins,
};

use crate::common::{TempUserDir, cstr, open};

/// Collects the candidate texts for the current composition.
fn candidate_texts(instance: *mut PinyinInstance) -> Vec<String> {
    assert!(pinyin_guess_candidates(instance, 0, 0x1e));
    let mut count: c_uint = 0;
    assert!(pinyin_get_n_candidate(instance, &mut count));
    let mut out = Vec::new();
    for index in 0..count {
        let mut cand: *mut LookupCandidate = std::ptr::null_mut();
        assert!(pinyin_get_candidate(instance, index, &mut cand));
        let mut text: *const pinyin_capi::GChar = std::ptr::null();
        assert!(pinyin_get_candidate_string(instance, cand, &mut text));
        assert!(!text.is_null());
        // SAFETY: `text` was just returned by `pinyin_get_candidate_string`
        // as a valid, NUL-terminated pointer into the instance snapshot.
        out.push(
            unsafe { std::ffi::CStr::from_ptr(text) }
                .to_string_lossy()
                .into_owned(),
        );
    }
    out
}

#[test]
fn zhuyin_keys_are_not_resegmented_by_the_pinyin_inventory() {
    let user_dir = TempUserDir::new("exact-zhuyin-xian");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    // STANDARD (dachen) keyboard: v=ㄒ u=ㄧ 0=ㄢ → the single key ㄒㄧㄢ (xian).
    let vu0 = cstr("vu0");
    assert_eq!(pinyin_parse_more_chewings(instance, vu0.as_ptr()), 3);
    let zhuyin = candidate_texts(instance);

    // The xian rows are present …
    assert!(
        zhuyin.iter().any(|text| text == "现"),
        "xian row missing: {zhuyin:?}"
    );
    assert!(
        zhuyin.iter().any(|text| text == "先"),
        "xian row missing: {zhuyin:?}"
    );
    // … and the xi'an-only phrases are not: the zhuyin parse made one
    // exact key, and no bi+e-style re-segmentation may widen it.
    assert!(
        !zhuyin
            .iter()
            .any(|text| text == "西安" || text == "锡安" || text == "西岸"),
        "xi'an-only phrases leaked into the zhuyin window: {zhuyin:?}"
    );

    // The full-pinyin contrast: bare `xian` (no apostrophe) legitimately
    // enumerates the xi+an segmentation as well, so its window carries
    // the xi'an phrases. This pins that the exact seam narrowed the
    // scheme path only — the full-pinyin path policy is untouched.
    let xian = cstr("xian");
    assert_eq!(pinyin_parse_more_full_pinyins(instance, xian.as_ptr()), 4);
    let full = candidate_texts(instance);
    assert!(
        full.iter().any(|text| text == "西安"),
        "full-pinyin xian lost its xi+an segmentation: {full:?}"
    );

    pinyin_capi::pinyin_free_instance(instance);
    pinyin_capi::pinyin_fini(context);
}

#[test]
fn zhuyin_only_spellings_do_not_fall_back_to_shorter_pinyin_keys() {
    let user_dir = TempUserDir::new("exact-zhuyin-den");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));

    // ㄉㄣ (2 p) is a recovered zhuyin spelling whose pinyin string "den"
    // exists in no pinyin inventory and carries no dictionary rows. The
    // exact seam must keep the key whole — an empty window — instead of
    // re-parsing the text as `de` plus leftovers.
    let den = cstr("2p");
    assert_eq!(pinyin_parse_more_chewings(instance, den.as_ptr()), 2);
    let texts = candidate_texts(instance);
    assert!(
        !texts.iter().any(|text| text == "的" || text == "得"),
        "den re-parsed as de: {texts:?}"
    );

    pinyin_capi::pinyin_free_instance(instance);
    pinyin_capi::pinyin_fini(context);
}
