//! Saved profiles and pin-derived native rows through the user-store library loop.
use crate::native_fixture::{TempUserDir, craft_hash, store_rows, write_user_conf};
use crate::test_session::{native_keys, native_unigram, open_native};
use crate::{UserConfLaw, UserStore};
use std::path::Path;
#[path = "../../../../oxpinyin-facade/tests/common/contract_data.rs"]
mod contract_data;
use contract_data::contract_data_dir;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn rows(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}
fn container_rows(path: &Path, hash: bool) -> Vec<(String, String)> {
    let mut rows = super::hex_rows(store_rows(path, hash));
    rows.sort();
    rows
}
fn user_rows(user: &Path) -> [Vec<(String, String)>; 3] {
    [
        container_rows(&user.join("user_bigram.db"), true),
        container_rows(&user.join("user_phrase_index.bin"), false),
        container_rows(&user.join("user_pinyin_index.bin"), false),
    ]
}
fn import(store: &mut UserStore, text: &str, reading: &str, count: u64, law: UserConfLaw) -> u32 {
    let keys = match law {
        UserConfLaw::Pinyin => native_keys(reading),
        UserConfLaw::Zhuyin => {
            oxpinyin_core::parse_zhuyin_direct(reading, oxpinyin_core::OptionBits::USE_TONE)
                .into_iter()
                .map(|(key, tone)| crate::toned_key(key.index(), tone).unwrap())
                .collect()
        }
    };
    let token = store
        .add_phrase_in(7, &text.into(), &keys, Some(count))
        .unwrap();
    store.compact_libraries();
    store.mark_modified();
    token
}
fn write_profile(case: &str, dir: &Path, law: UserConfLaw) {
    match law {
        UserConfLaw::Pinyin => crate::native_fixture::write_pin_profile(case, dir),
        UserConfLaw::Zhuyin => crate::native_fixture::write_zhuyin_pin_profile(case, dir),
    }
}

/// The pin's saved profile (#929) keeps the exact records and tokens after a zero-count import.
fn saved_profile(case: &str, reading: &str, law: UserConfLaw) {
    let user = TempUserDir::new("save-929-cross-open");
    write_profile(case, &user.path, law);
    let expected = [
        rows(&[("01000007", "250000000200000725000000")]),
        rows(&[
            ("969f0000", ""),
            ("969f0000969f0000", "02000007"),
            ("989f0000", ""),
            ("989f0000989f0000", "01000007"),
        ]),
        rows(&[
            ("0400", ""),
            ("04000400", "0100000784208420"),
            ("1200", ""),
            ("12001200", "0200000792409240"),
            ("8400", ""),
            ("84008400", "0100000784208420"),
            ("9200", ""),
            ("92009200", "0200000792409240"),
        ]),
    ];

    assert_eq!(user_rows(&user.path), expected);
    let mut store = open_native(&user.path, &contract_data_dir(), law);
    let tokens =
        ["龘龘", "龖龖", "靐靐"].map(|text| store.phrase_table_tokens_for_text(text).unwrap());
    assert_eq!(tokens, [vec![0x0700_0001], vec![0x0700_0002], vec![]]);
    import(&mut store, "龘龘", reading, 0, law);
    store.save_reporting().unwrap();
    assert_eq!(user_rows(&user.path), expected);
}
#[test]
fn the_pins_saved_profile_opens_with_its_records() {
    saved_profile(
        "save-929-logical-cross-open-pinyin",
        "da2'da2",
        UserConfLaw::Pinyin,
    );
}
#[test]
fn a_pin_written_profile_opens_and_saves_with_its_logical_records() {
    saved_profile(
        "save-929-logical-cross-open-zhuyin",
        "ㄉㄚˊ ㄉㄚˊ",
        UserConfLaw::Zhuyin,
    );
}

/// The three user grams of #1114 whose keys collide in one TinyDBM bucket.
fn collision_rows() -> Vec<(Vec<u8>, Vec<u8>)> {
    [
        (0x0100_0d46_u32, 1_u32),
        (0x0100_4404, 6),
        (0x0100_4dd8, 100),
    ]
    .into_iter()
    .map(|(token, degree)| {
        let items: Vec<(u32, u32)> = (0x0100_0001..0x0100_0001 + degree)
            .map(|item| (item, 69))
            .collect();
        (
            token.to_le_bytes().to_vec(),
            crate::native_fixture::gram_bytes(69 * degree, &items),
        )
    })
    .collect()
}
/// Saved HashDBM bytes with the timestamp (`META_OFFSET_TIMESTAMP`, bytes
/// 48 to 56) zeroed.
fn masked(raw: &[u8]) -> Vec<u8> {
    assert!(raw.len() >= 128, "the HashDBM metadata");
    let mut masked = raw.to_vec();
    masked[48..56].fill(0);
    masked
}

