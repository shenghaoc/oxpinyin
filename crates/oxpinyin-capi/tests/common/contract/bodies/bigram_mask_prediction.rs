//! The bigram_mask_prediction family's case bodies: each answers the result document the old case returned.
//!
//! The user bigram under `mask_out`, export and prediction (inventory row 34,
//! #525, #677, #1114, #1231): malformed planted grams, colliding hash keys,
//! a lazily created user bigram and predicted-bigram rows.

use std::ffi::c_char;
use std::path::Path;

use crate::bodies::case_helpers::{Trained, token_unigram_trained};
use crate::facade;
use crate::fixtures::{
    craft_hash, crafted_bigram_context, gram_bytes, hex_bytes, index_file_rows,
    tkrzw_saved_bigram_bytes, write_profile, write_user_conf,
};
use crate::readers::{c, owned_out, predicted_rows, reopen, row_of, seeded, tokens_of, unigram_of};
use crate::support::{J, Kit, P, Raw, borrowed_text};

/// The body of a case whose reference holds the pin-written handoff profile.
pub fn case_with_profile(k: &mut Kit, id: &str, profile: &Raw) -> J {
    match id {
        "issue1114-collision-load-save-pinyin" | "issue1114-collision-load-save-zhuyin" => {
            issue1114_collision_saves(k, profile)
        }
        "lazy-user-bigram-pinyin" | "lazy-user-bigram-zhuyin" => lazy_user_bigram(k, profile),
        other => panic!("no profile case {other}"),
    }
}

/// The pin-written profile, written to `handoff-pin-X` (the old verify's donor).
fn pin_donor(k: &Kit, profile: &Raw) -> std::path::PathBuf {
    let donor = k.scratch.join("handoff-pin-X");
    std::fs::create_dir(&donor).expect("the pin's handoff profile");
    write_profile(profile, &donor);
    donor
}

/// The case body for `id`.
pub fn case(k: &mut Kit, id: &str) -> J {
    match id {
        "predicted-bigram-is-user-candidate" => predicted_is_user(k),
        "abort-mask-out-short-bigram-value" | "abort-zhuyin-mask-out-short-bigram-value" => {
            mask_crafted(k, &[(vec![1, 0, 0, 0], vec![7, 0, 0])], 0)
        }
        "mask-out-short-bigram-value-masked-key" => {
            mask_crafted(k, &[(vec![1, 0, 0, 0], vec![7, 0, 0])], 1)
        }
        "import-after-partial-bigram-mask-pinyin" | "import-after-partial-bigram-mask-zhuyin" => {
            import_after_partial_mask(k)
        }
        "abort-mask-out-non-token-bigram-key" | "abort-zhuyin-mask-out-non-token-bigram-key" => {
            mask_crafted(k, &[(vec![0, 1], vec![7, 0, 0, 0])], 0)
        }
        "abort-mask-out-residual-bigram-gram" | "abort-zhuyin-mask-out-residual-bigram-gram" => {
            let value = [&[7_u8, 0, 0, 0][..], &[0; 8]].concat();
            mask_crafted(k, &[(vec![1, 0, 0, 0], value)], 0)
        }
        "abort-begin-bigram-phrases-non-token-key" => {
            let ctx = crafted_bigram_context(k, &[(vec![0, 1], vec![7, 0, 0, 0])]);
            obj! { "ret" => !facade::begin_get_bigram_phrases(ctx).is_null() }
        }
        "abort-bigram-export-short-predecessor" => bigram_export_short_predecessor(k),
        "issue677-mask-save-sequence-pinyin" | "issue677-mask-save-sequence-zhuyin" => {
            issue677_sequence(k)
        }
        other => panic!("no bigram_mask_prediction case {other}"),
    }
}

