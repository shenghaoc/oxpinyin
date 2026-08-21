//! Initial-key projection for the incomplete-index probe list.
//!
//! `SystemDictionary::open` derives one initial key per pinyin-index key:
//! every syllable replaced by the longest incomplete key that prefixes it
//! (`syllable_initial`), or `0` when none does, joined by `'`. The load
//! used to pay a 23-key prefix scan per syllable and then sort the
//! projected `String`s; this module does both cheaper:
//!
//! - [`InitialAlphabet`] answers "which initial prefixes this syllable"
//!   from a first-two-bytes table built once per load — the incomplete
//!   keys are all one or two lowercase letters, so the first two bytes of
//!   the syllable decide the match, longest key winning.
//! - [`InitialAlphabet::pack`] folds a whole projected key into one
//!   `u128`: five bits per syllable slot, 25 slots, slot 0 most
//!   significant. The packed order **is** the joined-string order (proof
//!   in [`InitialAlphabet::pack`]), so the load sorts and dedups plain
//!   integers and decodes only the survivors.
//!
//! Every path has a fallback for inputs the fast shapes cannot hold (a
//! future inventory with three-letter initials, a 26-syllable key): the
//! projection degrades to `syllable_initial` itself and the list to a
//! string sort, with identical results.

use oxpinyin_core::{INCOMPLETE_PINYIN_KEYS, syllable_initial};

/// Bits per syllable slot in a packed key.
const SLOT_BITS: u32 = 5;
/// Slots in a packed key: `25 × 5 = 125 ≤ 128` bits.
const MAX_SLOTS: usize = 25;
/// Slot code of the `0` vowel-initial sentinel.
const VOWEL_CODE: u128 = 1;
/// First code of an incomplete key; `key + 1` for byte-sorted keys.
const FIRST_KEY_CODE: u128 = 2;

/// The per-load projection tables.
pub(crate) struct InitialAlphabet {
    /// Slot code for a syllable spelling, indexed by its first byte
    /// (`a..=z` → `0..26`) and second byte (`a..=z` → `0..26`, anything
    /// else — including no second byte — → `26`). Unmatched spellings
    /// carry [`VOWEL_CODE`].
    code_by_prefix: [[u128; 27]; 26],
    /// The incomplete keys in byte order; slot code `FIRST_KEY_CODE + i`
    /// projects to `keys[i]`.
    sorted_keys: Box<[&'static str]>,
    /// Whether `code_by_prefix` is exact: every incomplete key is one or
    /// two lowercase letters and the codes fit five bits. When false the
    /// caller must fall back to [`syllable_initial`].
    table_valid: bool,
}

impl InitialAlphabet {
    /// Builds the tables from the frozen inventory.
    pub(crate) fn new() -> Self {
        let mut sorted_keys: Vec<&'static str> = INCOMPLETE_PINYIN_KEYS.to_vec();
        sorted_keys.sort_unstable();
        let mut table_valid = sorted_keys.len() + FIRST_KEY_CODE as usize <= 1 << SLOT_BITS;
        let mut code_by_prefix = [[VOWEL_CODE; 27]; 26];
        for (rank, key) in sorted_keys.iter().enumerate() {
            let bytes = key.as_bytes();
            if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_lowercase) || bytes.len() > 2 {
                table_valid = false;
                continue;
            }
            let code = FIRST_KEY_CODE + rank as u128;
            let first = (bytes[0] - b'a') as usize;
            if bytes.len() == 1 {
                // Matches any second byte, and loses to a two-letter key
                // that shares the first byte (assigned later: byte order
                // puts `z` before `zh`, and `zh` rewrites its own cell).
                code_by_prefix[first] = [code; 27];
            } else {
                code_by_prefix[first][(bytes[1] - b'a') as usize] = code;
            }
        }
        Self {
            code_by_prefix,
            sorted_keys: sorted_keys.into_boxed_slice(),
            table_valid,
        }
    }

    /// Slot code of one syllable: the code of its longest prefixing
    /// incomplete key, or [`VOWEL_CODE`] when none does — the two-byte
    /// table answer to [`syllable_initial`].
    fn slot_code(&self, syllable: &str) -> Option<u128> {
        if !self.table_valid {
            return None;
        }
        let bytes = syllable.as_bytes();
        let first = *bytes.first()?;
        if !first.is_ascii_lowercase() {
            // No incomplete key starts a non-lowercase byte, so
            // `syllable_initial` answers `None` for every such spelling.
            return Some(VOWEL_CODE);
        }
        let second = bytes
            .get(1)
            .filter(|&&byte| byte.is_ascii_lowercase())
            .map_or(26usize, |&byte| (byte - b'a') as usize);
        Some(self.code_by_prefix[(first - b'a') as usize][second])
    }

