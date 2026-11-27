//! Native unreadable candidate fixtures shared by the runtime and C tests.
use oxpinyin::__internal::data::row_format::pinyin_index::{encode_complete_key, encode_item};
use oxpinyin::__internal::store::{DefaultStore, WriteStore};
use oxpinyin::facade::ChewingKey;
use std::{fs, path::Path};

#[derive(Clone, Copy)]
#[allow(dead_code)] // Each target uses its own cases from this shared fixture.
pub(crate) enum Case {
    Single,
    Pair,
    Mixed,
    Fallback,
    MaskedExtension,
    #[cfg(unix)]
    Zero,
}

pub(crate) fn dirs(case: Case) -> (tempfile::TempDir, tempfile::TempDir) {
    let user = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    let total = u32::from(!matches!(case, Case::Zero));
    #[cfg(not(unix))]
    let total = 1_u32;
    let dir = tempfile::tempdir().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(if env!("CARGO_PKG_NAME") == "oxpinyin" {
            "fixtures/w3"
        } else {
            "../fixtures/w3"
        })
        .join(oxpinyin::__internal::DEFAULT_STORE_EXT);
    for name in [
        "table.conf",
        "phrase_index.bin",
        "bigram.db",
        "punct.bin",
        "addon_pinyin_index.bin",
        "addon_phrase_index.bin",
    ] {
        fs::copy(source.join(name), dir.path().join(name)).unwrap();
    }

    let key = ChewingKey::from_pinyin("ni").unwrap();
    // Keep the library loaded independently of the indexed slots.
    for (name, total) in [
        ("gb_char.bin", total),
        ("gbk_char.bin", 0),
        ("merged.bin", 0),
        ("opengram.bin", 0),
    ] {
        let mut chunk = super::chunk_fixture::ChunkBuilder::new(total);
        if name == "gb_char.bin" && matches!(case, Case::Mixed) {
            chunk.add(
                0x11,
                1,
                "你",
                vec![(key.with_tone(3).to_packed().to_le_bytes().to_vec(), 1)],
            );
        }
        fs::write(dir.path().join(name), chunk.build()).unwrap();
    }

    let index_path = if matches!(case, Case::Fallback) {
        let conf = oxpinyin::facade::TableConf::load_from_dir(dir.path()).unwrap();
        let versions = oxpinyin::__internal::data::user_files::SystemVersions::from_conf(&conf);
        assert!(
            oxpinyin::__internal::data::user_files::UserTableInfo::conform_to(&versions)
                .save(user.path().join("user.conf").as_path())
        );
        user.path().join("user_pinyin_index.bin")
    } else {
        dir.path().join("pinyin_index.bin")
    };
    let token = if matches!(case, Case::MaskedExtension) {
        0x0200_0001
    } else {
        0x0100_0010
    };
    let mut value = encode_item(token, &[key.with_tone(3)]);
    if matches!(case, Case::Pair | Case::Mixed) {
        value.extend(encode_item(0x0100_0011, &[key.with_tone(3)]));
    }
    let extension = encode_complete_key(&[key, ChewingKey::from_pinyin("hao").unwrap()]);
    let key = encode_complete_key(&[key]);
    if matches!(case, Case::Fallback) {
        DefaultStore::write_user_index(&index_path, |sink| sink(&key, &value)).unwrap();
    } else {
        let index = DefaultStore::create(&index_path).unwrap();
        index.set_raw(&key, &value).unwrap();
        if matches!(case, Case::MaskedExtension) {
            // Too short for one record: only a decode of this
            // row fails.
            index.set_raw(&extension, &[1, 2, 3]).unwrap();
        }
    }
    (dir, user)
}
