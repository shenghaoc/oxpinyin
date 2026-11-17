//! W11 union surface: user/addon candidates and phrase prediction.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::context::pinyin_init;

use crate::instance::pinyin_alloc_instance;

use crate::iterators::{
    pinyin_begin_add_phrases, pinyin_end_add_phrases, pinyin_iterator_add_phrase,
};

use crate::test_support::{TempSystemDir, TempUserDir, cstr};

/// A default row typed `USER_FILE` is loaded from the user directory into
/// its own sub-index, whatever its index, and a token is dispatched by its
/// library nibble alone (074a2219 `pinyin.cpp:303-321,377-392`,
/// `phrase_index.h:646-652`). With `GB_DICTIONARY` retyped, slot 1 of
/// `gb_user.bin` is read at token 0x01000001 (`pinyin.cpp:2821-2834`), and
/// an import into library 1 takes the next slot (`pinyin.cpp:588-608`).
#[test]
fn a_user_file_row_at_a_system_index_is_read_and_imported_into() {
    use oxpinyin_data::chunk_write::{ChunkItem, build_chunk};

    let system = TempSystemDir::new("gb-user-file");
    let conf = std::fs::read_to_string(system.path.join("table.conf")).expect("table.conf");
    let row = "default GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE";
    assert!(conf.contains(row));
    system.write(
        "table.conf",
        &conf.replace(row, "default GB_DICTIONARY NULL NULL gb_user.bin USER_FILE"),
    );
    let user = TempUserDir::new("gb-user-file-user");
    crate::test_support::write_user_conf(&user.path, &system.path);
    let item = ChunkItem {
        phrase: vec![u32::from('你')],
        unigram: 17,
        prons: vec![(vec![0x302b], 5)],
    };
    std::fs::write(
        user.path.join("gb_user.bin"),
        build_chunk(&[(1, item)]).expect("one-item chunk"),
    )
    .expect("write gb_user.bin");
    let context = unsafe {
        pinyin_init(
            cstr(system.path.to_str().expect("UTF-8 path")).as_ptr(),
            cstr(user.path.to_str().expect("UTF-8 path")).as_ptr(),
        )
    };
    assert!(!context.is_null());
    let instance = unsafe { pinyin_alloc_instance(context) };
    assert!(!instance.is_null());
    let unigram = |token: u32| {
        let mut freq = 0;
        let found = unsafe {
            crate::dict::pinyin_token_get_unigram_frequency(instance, token, &raw mut freq)
        };
        (found, freq)
    };
    assert_eq!(unigram(0x0100_0001), (true, 17));
    let mut length = 0;
    let mut text = std::ptr::null_mut();
    assert!(unsafe {
        crate::dict::pinyin_token_get_phrase(instance, 0x0100_0001, &raw mut length, &raw mut text)
    });
    assert_eq!(length, 1);
    assert!(!text.is_null());
    // SAFETY: the getter returned a NUL-terminated GLib string that we own.
    let rendered = unsafe { std::ffi::CStr::from_ptr(text) }
        .to_bytes()
        .to_vec();
    // SAFETY: freed once with the matching allocator.
    unsafe { glib_sys::g_free(text.cast()) };
    assert_eq!(rendered, "你".as_bytes());
    assert!(!unigram(0x0100_0002).0);
    let iter = unsafe { pinyin_begin_add_phrases(context, 1) };
    assert!(unsafe {
        pinyin_iterator_add_phrase(iter, cstr("测测").as_ptr(), cstr("ce4'ce4").as_ptr(), 5)
    });
    unsafe { pinyin_end_add_phrases(iter) };
    assert!(unigram(0x0100_0002).0);
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };
}

/// The packed key of `spelling` at `tone`, as an addon item stores it.
fn packed_key(spelling: &str, tone: u8) -> u16 {
    oxpinyin_core::ChewingKey::from_pinyin(spelling)
        .expect("frozen syllable")
        .with_tone(tone)
        .to_packed()
}

/// A private system whose addon library 4 holds 二簧 with two toned
/// readings, an item whose first word is the surrogate 0xD800, and 三次 indexed under `san ci` with no reading in its body.
pub(crate) fn promotion_system(parent: &std::path::Path) -> std::path::PathBuf {
    use crate::test_support::{AddonItem, addon_system, system_dir};
    let readings: [(&[u16], u32); 2] = [
        (&[packed_key("er", 2), packed_key("huang", 2)], 7),
        (&[packed_key("er", 4), packed_key("huang", 1)], 3),
    ];
    let system = addon_system(
        &system_dir(),
        parent,
        &[
            AddonItem {
                text: &[0x4e8c, 0x7c27],
                readings: &readings,
                indexed_as: &[packed_key("er", 0), packed_key("huang", 0)],
                unigram: 11,
            },
            AddonItem {
                text: &[0xD800, 0x4e8c],
                readings: &[(&[packed_key("ba", 1), packed_key("ma", 3)], 5)],
                indexed_as: &[packed_key("ba", 0), packed_key("ma", 0)],
                unigram: 5,
            },
            AddonItem {
                text: &[0x4e09, 0x6b21],
                readings: &[],
                indexed_as: &[packed_key("san", 0), packed_key("ci", 0)],
                unigram: 9,
            },
        ],
    );
    std::fs::write(
        system.join("interpolation2.text"),
        "\\data model interpolation\n\\1-gram\n\\item 1 ok count 1\n",
    )
    .expect("interpolation2.text");
    system
}
