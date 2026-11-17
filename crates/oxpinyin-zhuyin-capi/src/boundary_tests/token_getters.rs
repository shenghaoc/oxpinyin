//! The token getters: phrases, readings, pronunciation counts and caller
//! arrays.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use super::*;

fn n_pronunciation(instance: *mut ZhuyinInstance, token: u32) -> (bool, u32) {
    let mut count = UNTOUCHED;
    let ret = unsafe { zhuyin_token_get_n_pronunciation(instance, token, &raw mut count) };
    (ret, count)
}

/// At 074a2219 the USER_FILE load installs the whole offset and content
/// sections (phrase_index.cpp:335-362). A bad offset fails only the item
/// read (phrase_index.cpp:180-205); neighbouring items remain readable.
#[test]
fn a_bad_user_item_offset_leaves_neighbouring_items_readable() {
    use oxpinyin_data::chunk_format::build_memory_chunk;
    use oxpinyin_data::chunk_write::{ChunkItem, build_chunk};

    let user = TempUserDir::new("user-file-bad-item");
    write_user_conf(&user.path, &system_dir());
    let rows = [(1, "你", 0x302b_u16), (2, "好", 0x30a7), (3, "吗", 0x5083)];
    let items = rows.map(|(slot, phrase, key)| {
        (
            slot,
            ChunkItem {
                phrase: phrase.chars().map(u32::from).collect(),
                unigram: 17,
                prons: vec![(vec![key], 5)],
            },
        )
    });
    let chunk = build_chunk(&items).expect("three-item user chunk");
    let mut payload = chunk[8..].to_vec();
    let offsets_start = u32::from_le_bytes(payload[4..8].try_into().expect("offset header"));
    let slot_two = offsets_start as usize + 2 * 4;
    payload[slot_two..slot_two + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    std::fs::write(
        user.path.join("user.bin"),
        build_memory_chunk(&payload).expect("frame"),
    )
    .expect("write user.bin");

    let (context, instance) = open_with_user(&user.path);
    for (slot, expected, key) in [rows[0], rows[2]] {
        let token = 0x0700_0000 | slot;
        assert_eq!(
            token_phrase(instance, token),
            (true, 1, Some(expected.to_owned()))
        );
        assert_eq!(n_pronunciation(instance, token), (true, 1));
        let [low, high] = key.to_le_bytes();
        assert_eq!(
            nth_pronunciation(instance, token, 0),
            (true, 1, format!("{low:02x}{high:02x}"))
        );
    }
    assert_eq!(
        token_phrase(instance, 0x0700_0002),
        (false, UNTOUCHED, None)
    );
    assert_eq!(n_pronunciation(instance, 0x0700_0002), (false, 0));
    assert_eq!(
        nth_pronunciation(instance, 0x0700_0002, 0),
        (false, 0, String::new())
    );
    close(context, instance);
}

/// Unloading the user library hides its token from the token getters, and
/// the load shows it again.
#[test]
fn an_unloaded_user_library_hides_its_token_until_the_load() {
    let user_dir = TempUserDir::new("token-unload");
    let (context, instance) = open_with_user(&user_dir.path);
    assert!(import(context, 7, "你好", "ㄋㄧˇ ㄏㄠˇ".as_bytes(), 1));
    let tokens = user_tokens(instance, "你好");
    assert_eq!(tokens.len(), 1);
    let token = tokens[0];
    let shown = |instance| {
        assert_eq!(n_pronunciation(instance, token), (true, 1));
        assert_eq!(
            nth_pronunciation(instance, token, 0),
            (true, 2, "2b308632".to_owned())
        );
        assert_eq!(
            token_phrase(instance, token),
            (true, 2, Some("你好".to_owned()))
        );
    };
    shown(instance);
    assert!(unsafe { zhuyin_save(context) });
    assert!(unsafe { zhuyin_unload_phrase_library(context, 7) });
    assert_eq!(n_pronunciation(instance, token), (false, 0));
    assert_eq!(
        nth_pronunciation(instance, token, 0),
        (false, 0, String::new())
    );
    assert_eq!(token_phrase(instance, token), (false, UNTOUCHED, None));
    assert!(unsafe { zhuyin_load_phrase_library(context, 7) });
    shown(instance);
    close(context, instance);
}