/// A crafted-bigram context's `mask_out(ctx, 0xFFFFFFFF, value)`.
fn mask_crafted(k: &mut Kit, rows: &[(Vec<u8>, Vec<u8>)], value: u32) -> J {
    let ctx = crafted_bigram_context(k, rows);
    obj! { "ret" => facade::mask_out(ctx, 0xFFFF_FFFF, value) }
}

/// Imports `text` under `reading` into `library` of `ctx` with `count`,
/// answering the add.
fn import(ctx: P, library: u8, text: &str, reading: &str, count: i32) -> bool {
    let it = facade::begin_add_phrases(ctx, library);
    assert!(!it.is_null(), "the import iterator");
    let added = facade::iterator_add_phrase(it, c(text).as_ptr(), c(reading).as_ptr(), count);
    facade::end_add_phrases(it);
    added
}

/// The facade's reading: pinyin or zhuyin.
fn reading<'a>(pinyin: &'a str, zhuyin: &'a str) -> &'a str {
    if facade::ZHUYIN { zhuyin } else { pinyin }
}

/// `tkrzw_saved_bigram_bytes`: saved HashDBM bytes with only the timestamp
/// (`META_OFFSET_TIMESTAMP`, bytes 48-55) zeroed.
fn masked_bigram_bytes(raw: &[u8]) -> Vec<u8> {
    assert!(
        raw.len() >= 128,
        "saved HashDBM is shorter than its metadata"
    );
    let mut masked = raw.to_vec();
    masked[48..56].fill(0);
    masked
}

/// `issue1114_collision_saves`' fixed rows: three keys in TinyDBM bucket 74121.
fn collision_rows() -> Vec<(Vec<u8>, Vec<u8>)> {
    [
        (0x0100_0d46_u32, 1_u32),
        (0x0100_4404, 6),
        (0x0100_4dd8, 100),
    ]
    .into_iter()
    .map(|(token, degree)| {
        let mut value = (69 * degree).to_le_bytes().to_vec();
        for item in 0x0100_0001..0x0100_0001 + degree {
            value.extend(item.to_le_bytes());
            value.extend(69_u32.to_le_bytes());
        }
        (token.to_le_bytes().to_vec(), value)
    })
    .collect()
}

/// `issue1114_collision_saves`' save loop on `ctx`: import one phrase before
/// each save, check the rows (and, reloading, the masked bytes against
/// `original`), reopen after each save.
fn collision_saves(k: &Kit, mut ctx: P, rows: &[(Vec<u8>, Vec<u8>)], original: Option<&[u8]>) -> J {
    let path = k.user.join("user_bigram.db");
    let mut expected: Vec<(String, String)> = rows
        .iter()
        .map(|(key, value)| (hex_bytes(key), hex_bytes(value)))
        .collect();
    expected.sort();
    let mut saved = Vec::new();
    for (text, pinyin, zhuyin) in [
        ("达达", "da2'da2", "ㄉㄚˊ ㄉㄚˊ"),
        ("你你", "ni3'ni3", "ㄋㄧˇ ㄋㄧˇ"),
    ] {
        assert!(import(ctx, 7, text, reading(pinyin, zhuyin), 100));
        assert!(facade::save(ctx));
        let raw = std::fs::read(&path).expect("the saved bigram");
        if let Some(original) = original {
            assert_eq!(
                masked_bigram_bytes(&raw),
                masked_bigram_bytes(original),
                "cross-reload changed saved bytes"
            );
        }
        let actual = index_file_rows(&path, true);
        assert_eq!(actual, expected, "cross-reload changed a key or value");
        let digest = {
            use sha2::Digest as _;
            hex_bytes(&sha2::Sha256::digest(masked_bigram_bytes(&raw)))
        };
        saved.push(obj! { "bytes" => raw.len(), "sha256" => digest, "rows" => actual });
        facade::fini(ctx);
        ctx = k.init_default();
        assert!(!ctx.is_null());
    }
    facade::fini(ctx);
    obj! { "saved_bigram" => saved }
}