    /// Slot code via the exact fallback: the code of the initial
    /// `syllable_initial` reports. `None` when there is no initial, or —
    /// unreachable in practice, since `syllable_initial` answers inventory
    /// entries — when the initial is missing from the sorted keys.
    fn slot_code_slow(&self, syllable: &str) -> Option<u128> {
        let initial = syllable_initial(syllable)?;
        Some(FIRST_KEY_CODE + self.sorted_keys.iter().position(|key| *key == initial)? as u128)
    }

    /// Slot code either way: the two-byte table when it is exact, else the
    /// `syllable_initial` fallback.
    fn code_of(&self, syllable: &str) -> Option<u128> {
        self.slot_code(syllable)
            .or_else(|| self.slot_code_slow(syllable))
    }

    /// Folds a projected initial key into one `u128`: five bits per
    /// syllable slot, slot 0 in the most significant bits, zero padding
    /// after the last slot. `None` when the key has more than
    /// [`MAX_SLOTS`] syllables or a slot code does not fit five bits.
    ///
    /// Packing preserves the joined-string order — the order the probe
    /// list is binary-searched under. Writing every slot value in bracket
    /// order, the joined string is `i₀ ' i₁ ' …` and its byte order is
    /// `i₀ <₀ i₁ <₀ …` slot by slot, where within one slot the padding
    /// after a shorter key (0), the `0` sentinel (1), and the keys in
    /// byte order (2…) sort exactly as their projected bytes do: `'`
    /// (0x27) < `0` (0x30) < lowercase letters, a one-letter key's `'`
    /// separator undercuts the second letter of a two-letter key, and
    /// zero padding sorts a shorter key first.
    pub(crate) fn pack(&self, pinyin: &str) -> Option<u128> {
        // A code above 31 would bleed into the next slot's bits, so stop
        // packing while the order is still exact.
        if FIRST_KEY_CODE + self.sorted_keys.len() as u128 > 0x1f {
            return None;
        }
        let mut packed = 0_u128;
        let mut slot = 0_usize;
        let mut syllable_start = 0_usize;
        for (index, byte) in pinyin.bytes().enumerate() {
            // `'` is ASCII, so it can only appear as a whole character;
            // both sides of the split are char boundaries by construction.
            if byte == b'\'' {
                let code = self.code_of(&pinyin[syllable_start..index])?;
                packed |= code << slot_shift(slot);
                slot += 1;
                if slot >= MAX_SLOTS {
                    return None;
                }
                syllable_start = index + 1;
            }
        }
        let code = self.code_of(&pinyin[syllable_start..])?;
        packed |= code << slot_shift(slot);
        Some(packed)
    }

    /// Renders the projected key of `pinyin` as its joined string — the
    /// [`InitialAlphabet::pack`] inverse for the slot values, with the
    /// same fallbacks. A syllable no path can code (unreachable; every
    /// inventory initial is codable) projects as the vowel sentinel.
    pub(crate) fn project(&self, pinyin: &str) -> String {
        let mut projected = String::new();
        for (position, syllable) in pinyin.split('\'').enumerate() {
            if position > 0 {
                projected.push('\'');
            }
            match self.code_of(syllable) {
                Some(code) => push_slot(&mut projected, code, &self.sorted_keys),
                None => projected.push('0'),
            }
        }
        projected
    }

    /// Decodes a packed key back to its joined string.
    pub(crate) fn unpack(&self, packed: u128) -> String {
        let mut projected = String::new();
        for position in 0..MAX_SLOTS {
            let code = (packed >> (u128::BITS - SLOT_BITS * (position as u32 + 1))) & 0x1f;
            if code == 0 {
                break;
            }
            if position > 0 {
                projected.push('\'');
            }
            push_slot(&mut projected, code, &self.sorted_keys);
        }
        projected
    }
}

/// Bit shift of slot `position`'s field: slot 0 is the most significant.
const fn slot_shift(position: usize) -> u32 {
    u128::BITS - SLOT_BITS * (position as u32 + 1)
}

