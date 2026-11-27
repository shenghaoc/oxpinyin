//! The batch-parse seams and one-key probes over the core parsers.

use crate::OptionBits;
use crate::full_pinyin_index::parse_full_pinyin_index_with_options;
use crate::graph::ExactSegment;
use crate::parser::{
    ChewingKey, DoublePinyinKey, DoublePinyinParser, FullPinyinParser, FullPinyinScheme, ZhuyinKey,
    ZhuyinParser, ZhuyinScheme,
};
use crate::vocab::SyllableKey;

use crate::engine::MatrixKey;
use crate::facade::instance::{InstanceState, InstanceView, ModeParse};

/// How the facade forwards the option word for one-key and batch chewing parses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChewingCorrections {
    /// Pinyin clears `ZHUYIN_CORRECT_ALL` (`pinyin.cpp:1573,1589`).
    Masked,
    /// Zhuyin forwards the full option word (`zhuyin.cpp:1049-1054,1061-1070`).
    FromOptions,
}

impl ChewingCorrections {
    fn apply(self, options: OptionBits) -> OptionBits {
        match self {
            Self::Masked => options - OptionBits::ZHUYIN_CORRECT_ALL,
            Self::FromOptions => options,
        }
    }
}

/// Which correction bits the one-key full-pinyin probe reads.
///
/// Both facades run the same `parse_full_pinyin` probe on the live
/// option word; the one difference is the `PINYIN_CORRECT_ALL` mask,
/// which the libzhuyin facade applies (`zhuyin.cpp:1013` at the pin)
/// and the pinyin facade does not (`pinyin.cpp:1484`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PinyinCorrections {
    /// The pinyin facade's law: the probe reads the live option word
    /// unchanged.
    FromOptions,
    /// The libzhuyin facade's law: the probe reads the live option word
    /// minus `PINYIN_CORRECT_ALL`.
    Masked,
}

/// The full-pinyin-scheme dispatch over the header discriminants.
#[must_use]
pub fn full_scheme(value: i32) -> Option<FullPinyinScheme> {
    FullPinyinScheme::try_from(value).ok()
}

/// The zhuyin-scheme dispatch over the header discriminants.
///
/// Total over
/// 1..=9 — the `STANDARD_DVORAK` (7) abort slot parses nothing but is a
/// real header value; the C setters refuse it before it can be stored.
#[must_use]
pub fn zhuyin_scheme(value: i32) -> Option<ZhuyinScheme> {
    ZhuyinScheme::try_from(value).ok()
}

/// The double-pinyin-scheme dispatch over the header discriminants. The
/// setter validates with `DoublePinyinScheme::try_from` itself, so only the
/// tests read this.
#[cfg(test)]
#[must_use]
pub(crate) fn double_scheme(value: i32) -> Option<crate::parser::DoublePinyinScheme> {
    use crate::parser::DoublePinyinScheme;
    DoublePinyinScheme::try_from(value)
        .ok()
        .filter(|scheme| *scheme != DoublePinyinScheme::Customized)
}

/// The per-key view [`exact_input`] needs: the resolved syllable and its
/// tone.
pub(crate) trait ExactKey {
    /// The full-pinyin key this scheme key resolved to.
    fn key(&self) -> SyllableKey;
    /// The tone consumed with the key, `0` when toneless.
    fn tone(&self) -> u8;
}

impl ExactKey for ZhuyinKey {
    fn key(&self) -> SyllableKey {
        ZhuyinKey::key(self)
    }
    fn tone(&self) -> u8 {
        ZhuyinKey::tone(self)
    }
}

impl ExactKey for DoublePinyinKey {
    fn key(&self) -> SyllableKey {
        DoublePinyinKey::key(*self)
    }
    fn tone(&self) -> u8 {
        DoublePinyinKey::tone(*self)
    }
}