/// `issue1114_collision_saves`: the three-row profile saved and reopened twice
/// in this side's handoff directory, as the old verify ran it. Its saved user
/// bigram must equal the pin's byte for byte, timestamp masked; then the pin's
/// profile is reloaded as it is, and that reload, with its GLib records alone,
/// is `cross_reload` (the old verify's fresh reload worker on the pin's profile).
fn issue1114_collision_saves(k: &mut Kit, profile: &Raw) -> J {
    let rows = collision_rows();
    let own = k.scratch.join("handoff-subject-X");
    std::fs::create_dir(&own).expect("the handoff profile");
    k.user = own.clone();
    let ctx = crafted_bigram_context(k, &rows);
    let mut out = collision_saves(k, ctx, &rows, None);
    let donor = pin_donor(k, profile);
    let saved = |dir: &std::path::Path| {
        tkrzw_saved_bigram_bytes(
            &std::fs::read(dir.join("user_bigram.db")).expect("a saved bigram"),
        )
    };
    assert!(
        saved(&donor) == saved(&own),
        "saved bytes differ outside META_OFFSET_TIMESTAMP"
    );
    k.user = donor;
    let seen = k.logs().len();
    let ctx = k.init_default();
    assert!(!ctx.is_null());
    let original = std::fs::read(k.user.join("user_bigram.db")).expect("the donor bigram");
    let mut cross = collision_saves(k, ctx, &rows, Some(&original));
    let logs: Vec<J> = k.logs().into_iter().skip(seen).map(J::from).collect();
    cross.set("logs", logs);
    out.set("cross_reload", cross);
    out
}

/// `sentence_out` / `zhuyin_sentence_out`: the first sentence's answer and text.
fn sentence(inst: P) -> J {
    let mut out = seeded();
    let ret = if facade::ZHUYIN {
        facade::get_sentence_only(inst, &raw mut out)
    } else {
        facade::get_sentence_at(inst, 0, &raw mut out)
    };
    J::from((ret, owned_out(out)))
}

/// `lazy_user_bigram`'s `reads`: parse and guess 癫痫's input on the cached
/// instance; its tokens, the sentence and (pinyin) 我's predictions.
fn reads(k: &mut Kit) -> J {
    let text: &[u8] = if facade::ZHUYIN {
        b"2u04vu04"
    } else {
        b"dian4xian4"
    };
    let inst = k.inst();
    let parsed = if facade::ZHUYIN {
        facade::parse_more_chewings(inst, c(text).as_ptr())
    } else {
        facade::parse_more_full_pinyins(inst, c(text).as_ptr())
    };
    assert_eq!(parsed, text.len());
    let guessed = facade::guess_sentence(inst);
    assert!(guessed);
    let mut out = obj! {
        "sentence" => sentence(inst),
        "tokens" => tokens_of(k, "癫痫"),
        "guessed" => guessed,
    };
    if !facade::ZHUYIN {
        let (rows, count) = predicted_rows(inst, "我", 12);
        out.set("predictions", (rows, count));
    }
    out
}

/// `lazy_user_bigram`'s `empty_export`: (pinyin) a bigram export iterator that
/// has nothing to hand out.
fn empty_export(k: &mut Kit) -> P {
    if facade::ZHUYIN {
        return std::ptr::null_mut();
    }
    let it = facade::begin_get_bigram_phrases(k.ctx());
    assert!(!it.is_null());
    assert!(!facade::bigram_iterator_has_next_phrase(it));
    it
}

/// `lazy_user_bigram`'s `finish_export`.
fn finish_export(it: P) {
    if !it.is_null() {
        assert!(!facade::bigram_iterator_has_next_phrase(it));
        facade::end_get_bigram_phrases(it);
    }
}