/// Appends one slot's projection: `0` for the vowel sentinel, the key for
/// key codes.
fn push_slot(projected: &mut String, code: u128, sorted_keys: &[&'static str]) {
    if code == VOWEL_CODE {
        projected.push('0');
    } else if let Ok(index) = usize::try_from(code - FIRST_KEY_CODE)
        && let Some(key) = sorted_keys.get(index)
    {
        projected.push_str(key);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::InitialAlphabet;
    use oxpinyin_core::{FULL_PINYIN_SYLLABLES, INCOMPLETE_PINYIN_KEYS, syllable_initial};

    /// The joined projection of `pinyin` under `syllable_initial` — the
    /// pre-table behaviour the fast path must reproduce.
    fn reference_projection(pinyin: &str) -> String {
        let mut projected = String::new();
        for (position, syllable) in pinyin.split('\'').enumerate() {
            if position > 0 {
                projected.push('\'');
            }
            match syllable_initial(syllable) {
                Some(initial) => projected.push_str(initial),
                None => projected.push('0'),
            }
        }
        projected
    }

    #[test]
    fn fast_projection_matches_syllable_initial_on_every_syllable() {
        let alphabet = InitialAlphabet::new();
        for syllable in FULL_PINYIN_SYLLABLES
            .iter()
            .chain(INCOMPLETE_PINYIN_KEYS.iter())
        {
            assert_eq!(
                alphabet.project(syllable),
                reference_projection(syllable),
                "projection diverges for {syllable:?}"
            );
        }
        // Spellings the inventory never sees, including the empty fragment
        // and non-lowercase bytes.
        for syllable in ["", "a", "ai", "ng", "zH", "ü", "zzzz"] {
            assert_eq!(
                alphabet.project(syllable),
                reference_projection(syllable),
                "projection diverges for {syllable:?}"
            );
        }
    }

    #[test]
    fn packing_preserves_the_joined_string_order() {
        let alphabet = InitialAlphabet::new();
        let mut keys = BTreeSet::new();
        for syllables in [
            vec!["ni"],
            vec!["ni", "hao"],
            vec!["zhong", "guo"],
            vec!["ai"],
            vec!["a", "ai"],
            vec!["z", "a"],
            vec!["zh", "a"],
            vec!["z", "ha"],
            vec!["sh", "eng"],
            vec!["s", "heng"],
            vec!["er", "zi"],
            vec!["e"],
        ] {
            keys.insert(syllables.join("'"));
        }
        let mut joined: Vec<String> = keys.iter().map(|key| alphabet.project(key)).collect();
        joined.sort();
        joined.dedup();
        let mut packed: Vec<u128> = keys.iter().filter_map(|key| alphabet.pack(key)).collect();
        packed.sort_unstable();
        packed.dedup();
        let unpacked: Vec<String> = packed.iter().map(|&key| alphabet.unpack(key)).collect();
        assert_eq!(
            joined, unpacked,
            "packed order must equal the joined-string order"
        );
    }

    #[test]
    fn random_keys_keep_the_joined_string_order() {
        // Deterministic LCG: no test RNG dependency, reproducible failures.
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as usize
        };
        let alphabet = InitialAlphabet::new();
        let vocabulary: Vec<&str> = FULL_PINYIN_SYLLABLES
            .iter()
            .chain(INCOMPLETE_PINYIN_KEYS.iter())
            .copied()
            .collect();
        let mut joined = BTreeSet::new();
        let mut packed = Vec::new();
        for _ in 0..1_000 {
            let len = 1 + next() % 6;
            let syllables: Vec<&str> = (0..len)
                .map(|_| vocabulary[next() % vocabulary.len()])
                .collect();
            let key = syllables.join("'");
            joined.insert(alphabet.project(&key));
            if let Some(code) = alphabet.pack(&key) {
                packed.push(code);
            }
        }
        packed.sort_unstable();
        packed.dedup();
        let unpacked: Vec<String> = packed.iter().map(|&code| alphabet.unpack(code)).collect();
        assert_eq!(
            joined.into_iter().collect::<Vec<_>>(),
            unpacked,
            "packed order must equal the joined-string order on random keys"
        );
    }

    #[test]
    fn packing_round_trips_long_keys() {
        let alphabet = InitialAlphabet::new();
        // 14 syllables — the longest key in the pinned export — round
        // trips; 26 syllables reports `None` and projects via the string
        // path instead.
        let long = "bin'wei'ye'sheng'dong'zhi'wu'zhong'guo'ji'mao'yi'gong'yue";
        assert_eq!(
            alphabet.unpack(alphabet.pack(long).unwrap()),
            alphabet.project(long)
        );
        let oversized = ["ni"; 26];
        assert!(alphabet.pack(&oversized.join("'")).is_none());
        assert_eq!(
            alphabet.project(&oversized.join("'")).split('\'').count(),
            26
        );
    }
}
