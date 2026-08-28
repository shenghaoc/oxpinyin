//! libpinyin's `SingleGram` chunk — the value stored under each bigram
//! key, byte for byte.
//!
//! Pure byte manipulation with no Kyoto Cabinet involvement, so it is
//! unit-testable without the library and is where the format is stated
//! once.
//!
//! **The chunk is the DBM backend's business only in that a DBM stores
//! it.** libpinyin's `SingleGram` is backend-independent (`ngram.cpp`,
//! shared by the Berkeley DB, Kyoto Cabinet and tkrzw builds alike), so
//! this layout is identical whichever library wrote the file — which is
//! also why a Kyoto-Cabinet-built libpinyin and a Berkeley-DB-built one
//! hold the *same logical content* under the *same key* in *different*
//! physical files.
//!
//! # Layout (`ngram.cpp:31-74`, `:178-196`)
//!
//! ```text
//! offset 0 .. 4   guint32  total_freq          native-endian
//! offset 4 .. N   SingleGramItem[]             (N-4)/8 entries
//!                 struct SingleGramItem {      8 bytes, no padding
//!                     phrase_token_t m_token;  guint32, native-endian
//!                     guint32        m_freq;   native-endian
//!                 }
//! ```
//!
//! Two invariants upstream maintains and this module preserves:
//!
//! * the item array is sorted **ascending by token** — `insert_freq`
//!   places each new item at `lower_bound(begin, end, …,
//!   token_less_than)`;
//! * a fresh gram is a 4-byte chunk whose `total_freq` is 0, and
//!   `get_length` asserts that a zero-item gram has zero total.
//!
//! Both were confirmed over the installed 25.9 MB system `bigram.db` —
//! a **Berkeley DB** file, because that is what this machine's libpinyin
//! was built against: 56,359 records and 1,849,609 successor items, every
//! value `4 + 8n` bytes, every item array strictly ascending, and every
//! `total_freq` equal to the sum of its items' frequencies. Those two
//! counts are also what `oxpinyin-datagen` derives independently from
//! `model20` (`docs/findings/datagen-model20.md`), a second and unrelated
//! confirmation. The chunk being backend-independent is what carries that
//! measurement across to this backend; see
//! `docs/findings/kyotocabinet-backend.md` for what that does and does
//! not establish.
//!
//! # Native-endian, deliberately
//!
//! libpinyin stores `guint32` in memory order, so a file written on a
//! little-endian machine is little-endian. This module uses
//! `u32::from_ne_bytes`/`to_ne_bytes` rather than picking an order,
//! because the goal is to agree with whatever libpinyin wrote on *this*
//! machine — the same file is not portable between endiannesses for
//! libpinyin either.

use crate::StoreError;

/// Bytes of the `total_freq` header.
const HEADER: usize = 4;
/// Bytes of one `SingleGramItem`.
const ITEM: usize = 8;

/// A decoded `SingleGram` chunk.
///
/// Held as the decoded pairs rather than the raw bytes so callers cannot
/// accidentally hand back a chunk that has drifted from its total; every
/// write path goes through [`Self::encode`], which recomputes nothing and
/// simply lays out what the accessors maintain.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SingleGram {
    total: u32,
    items: Vec<(u32, u32)>,
}

/// Rejects a chunk that is not `4 + 8n` bytes.
fn framing_error(len: usize) -> StoreError {
    StoreError::Backend(
        format!(
            "corrupt SingleGram chunk: {len} bytes is not 4 + 8n (a 4-byte total_freq \
             followed by whole 8-byte items)"
        )
        .into(),
    )
}