/// `token_unigram_user(k, add_frequency=False)`: imports 你好你好 into library 7
/// with count nine and reads its unigram twice.
fn token_unigram_user(k: &mut Kit) -> J {
    let phrase = "你好你好";
    let ctx = k.ctx();
    let added = import(
        ctx,
        7,
        phrase,
        reading("ni3'hao3'ni3'hao3", "ㄋㄧˇ ㄏㄠˇ ㄋㄧˇ ㄏㄠˇ"),
        9,
    );
    assert!(added);
    let tokens: Vec<u32> = tokens_of(k, phrase)
        .into_iter()
        .filter(|token| token >> 24 == 7)
        .collect();
    assert_eq!(tokens.len(), 1, "{tokens:?}");
    let token = tokens[0];
    let before = unigram_of(k, token);
    obj! {
        "imported" => added,
        "token" => format!("{token:#x}"),
        "before" => before,
        "after" => unigram_of(k, token),
    }
}

/// `token_unigram_trained`'s document.
fn trained_j(trained: Trained) -> J {
    let training = trained.training;
    obj! {
        "token" => trained.token,
        "parsed" => trained.parsed,
        "before" => trained.before,
        "training" => obj! {
            "chosen" => training.chosen,
            "kind" => training.kind,
            "cursor" => training.cursor,
            "after-choose" => training.after_choose,
            "guess" => training.guess,
            "train" => training.train,
        },
        "after" => trained.after,
    }
}

/// `lazy_user_bigram`: a missing, then known-empty user bigram, then one
/// constrained training write, each save reopened. The saves are kept in the
/// snapshot directory and must equal the pin's (its handoff profile), timestamp
/// masked, before `saved_byte_comparisons` is recorded, as the old verify did.
fn lazy_user_bigram(k: &mut Kit, profile: &Raw) -> J {
    let snapshots = k.scratch.join("handoff-subject-X");
    std::fs::create_dir(&snapshots).expect("the snapshot directory");
    let path = k.user.join("user_bigram.db");
    assert!(!path.exists());
    let mut out = obj! { "fresh" => reads(k) };
    let it = empty_export(k);
    finish_export(it);
    out.set("mask", facade::mask_out(k.ctx(), 0xffff_ffff, 0xff12_3456));
    out.set("import", token_unigram_user(k));
    assert!(facade::save(k.ctx()));
    let raw = std::fs::read(&path).expect("the empty bigram");
    assert!(index_file_rows(&path, true).is_empty());
    std::fs::write(snapshots.join("empty.db"), &raw).expect("empty.db");
    out.set("empty_bytes", raw.len());
    reopen(k);
    out.set("reopened_empty", reads(k));
    let it = empty_export(k);
    assert!(facade::reset(k.inst()));
    out.set("training", trained_j(token_unigram_trained(k)));
    finish_export(it);
    assert!(facade::save(k.ctx()));
    let rows = index_file_rows(&path, true);
    assert!(!rows.is_empty(), "one training must write a user bigram");
    std::fs::write(
        snapshots.join("trained.db"),
        std::fs::read(&path).expect("the trained bigram"),
    )
    .expect("trained.db");
    out.set("trained_rows", rows.clone());
    reopen(k);
    out.set("reopened_trained", reads(k));
    assert_eq!(index_file_rows(&path, true), rows);
    let donor = pin_donor(k, profile);
    for name in ["empty.db", "trained.db"] {
        let saved = |dir: &std::path::Path| {
            tkrzw_saved_bigram_bytes(&std::fs::read(dir.join(name)).expect("a saved snapshot"))
        };
        assert!(
            saved(&donor) == saved(&snapshots),
            "{name} differs outside META_OFFSET_TIMESTAMP"
        );
    }
    out.set("saved_byte_comparisons", vec!["empty.db", "trained.db"]);
    out
}