/// Builds the exact-decoder input for a scheme parse: the `'`-joined
/// full-pinyin text plus one [`ExactSegment`] per key over that text.
///
/// The session's graph then carries exactly the scheme parser's keys —
/// the pinyin inventory never re-segments the joined spelling (upstream's
/// decoder receives the parser's `ChewingKey`s;
/// `docs/findings/bopomofo-spec.md`).
#[must_use]
pub(crate) fn exact_input(keys: &[impl ExactKey]) -> (String, Vec<ExactSegment>) {
    let mut text = String::new();
    let mut segments = Vec::with_capacity(keys.len());
    for key in keys {
        if !text.is_empty() {
            text.push('\'');
        }
        let start = text.len();
        text.push_str(key.key().text());
        segments.push(ExactSegment::new(start, text.len(), key.key(), key.tone()));
    }
    (text, segments)
}

/// `FullPinyinParser2::parse_one_key` (`pinyin_parser2.cpp:164-214` at
/// 074a2219) over a scheme index: an apostrophe is the assert shape,
/// returned as `None` here for the C adapters to abort on; under `USE_TONE` a trailing `1..5`
/// is the tone, which `FORCE_TONE` then requires; the remaining spelling
/// must be one row of the index exactly, and the key is the row's
/// canonical syllable with that tone.
fn parse_one_scheme_key(
    options: OptionBits,
    input: &[u8],
    index: crate::parser::FullPinyinIndex,
) -> Option<ChewingKey> {
    if input.contains(&b'\'') {
        return None;
    }
    let use_tone = options.contains(OptionBits::USE_TONE);
    let force_tone = options.contains(OptionBits::FORCE_TONE);
    let mut tone = crate::chewing_key::CHEWING_ZERO_TONE;
    let mut spelling = input;
    if use_tone {
        if let Some((&last, rest)) = input.split_last()
            && (b'1'..=b'5').contains(&last)
        {
            tone = last - b'0';
            spelling = rest;
        }
        if force_tone && tone == crate::chewing_key::CHEWING_ZERO_TONE {
            return None;
        }
    }
    let spelling = core::str::from_utf8(spelling).ok()?;
    let canonical = index.canonical(spelling)?;
    let mut key = ChewingKey::from_pinyin(canonical)?;
    key.tone = tone;
    Some(key)
}

impl<S: std::borrow::Borrow<InstanceState>, E: std::borrow::Borrow<crate::engine::SessionState>>
    InstanceView<'_, S, E>
{
    /// The one-key full-pinyin probe — the `parse_full_pinyin` law. The
    /// two facades' pins differ in exactly one bit, which
    /// [`PinyinCorrections`] names. `None` is upstream's `false`.
    #[must_use]
    pub fn parse_one_full_pinyin(
        &self,
        text: &str,
        corrections: PinyinCorrections,
    ) -> Option<ChewingKey> {
        let options = match corrections {
            PinyinCorrections::FromOptions => self.options(),
            PinyinCorrections::Masked => self.options() - OptionBits::PINYIN_CORRECT_ALL,
        };
        // The probe runs on the context's live full-pinyin parser, whose
        // index `pinyin_set_full_pinyin_scheme` swaps
        // (`FullPinyinParser2::set_scheme`, `pinyin_parser2.cpp:383-401`
        // at 074a2219): under LUOMA / SECONDARY_ZHUYIN the spelling is
        // looked up in that scheme's table — plain rows, no option gating
        // — and the key is the canonical syllable's (issue #547).
        if let Some(index) = self.live.full_scheme.get().index() {
            return parse_one_scheme_key(options, text.as_bytes(), index);
        }
        FullPinyinParser::parse_one_key(options, text.as_bytes())
    }

    /// The live double-pinyin parser: the live scheme, with its fallback
    /// table suppressed after an out-of-enum set.
    fn live_double_parser(&self) -> DoublePinyinParser {
        let parser = DoublePinyinParser::with_scheme(self.live.double_scheme.get());
        if self.live.double_fallback_cleared.get() {
            parser.with_fallback_suppressed()
        } else {
            parser
        }
    }

    /// The one-key double-pinyin probe — pinyin-facade-only, over the
    /// live double scheme and the raw option word. `None` is upstream's
    /// `false`.
    #[must_use]
    pub fn parse_one_double_pinyin(&self, text: &str) -> Option<ChewingKey> {
        self.live_double_parser()
            .parse_one_key(self.options(), text.as_bytes())
    }

    /// Parses one chewing key with the live scheme and the facade's correction options.
    #[must_use]
    pub fn parse_one_chewing(
        &self,
        text: &str,
        corrections: ChewingCorrections,
    ) -> Option<ChewingKey> {
        let options = corrections.apply(self.options());
        ZhuyinParser::with_scheme(self.live.zhuyin_scheme.get())
            .parse_one_key(options, text.as_bytes())
    }

    /// The zhuyin symbol(s) one keystroke maps to — the mapping half of
    /// `in_chewing_keyboard`, both facades' law: the live scheme's
    /// symbols under the live `USE_TONE` flag. Empty means the key is
    /// not on the keyboard (upstream's `false`).
    #[must_use]
    pub fn in_keyboard(&self, key: u8) -> Vec<&'static str> {
        let use_tone = self.options().contains(OptionBits::USE_TONE);
        ZhuyinParser::with_scheme(self.live.zhuyin_scheme.get()).symbols_for(key, use_tone)
    }
}