/// Two imports, saves and reloads preserve every colliding row and the pin's bytes outside the timestamp.
fn collision_cycles(
    user: &Path,
    original: Option<&[u8]>,
    law: UserConfLaw,
) -> Vec<(usize, String)> {
    let path = user.join("user_bigram.db");
    let mut expected = super::hex_rows(collision_rows());
    expected.sort();
    let readings = match law {
        UserConfLaw::Pinyin => ["da2'da2", "ni3'ni3"],
        UserConfLaw::Zhuyin => ["ㄉㄚˊ ㄉㄚˊ", "ㄋㄧˇ ㄋㄧˇ"],
    };
    let mut saves = Vec::new();
    let mut store = open_native(user, &contract_data_dir(), law);
    for (text, reading) in ["达达", "你你"].into_iter().zip(readings) {
        import(&mut store, text, reading, 100, law);
        store.save_reporting().unwrap();
        let raw = std::fs::read(&path).unwrap();
        if let Some(original) = original {
            assert_eq!(masked(&raw), masked(original));
        }
        assert_eq!(container_rows(&path, true), expected);
        use sha2::Digest as _;
        saves.push((raw.len(), hex(&sha2::Sha256::digest(masked(&raw)))));
        drop(store);
        store = open_native(user, &contract_data_dir(), law);
    }
    saves
}
fn colliding_profiles(case: &str, law: UserConfLaw) {
    let own = TempUserDir::new("issue1114-own");
    drop(open_native(&own.path, &contract_data_dir(), law));
    craft_hash(&own.path.join("user_bigram.db"), &collision_rows());
    write_user_conf(&own.path, &contract_data_dir());
    let saved = (
        4_199_312,
        "44042431c088b786e90ee01b493aa01661912dda345a8cc5f758a41c682f7c22".to_owned(),
    );
    assert_eq!(
        collision_cycles(&own.path, None, law),
        [saved.clone(), saved.clone()]
    );
    let donor = TempUserDir::new("issue1114-pin");
    write_profile(case, &donor.path, law);
    let bigram = |dir: &Path| std::fs::read(dir.join("user_bigram.db")).unwrap();
    assert!(masked(&bigram(&donor.path)) == masked(&bigram(&own.path)));
    let original = bigram(&donor.path);
    assert_eq!(
        collision_cycles(&donor.path, Some(&original), law),
        [saved.clone(), saved]
    );
}
#[test]
fn colliding_user_grams_survive_load_and_save() {
    colliding_profiles("issue1114-collision-load-save-pinyin", UserConfLaw::Pinyin);
}
#[test]
fn colliding_bigram_keys_save_the_pins_bytes() {
    colliding_profiles("issue1114-collision-load-save-zhuyin", UserConfLaw::Zhuyin);
}

/// `%d` consumes versions 2^32 above the table.conf versions (#920). One more wipes the profile.
#[test]
fn user_conf_versions_past_int_take_the_scanners_int() {
    let session = |user: &Path, extra: i64| {
        let system = contract_data_dir();
        let mut store = open_native(user, &system, UserConfLaw::Pinyin);
        let trained = 0x0100_3e57;
        assert_eq!(
            crate::test_session::native_tokens(&system, "癫痫"),
            [trained]
        );
        store.observe_selection(1, trained).unwrap();
        assert_eq!(native_unigram(&store, trained), Some(550));
        let imported = import(
            &mut store,
            "你好你好",
            "ni3'hao3'ni3'hao3",
            9,
            UserConfLaw::Pinyin,
        );
        assert_eq!(native_unigram(&store, imported), Some(27));
        store.save_reporting().unwrap();
        drop(store);
        let path = user.join("user.conf");
        let conf = std::fs::read_to_string(&path).unwrap();
        let edited: String = conf
            .split_inclusive('\n')
            .map(|line| {
                for key in ["binary format version:", "model data version:"] {
                    if let Some(rest) = line.strip_prefix(key) {
                        let value: i64 = rest.trim().parse().unwrap();
                        return format!("{key}{}\n", value + (1 << 32) + extra);
                    }
                }
                line.to_owned()
            })
            .collect();
        std::fs::write(path, edited).unwrap();
        let store = open_native(user, &system, UserConfLaw::Pinyin);
        [
            native_unigram(&store, trained),
            native_unigram(&store, imported),
        ]
    };
    let conforms = TempUserDir::new("user-conf-920-conforms");
    assert_eq!(session(&conforms.path, 0), [Some(550), Some(27)]);
    let wiped = TempUserDir::new("user-conf-920-nonconforms");
    assert_eq!(session(&wiped.path, 1), [Some(67), None]);
}