impl SingleGram {
    /// A fresh gram: no items, zero total — upstream's 4-byte chunk.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Decodes a chunk as libpinyin wrote it.
    ///
    /// # Errors
    ///
    /// [`StoreError::Backend`] when the length is not `4 + 8n`, when the
    /// item array is not strictly ascending by token, or when a zero-item
    /// chunk carries a non-zero total — each of which upstream's own
    /// `get_length` assertion or `insert_freq` ordering rules out, so
    /// seeing one means the file is damaged rather than merely old.
    pub fn decode(bytes: &[u8]) -> Result<Self, StoreError> {
        if bytes.len() < HEADER || !(bytes.len() - HEADER).is_multiple_of(ITEM) {
            return Err(framing_error(bytes.len()));
        }
        let total = u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let count = (bytes.len() - HEADER) / ITEM;
        let mut items = Vec::with_capacity(count);
        let mut previous: Option<u32> = None;
        for index in 0..count {
            let at = HEADER + index * ITEM;
            let token =
                u32::from_ne_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
            let freq =
                u32::from_ne_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]);
            if previous.is_some_and(|last| token <= last) {
                return Err(StoreError::Backend(
                    format!(
                        "corrupt SingleGram chunk: item {index} token {token} does not \
                         follow {} in ascending order",
                        previous.unwrap_or_default()
                    )
                    .into(),
                ));
            }
            previous = Some(token);
            items.push((token, freq));
        }
        if items.is_empty() && total != 0 {
            return Err(StoreError::Backend(
                format!("corrupt SingleGram chunk: no items but total_freq is {total}").into(),
            ));
        }
        Ok(Self { total, items })
    }

    /// Lays the chunk out as libpinyin reads it.
    ///
    /// The inverse of [`Self::decode`] byte for byte: decode-then-encode
    /// reproduces the input exactly, which is the property the round-trip
    /// test over the real system file asserts.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER + self.items.len() * ITEM);
        out.extend_from_slice(&self.total.to_ne_bytes());
        for (token, freq) in &self.items {
            out.extend_from_slice(&token.to_ne_bytes());
            out.extend_from_slice(&freq.to_ne_bytes());
        }
        out
    }

    /// `total_freq` — upstream's `get_total_freq`.
    #[must_use]
    pub const fn total(&self) -> u32 {
        self.total
    }

    /// Overwrites `total_freq` — upstream's `set_total_freq`.
    ///
    /// Upstream keeps the total as an independent field rather than a sum
    /// of the items (`train`'s deletion path lowers items without
    /// lowering the total), so this does not touch the array.
    pub const fn set_total(&mut self, total: u32) {
        self.total = total;
    }

    /// The count stored for `token`, or `None` — upstream's `get_freq`,
    /// whose out-parameter stays 0 on a miss.
    #[must_use]
    pub fn freq(&self, token: u32) -> Option<u32> {
        self.items
            .binary_search_by_key(&token, |(stored, _)| *stored)
            .ok()
            .map(|index| self.items[index].1)
    }

    /// Inserts or overwrites `token`'s count, keeping the array sorted —
    /// upstream's `insert_freq`, whose `lower_bound` placement is what
    /// makes the array ascending in the first place.
    ///
    /// Returns the previous count when the token was already present.
    pub fn set_freq(&mut self, token: u32, freq: u32) -> Option<u32> {
        match self
            .items
            .binary_search_by_key(&token, |(stored, _)| *stored)
        {
            Ok(index) => Some(std::mem::replace(&mut self.items[index].1, freq)),
            Err(index) => {
                self.items.insert(index, (token, freq));
                None
            }
        }
    }

    /// Removes `token` — upstream's `remove_freq` — returning its count.
    pub fn remove_freq(&mut self, token: u32) -> Option<u32> {
        self.items
            .binary_search_by_key(&token, |(stored, _)| *stored)
            .ok()
            .map(|index| self.items.remove(index).1)
    }

    /// The successor items, ascending by token.
    #[must_use]
    pub fn items(&self) -> &[(u32, u32)] {
        &self.items
    }

    /// Number of successors.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the gram has no successors.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{HEADER, ITEM, SingleGram};

    #[test]
    fn a_fresh_gram_is_four_zero_bytes() {
        // Upstream's fresh chunk: 4 bytes, total 0, no items.
        let fresh = SingleGram::new();
        assert_eq!(fresh.encode(), vec![0, 0, 0, 0]);
        assert_eq!(fresh.encode().len(), HEADER);
        assert!(fresh.is_empty());
    }

    #[test]
    fn items_stay_ascending_however_they_arrive() {
        // `insert_freq`'s lower_bound placement, which is the only reason
        // the arrays in the real file are sorted at all.
        let mut gram = SingleGram::new();
        for token in [0x0100_a271, 0x0100_05db, 0x0100_6538, 0x0100_298c] {
            assert_eq!(gram.set_freq(token, 1), None);
        }
        let tokens: Vec<u32> = gram.items().iter().map(|(token, _)| *token).collect();
        assert_eq!(
            tokens,
            [0x0100_05db, 0x0100_298c, 0x0100_6538, 0x0100_a271],
            "insertion order must not survive into the chunk"
        );
        assert_eq!(gram.set_freq(0x0100_6538, 9), Some(1), "overwrite reports");
        assert_eq!(gram.freq(0x0100_6538), Some(9));
        assert_eq!(gram.remove_freq(0x0100_05db), Some(1));
        assert_eq!(gram.freq(0x0100_05db), None);
        assert_eq!(gram.len(), 3);
    }

    #[test]
    fn encode_reproduces_the_pins_own_bytes() {
        // The first record of the installed system bigram.db, transcribed
        // from the C probe: prev 0x03001801, total 65, four items. The
        // file is Berkeley DB on this machine, but the chunk inside it is
        // the backend-independent one this module encodes.
        let mut gram = SingleGram::new();
        gram.set_total(65);
        gram.set_freq(0x0100_05db, 8);
        gram.set_freq(0x0100_298c, 3);
        gram.set_freq(0x0100_6538, 2);
        gram.set_freq(0x0100_a271, 52);
        let bytes = gram.encode();
        assert_eq!(bytes.len(), HEADER + 4 * ITEM, "36 bytes, as the file has");
        assert_eq!(&bytes[0..4], &65_u32.to_ne_bytes());
        assert_eq!(&bytes[4..8], &0x0100_05db_u32.to_ne_bytes());
        assert_eq!(&bytes[8..12], &8_u32.to_ne_bytes());
        assert_eq!(
            SingleGram::decode(&bytes).expect("round trip"),
            gram,
            "decode is encode's exact inverse"
        );
    }

    #[test]
    fn a_chunk_that_is_not_four_plus_eight_n_is_refused() {
        for length in [0_usize, 1, 3, 5, 11, 13] {
            let bytes = vec![0_u8; length];
            assert!(
                SingleGram::decode(&bytes).is_err(),
                "{length} bytes is not a valid chunk and must not decode"
            );
        }
        assert!(
            SingleGram::decode(&[0, 0, 0, 0]).is_ok(),
            "4 bytes is fresh"
        );
        assert!(SingleGram::decode(&[0; 12]).is_ok(), "4 + 8 is one item");
    }

    #[test]
    fn a_descending_or_duplicated_item_array_is_refused() {
        // Upstream's insert_freq cannot produce either, so a file that has
        // one is damaged and must not be silently re-encoded.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&2_u32.to_ne_bytes());
        bytes.extend_from_slice(&9_u32.to_ne_bytes());
        bytes.extend_from_slice(&1_u32.to_ne_bytes());
        bytes.extend_from_slice(&4_u32.to_ne_bytes());
        bytes.extend_from_slice(&1_u32.to_ne_bytes());
        assert!(SingleGram::decode(&bytes).is_err(), "descending is refused");

        let mut duplicated = Vec::new();
        duplicated.extend_from_slice(&2_u32.to_ne_bytes());
        for _ in 0..2 {
            duplicated.extend_from_slice(&7_u32.to_ne_bytes());
            duplicated.extend_from_slice(&1_u32.to_ne_bytes());
        }
        assert!(
            SingleGram::decode(&duplicated).is_err(),
            "a repeated token is refused"
        );
    }

    #[test]
    fn a_zero_item_chunk_with_a_total_is_refused() {
        // `get_length`'s assertion, carried over: an empty gram has no mass.
        let bytes = 7_u32.to_ne_bytes().to_vec();
        assert!(SingleGram::decode(&bytes).is_err());
    }
}