/// `lookup_tokens` of `text`, required to succeed.
fn lookup_ok(inst: P, text: &str) -> Vec<u32> {
    // SAFETY: a fresh GArray of four-byte elements, freed below.
    let array = unsafe { glib_sys::g_array_new(0, 0, 4) };
    assert!(facade::lookup_tokens(inst, c(text).as_ptr(), array.cast()));
    let tokens = crate::readers::garray_u32(array);
    // SAFETY: the array was allocated above and is freed once.
    unsafe { glib_sys::g_array_free(array, 1) };
    tokens
}

/// `predicted_bigram_row_context`: a context whose user bigram lists
/// `successor` after `phrase` with `count`; the context is not cached.
fn predicted_bigram_row_context(k: &Kit, phrase: &str, successor: &str, count: u32) -> P {
    let ctx = k.init_default();
    assert!(!ctx.is_null(), "the first init failed");
    let inst = facade::alloc_instance(ctx);
    let found: Vec<Vec<u32>> = [phrase, successor]
        .iter()
        .map(|text| lookup_ok(inst, text))
        .collect();
    facade::free_instance(inst);
    facade::fini(ctx);
    let value = gram_bytes(count, &[(found[1][0], count)]);
    let rows: Vec<(Vec<u8>, Vec<u8>)> = found[0]
        .iter()
        .map(|token| (token.to_le_bytes().to_vec(), value.clone()))
        .collect();
    craft_hash(&k.user.join("user_bigram.db"), &rows);
    write_user_conf(k);
    let ctx = k.init_default();
    assert!(!ctx.is_null(), "the reopen failed");
    ctx
}

/// `predicted-bigram-is-user-candidate`: a predicted-bigram row whose successor
/// is a user phrase, and its user status.
fn predicted_is_user(k: &mut Kit) -> J {
    let ctx = k.init_default();
    assert!(!ctx.is_null(), "the import context failed");
    assert!(
        import(ctx, 7, "龘龘", "da2'da2", 5),
        "the user successor did not import"
    );
    assert!(facade::save(ctx), "the user successor did not save");
    facade::fini(ctx);
    let ctx = predicted_bigram_row_context(k, "我", "龘龘", 20);
    let inst = facade::alloc_instance(ctx);
    facade::guess_predicted_candidates_with_punctuations(inst, c("我").as_ptr());
    let row = row_of(inst, 4).expect("the planted gram listed no predicted-bigram row");
    let mut kind = 0_i32;
    let typed = facade::get_candidate_type(inst, row, &raw mut kind);
    let mut text: *const c_char = std::ptr::null();
    let rendered = facade::get_candidate_string(inst, row, &raw mut text);
    obj! {
        "type" => (typed, kind),
        "text" => (rendered, borrowed_text(text)),
        "is-user" => facade::is_user_candidate(inst, row),
    }
}

/// `import_after_partial_mask`: an appended phrase imported, a partial-tail
/// mask refused, the phrase imported again.
fn import_after_partial_mask(k: &mut Kit) -> J {
    let ctx = crafted_bigram_context(k, &[(vec![1, 0, 0, 0], vec![0, 0, 0, 0, 1])]);
    let text = "你你你你";
    let reading = reading("ni3'ni3'ni3'ni3", "ㄋㄧˇ ㄋㄧˇ ㄋㄧˇ ㄋㄧˇ");
    let first = import(ctx, 1, text, reading, 5);
    let masked = facade::mask_out(ctx, 0xffff_ffff, 0);
    let second = import(ctx, 1, text, reading, 4);
    facade::fini(ctx);
    obj! { "first" => first, "mask" => masked, "second" => second }
}

