//! The k-mixture model's record types on member 4's flexible-bigram layer
//! (`utils/training/k_mixture_model.h:115-166` at `074a2219`).
//!
//! Every field is the pin's native `guint32`; a record is the pin's
//! `sizeof` bytes, `memcpy`d without padding, a length prefix or a
//! normalised byte order. On the linux/amd64 reference that is
//! little-endian and the magic header, array header and array item are 12,
//! 8 and 16 bytes.
//!
//! | Record | Pin type | Bytes |
//! |---|---|---|
//! | `KMixtureModelMagicHeader` | `m_WC`, `m_N`, `m_total_freq` | 12 |
//! | `KMixtureModelArrayHeader` | `m_WC`, `m_freq` | 8 |
//! | `KMixtureModelArrayItem` | `m_WC`, `m_N_n_0`, `m_n_1`, `m_Mr` | 16 |
//!
//! [`KmmBigram`] is the pin's `KMixtureModelBigram`
//! (`flexible_ngram<MagicHeader, ArrayHeader, ArrayItem>`), and
//! [`KmmGram`] its `KMixtureModelSingleGram`. The `KMMP` magic number is
//! the pin's `K_MIXTURE_MODEL_MAGIC_NUMBER` (`k_mixture_model.h:115`).

use oxpinyin_store::{BackendError, DefaultFlexibleBigram, SpecGram, StoreError};

pub use oxpinyin_store::{ArrayItemWithToken, FixedRecord, FlexibleGramSpec, SingleGram};

/// `KMixtureModelMagicHeader` (`k_mixture_model.h:117-124`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KMixtureModelMagicHeader {
    /// `m_WC`: the total number of instances of all words.
    pub m_wc: u32,
    /// `m_N`: the total number of documents.
    pub m_n: u32,
    /// `m_total_freq`: the total freq of the unigram table.
    pub m_total_freq: u32,
}

/// `KMixtureModelArrayHeader` (`k_mixture_model.h:126-131`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KMixtureModelArrayHeader {
    /// `m_WC`: the total number of instances of word W1.
    pub m_wc: u32,
    /// `m_freq`: the unigram frequency of W1.
    pub m_freq: u32,
}

/// `KMixtureModelArrayItem` (`k_mixture_model.h:133-154`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KMixtureModelArrayItem {
    /// `m_WC`: the total number of all (W1, W2) instances (`m_T` is an
    /// alias of this field in the pin).
    pub m_wc: u32,
    /// `m_N_n_0`: the number of documents containing the pair, so
    /// `m_n_0 = magic.m_N − m_N_n_0`.
    pub m_n_n_0: u32,
    /// `m_n_1`: the number of documents with exactly one occurrence.
    pub m_n_1: u32,
    /// `m_Mr`: the maximum instances in one document seen so far.
    pub m_mr: u32,
}

fn record_error(what: &str, bytes: usize, expected: usize) -> StoreError {
    StoreError::Backend(BackendError::from(format!(
        "{what} is {bytes} bytes, not the pin's {expected}"
    )))
}

fn words<const N: usize>(bytes: &[u8]) -> [u32; N] {
    let mut out = [0_u32; N];
    for (index, word) in out.iter_mut().enumerate() {
        let at = index * 4;
        let mut field = [0_u8; 4];
        field.copy_from_slice(&bytes[at..at + 4]);
        *word = u32::from_ne_bytes(field);
    }
    out
}

impl FixedRecord for KMixtureModelMagicHeader {
    const LEN: usize = 12;

    fn zeroed() -> Self {
        Self::default()
    }

    fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(Self::LEN);
        for word in [self.m_wc, self.m_n, self.m_total_freq] {
            bytes.extend_from_slice(&word.to_ne_bytes());
        }
        bytes
    }

    fn decode(bytes: &[u8]) -> Result<Self, StoreError> {
        if bytes.len() != Self::LEN {
            return Err(record_error("k mixture magic header", bytes.len(), Self::LEN));
        }
        let [m_wc, m_n, m_total_freq] = words::<3>(bytes);
        Ok(Self {
            m_wc,
            m_n,
            m_total_freq,
        })
    }
}

impl FixedRecord for KMixtureModelArrayHeader {
    const LEN: usize = 8;

    fn zeroed() -> Self {
        Self::default()
    }

    fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(Self::LEN);
        for word in [self.m_wc, self.m_freq] {
            bytes.extend_from_slice(&word.to_ne_bytes());
        }
        bytes
    }

    fn decode(bytes: &[u8]) -> Result<Self, StoreError> {
        if bytes.len() != Self::LEN {
            return Err(record_error("k mixture array header", bytes.len(), Self::LEN));
        }
        let [m_wc, m_freq] = words::<2>(bytes);
        Ok(Self { m_wc, m_freq })
    }
}

impl FixedRecord for KMixtureModelArrayItem {
    const LEN: usize = 16;

    fn zeroed() -> Self {
        Self::default()
    }

    fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(Self::LEN);
        for word in [self.m_wc, self.m_n_n_0, self.m_n_1, self.m_mr] {
            bytes.extend_from_slice(&word.to_ne_bytes());
        }
        bytes
    }

    fn decode(bytes: &[u8]) -> Result<Self, StoreError> {
        if bytes.len() != Self::LEN {
            return Err(record_error("k mixture array item", bytes.len(), Self::LEN));
        }
        let [m_wc, m_n_n_0, m_n_1, m_mr] = words::<4>(bytes);
        Ok(Self {
            m_wc,
            m_n_n_0,
            m_n_1,
            m_mr,
        })
    }
}

/// The pin's `KMixtureModelBigram`
/// (`flexible_ngram<KMixtureModelMagicHeader, KMixtureModelArrayHeader,
/// KMixtureModelArrayItem>`, `k_mixture_model.h:156-159`) and its `KMMP`
/// magic number.
#[derive(Clone, Copy, Debug)]
pub struct KmmSpec;

impl FlexibleGramSpec for KmmSpec {
    const MAGIC_NUMBER: [u8; 4] = *b"KMMP";
    type MagicHeader = KMixtureModelMagicHeader;
    type ArrayHeader = KMixtureModelArrayHeader;
    type ArrayItem = KMixtureModelArrayItem;
}

/// The working model on the backend this build selected.
pub type KmmBigram = DefaultFlexibleBigram<KmmSpec>;

/// The pin's `KMixtureModelSingleGram`.
pub type KmmGram = SpecGram<KmmSpec>;

#[cfg(test)]
mod tests {
    use super::{
        KMixtureModelArrayHeader, KMixtureModelArrayItem, KMixtureModelMagicHeader, KmmSpec,
    };
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

        let header = KMixtureModelArrayHeader {
            m_wc: 7,
            m_freq: 8,
        };
        assert_eq!(header.encode().len(), 8);
        assert_eq!(KMixtureModelArrayHeader::decode(&header.encode()).unwrap(), header);

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
}
