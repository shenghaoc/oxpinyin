//! The phrase-library reader over the chunk files of the pinned libpinyin-data
//! package (`just fetch-contract-data`). The package's other files are in tkrzw
//! format, so this target requires that backend.

use oxpinyin_data::phrase_library::PhraseLibrary;

#[path = "../../oxpinyin-runtime/tests/common/contract_data.rs"]
mod contract_data;

#[test]
fn reads_the_pinned_package_libraries() {
    let dir = contract_data::contract_data_dir();

    let mut libraries = Vec::new();
    let mut items = 0_u64;
    for name in ["gb_char", "gbk_char", "opengram", "merged"] {
        let library = PhraseLibrary::open(&dir.join(format!("{name}.bin")))
            .unwrap_or_else(|e| panic!("{name}.bin: {e}"));
        assert!(library.total_freq() > 0, "{name} carries unigram mass");
        assert_eq!(library.token_range().start, 1, "{name} tokens start at 1");
        let count = library.items().count() as u64;
        assert!(count > 0, "{name} has items");
        items += count;
        libraries.push(library);
    }
    // The four system libraries of the model libpinyin 074a2219 builds: 138,096
    // tokens (`docs/findings/datagen-model20.md`).
    assert_eq!(items, 138_096);

    // 的: token 0x010005DB. The pinned model's `gb_char.table` carries two
    // readings, `de 的 16778715 2213855` and `di 的 16778715 11000`, the counts
    // the chunk stores verbatim (`datagen-model20.md`).
    let gb = &libraries[0];
    let item = gb.item(0x0100_05DB).expect("的 present in gb_char");
    assert_eq!(
        item.phrase_text().as_ref().and_then(|text| text.as_str()),
        Some("的")
    );
    let mut freqs: Vec<u32> = item
        .pronunciations()
        .map(|pronunciation| pronunciation.freq)
        .collect();
    freqs.sort_unstable();
    assert_eq!(freqs, [11_000, 2_213_855]);

    // 锕: token 0x01000001, one reading, `a 锕 … 7` in the same table.
    let item = gb.item(0x0100_0001).expect("锕 present in gb_char");
    assert_eq!(
        item.phrase_text().as_ref().and_then(|text| text.as_str()),
        Some("锕")
    );
    assert_eq!(item.n_pronunciations(), 1);
    assert_eq!(
        item.pronunciations()
            .next()
            .map(|pronunciation| pronunciation.freq),
        Some(7)
    );

    // The addon chunk files ride the same format.
    let art = PhraseLibrary::open(&dir.join("art.bin")).expect("addon art.bin");
    assert!(art.items().count() > 0);
}
