//! Candidate guesses, predictions, prefix and punctuation rows and the
//! zero facade total.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use super::*;

/// The prediction walks the user phrase table by raw key bytes and takes
/// the row's text from the live item: a key 你 + U+110000 lists 龘龘's
/// suffix 龘.
#[test]
fn a_raw_user_key_predicts_its_live_item() {
    let user = TempUserDir::new("raw-key-prediction");
    let token = saved_user_phrase(&user.path);
    craft_user_index(
        &user.path.join("user_phrase_index.bin"),
        &[
            (ucs4("你"), Vec::new()),
            (
                [ucs4("你"), 0x0011_0000_u32.to_le_bytes().to_vec()].concat(),
                token.to_le_bytes().to_vec(),
            ),
        ],
    );
    write_user_conf(&user.path, &system_dir());
    let (context, instance) = open_at(&user.path);
    assert!(unsafe { pinyin_guess_predicted_candidates(instance, c"你".as_ptr()) });
    assert!(
        capture_rows(instance)
            .iter()
            .any(|(kind, text, _)| *kind == PREDICTED_PREFIX && text == "龘")
    );
    close(context, instance);
}
