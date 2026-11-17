use super::{KMixtureModelArrayHeader, KMixtureModelArrayItem, KMixtureModelMagicHeader, KmmSpec};
use oxpinyin_store::{FixedRecord, FlexibleGramSpec};

#[test]
fn the_record_sizes_and_magic_number_are_the_pins() {
    assert_eq!(KMixtureModelMagicHeader::LEN, 12);
    assert_eq!(KMixtureModelArrayHeader::LEN, 8);
    assert_eq!(KMixtureModelArrayItem::LEN, 16);
    assert_eq!(KmmSpec::MAGIC_NUMBER, *b"KMMP");
}

#[test]
fn records_encode_native_little_endian_words() {
    let magic = KMixtureModelMagicHeader {
        m_wc: 0x0102_0304,
        m_n: 0x0506_0708,
        m_total_freq: 9,
    };
    let mut expected = Vec::new();
    for word in [0x0102_0304_u32, 0x0506_0708, 9] {
        expected.extend_from_slice(&word.to_ne_bytes());
    }
    assert_eq!(magic.encode(), expected);
    assert_eq!(KMixtureModelMagicHeader::decode(&expected).unwrap(), magic);

    let header = KMixtureModelArrayHeader { m_wc: 7, m_freq: 8 };
    assert_eq!(header.encode().len(), 8);
    assert_eq!(
        KMixtureModelArrayHeader::decode(&header.encode()).unwrap(),
        header
    );

    let item = KMixtureModelArrayItem {
        m_wc: 1,
        m_n_n_0: 2,
        m_n_1: 3,
        m_mr: 4,
    };
    let bytes = item.encode();
    assert_eq!(bytes.len(), 16);
    assert_eq!(&bytes[8..12], &3_u32.to_ne_bytes());
    assert_eq!(KMixtureModelArrayItem::decode(&bytes).unwrap(), item);
}

#[test]
fn a_short_or_long_record_is_refused() {
    assert!(KMixtureModelMagicHeader::decode(&[0; 11]).is_err());
    assert!(KMixtureModelArrayHeader::decode(&[0; 9]).is_err());
    assert!(KMixtureModelArrayItem::decode(&[0; 17]).is_err());
}