/// `abort-bigram-export-short-predecessor`: twelve sub-threshold predecessors
/// and one three-byte value planted, then the export walk.
fn bigram_export_short_predecessor(k: &mut Kit) -> J {
    let texts = [
        "你", "好", "号", "我", "他", "是", "的", "在", "有", "人", "中", "大",
    ];
    let tokens: Vec<u32> = texts.iter().map(|text| tokens_of(k, text)[0]).collect();
    let mut rows: Vec<(u32, Vec<u8>)> = tokens
        .iter()
        .enumerate()
        .map(|(n, &prev)| (prev, gram_bytes(1, &[(tokens[(n + 1) % tokens.len()], 1)])))
        .collect();
    rows.push((0x0200_1234, vec![7, 0, 0]));
    plant_user_grams(k, &rows);
    let it = facade::begin_get_bigram_phrases(k.ctx());
    assert!(!it.is_null(), "begin reads no gram");
    let steps: Vec<bool> = (0..2)
        .map(|_| facade::bigram_iterator_has_next_phrase(it))
        .collect();
    facade::end_get_bigram_phrases(it);
    obj! { "steps" => steps }
}

/// `_plant_user_grams(k, [], grams)`: an empty import, the system tokens read,
/// a save and close, then `user_bigram.db` replaced with `rows`.
fn plant_user_grams(k: &mut Kit, rows: &[(u32, Vec<u8>)]) {
    let it = facade::begin_add_phrases(k.ctx(), 7);
    facade::end_add_phrases(it);
    for text in ["你", "好", "号", "我"] {
        tokens_of(k, text);
    }
    assert!(facade::save(k.ctx()));
    k.close();
    let rows: Vec<(Vec<u8>, Vec<u8>)> = rows
        .iter()
        .map(|(token, value)| (token.to_le_bytes().to_vec(), value.clone()))
        .collect();
    craft_hash(&k.user.join("user_bigram.db"), &rows);
    write_user_conf(k);
}

/// `d18_bigram_db(k, path, read_keys=[key], allow_missing=True)[0]`: the raw
/// value of `key` in the saved user bigram, `None` when absent.
fn bigram_value(path: &Path, key: &[u8]) -> Option<Vec<u8>> {
    use oxpinyin_store::RawReadStore as _;
    let store = oxpinyin_store::DefaultStore::open_user_bigram(path).expect("the saved bigram");
    let mut value = Vec::new();
    store
        .get_raw(key, &mut value)
        .expect("read the key")
        .then_some(value)
}

/// `issue677_sequence`: three imports across five saves over a seeded
/// library-seven predecessor, user tokens masked after the first save.
fn issue677_sequence(k: &mut Kit) -> J {
    let key = 0x0700_0001_u32.to_le_bytes();
    let ctx = crafted_bigram_context(k, &[(key.to_vec(), gram_bytes(100, &[(0x0100_0001, 100)]))]);
    let add = |text: &str, pinyin: &str, zhuyin: &str| {
        assert!(import(ctx, 7, text, reading(pinyin, zhuyin), 100));
    };
    add("达达", "da2'da2", "ㄉㄚˊ ㄉㄚˊ");
    let ret1 = facade::save(ctx);
    let mask = facade::mask_out(ctx, 0x0f00_0000, 0x0700_0000);
    let ret2 = facade::save(ctx);
    let path = k.user.join("user_bigram.db");
    let after_mask = bigram_value(&path, &key);
    add("你你", "ni3'ni3", "ㄋㄧˇ ㄋㄧˇ");
    let ret3 = facade::save(ctx);
    let ret4 = facade::save(ctx);
    add("好好", "hao3'hao3", "ㄏㄠˇ ㄏㄠˇ");
    let ret5 = facade::save(ctx);
    let mut files: Vec<String> = std::fs::read_dir(&k.user)
        .expect("user directory")
        .map(|entry| crate::support::os_text(&entry.expect("entry").file_name()))
        .collect();
    files.sort();
    let final_value = bigram_value(&path, &key);
    obj! {
        "ret1" => ret1, "mask" => mask, "ret2" => ret2, "ret3" => ret3,
        "ret4" => ret4, "ret5" => ret5,
        "files" => files,
        "removed_key_after_mask" => after_mask.map(|value| hex_bytes(&value)),
        "removed_key_final" => final_value.map(|value| hex_bytes(&value)),
    }
}