impl<
    S: std::borrow::BorrowMut<InstanceState>,
    E: std::borrow::BorrowMut<crate::engine::SessionState>,
> InstanceView<'_, S, E>
{
    /// The pinyin full-pinyin batch parse. Its parse-time matrix applies
    /// resplit, inner split and fuzzy expansion (`pinyin.cpp:1514-1520`
    /// at 074a2219).
    ///
    /// Returns the original-input bytes consumed, 0 on failure or empty
    /// input.
    #[must_use]
    pub fn parse_full_more(&mut self, text: &str) -> usize {
        self.parse_full_more_with_splits(text, true)
    }

    /// The zhuyin full-pinyin batch parse. Its parse-time matrix applies
    /// fuzzy expansion after the fill (`zhuyin.cpp:1036-1038` at 074a2219).
    ///
    /// Returns the original-input bytes consumed, 0 on failure or empty
    /// input.
    #[must_use]
    pub fn parse_full_more_zhuyin(&mut self, text: &str) -> usize {
        self.parse_full_more_with_splits(text, false)
    }

    fn parse_full_more_with_splits(&mut self, text: &str, divided: bool) -> usize {
        self.begin_parse();
        if self.session.set_options(self.options()).is_err() {
            return 0;
        }
        if text.is_empty() {
            // fill_matrix clears the old matrix before testing keys->len
            // (074a2219, phonetic_key_matrix.cpp:34-38). Keep sentence
            // results until the next guess, but replace the session input.
            let _ = self.session.replace_raw("");
            return 0;
        }
        if let Some(index) = self.live.full_scheme.get().index() {
            let parsed =
                parse_full_pinyin_index_with_options(text.as_bytes(), self.options(), index);
            // The session reads the consumed original bytes with each key
            // at its own span, as plain full pinyin does: the parser is
            // the Hanyu one over another index (`set_scheme`,
            // `pinyin_parser2.cpp:383-401` at 074a2219), so `fill_matrix`
            // places keys and `'` zero keys at the same original columns
            // (`phonetic_key_matrix.cpp:52-79`).
            let consumed = text.get(..parsed.consumed()).unwrap_or(text);
            let segments: Vec<_> = parsed
                .keys()
                .iter()
                .filter_map(|key| {
                    SyllableKey::from_canonical_text(key.canonical()).map(|syllable| {
                        ExactSegment::new(key.start(), key.end(), syllable, key.tone())
                    })
                })
                .collect();
            if !segments.is_empty()
                && self
                    .session
                    .replace_raw_full_pinyin(consumed, &segments)
                    .is_err()
            {
                return 0;
            }
            self.parsed_len = parsed.consumed();
            let keys: Vec<_> = parsed
                .keys()
                .iter()
                .map(|key| {
                    let chewing = ChewingKey::from_pinyin(key.canonical()).unwrap_or_default();
                    MatrixKey::from_chewing(chewing.with_tone(key.tone()), key.start(), key.end())
                })
                .collect();
            self.keep_parse(text, ModeParse::Full(parsed), &keys, divided);
            return self.parsed_len;
        }
        let consumed = match self.session.replace_raw(text) {
            Ok(()) => self.session.full_parsed_len(),
            Err(_) => 0,
        };
        self.parsed_len = consumed;
        consumed
    }

    /// Stores `parse` and its input, and builds the pin's parse-time
    /// matrix from the parser's `keys` under the options in force now:
    /// `fill_matrix`, then `resplit_step` and `inner_split_step` when
    /// `divided` (pinyin's full-pinyin entry point, `pinyin.cpp:1514-1520`),
    /// then `fuzzy_syllable_step`. The remember walk reads the matrix
    /// later, whatever the options have become since.
    fn keep_parse(&mut self, text: &str, parse: ModeParse, keys: &[MatrixKey], divided: bool) {
        text.clone_into(&mut self.parse_input);
        self.mode_parse = Some(parse);
        self.parse_matrix =
            crate::engine::parse_time_keys(keys, self.parsed_len, self.options(), divided);
    }

    /// The chewing batch seam — the shared skeleton (continue/restart,
    /// scheme dispatch, empty check, exact-input drive, state store) with
    /// the facade's [`ChewingCorrections`].
    ///
    /// Returns the original-input bytes consumed, 0 on failure or empty
    /// input.
    #[must_use]
    pub fn parse_chewing_more(&mut self, text: &str, corrections: ChewingCorrections) -> usize {
        self.begin_parse();
        // The pin fills and fuzzes with the live context word before a
        // sentence guess (pinyin.cpp:1589-1604, zhuyin.cpp:1061-1076).
        if self.session.set_options(self.options()).is_err() {
            return 0;
        }

        let parser = ZhuyinParser::with_scheme(self.live.zhuyin_scheme.get());
        let parsed = parser.parse_with_options(text.as_bytes(), corrections.apply(self.options()));

        if text.is_empty() {
            // The same fill_matrix empty-input law as full pinyin.
            let _ = self.session.replace_raw("");
            self.parsed_len = 0;
            return 0;
        }

        let (full, segments) = exact_input(parsed.keys());
        if !full.is_empty() && self.session.replace_raw_exact(&full, &segments).is_err() {
            return 0;
        }

        self.parsed_len = parsed.consumed();
        let keys: Vec<_> = parsed.keys().iter().map(zhuyin_matrix_key).collect();
        self.keep_parse(text, ModeParse::Zhuyin(parsed), &keys, false);
        self.parsed_len
    }

    /// The double-pinyin batch seam — pinyin-facade-only (libzhuyin has
    /// no double-pinyin surface), carried here because it is the same
    /// orchestration shape: parse through the live scheme under the full
    /// option word (the frozen double-pinyin SPEC's Tone section), with the
    /// session under that same word, then drive the decoder with the exact
    /// segments.
    ///
    /// Returns the original-input bytes consumed, 0 on failure or empty
    /// input.
    #[must_use]
    pub fn parse_double_more(&mut self, text: &str) -> usize {
        self.begin_parse();
        // Match fill-then-fuzzy under the whole context word, applied once
        // per parse (pinyin.cpp:1543-1559, pin 074a2219).
        if self.session.set_options(self.options()).is_err() {
            return 0;
        }

        let parsed = self
            .live_double_parser()
            .parse_with_options(text.as_bytes(), self.options());

        if text.is_empty() {
            // The same fill_matrix empty-input law as full pinyin.
            let _ = self.session.replace_raw("");
            self.parsed_len = 0;
            return 0;
        }

        let (full, segments) = exact_input(parsed.keys());
        if !full.is_empty() && self.session.replace_raw_exact(&full, &segments).is_err() {
            return 0;
        }

        self.parsed_len = parsed.consumed();
        let keys: Vec<_> = parsed.keys().iter().map(double_matrix_key).collect();
        self.keep_parse(text, ModeParse::Double(parsed), &keys, false);
        self.parsed_len
    }
}

/// A zhuyin parser key as a matrix key: its chewing key under its tone.
fn zhuyin_matrix_key(key: &ZhuyinKey) -> MatrixKey {
    MatrixKey::from_chewing(
        key.key().chewing_key().with_tone(key.tone()),
        key.start(),
        key.end(),
    )
}

/// A double-pinyin parser key as a matrix key: its chewing key under its
/// tone.
fn double_matrix_key(key: &DoublePinyinKey) -> MatrixKey {
    MatrixKey::from_chewing(
        key.key().chewing_key().with_tone(key.tone()),
        key.start(),
        key.end(),
    )
}

#[cfg(test)]
mod tests;
