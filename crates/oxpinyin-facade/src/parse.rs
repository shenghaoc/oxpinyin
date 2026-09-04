//! The batch-parse seams and one-key probes over the core parsers.

use std::sync::atomic::Ordering;

use oxpinyin_core::graph::ExactSegment;
use oxpinyin_core::{
    ChewingKey, DoublePinyinKey, DoublePinyinParser, DoublePinyinScheme, FullPinyinParser,
    FullPinyinScheme, PINYIN_CORRECT_ALL, SyllableKey, USE_TONE, ZHUYIN_CORRECT_ALL, ZhuyinKey,
    ZhuyinParser, ZhuyinScheme, parse_full_pinyin_index,
};

use crate::instance::InstanceCore;

/// Which option bits the chewing batch seam forwards into the parser.
///
/// The two facades' pins genuinely differ here, and both are faithful to
/// their own upstream: the pinyin facade forwards `USE_TONE` and
/// `ZHUYIN_INCOMPLETE` only — `FORCE_TONE` does not cross its seam (the
/// open item recorded in `docs/findings/upstream-divergences.md`, out of
/// every reference consumer's reach per the compatibility policy's
/// availability class) — while the libzhuyin facade forwards the whole
/// option word (`zhuyin.cpp:1061` at the pin). The shared skeleton is
/// identical; this enum is the three-line difference, kept greppable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToneForwarding {
    /// The pinyin facade's law: `parse(use_tone, allow_incomplete)`.
    PinFacade,
    /// The libzhuyin facade's law: `parse_with_options(full word)`.
    ZhuyinFacade,
}

/// The full-pinyin-scheme dispatch over the header discriminants.
#[must_use]
pub const fn full_scheme(value: i32) -> Option<FullPinyinScheme> {
    match value {
        1 => Some(FullPinyinScheme::Hanyu),
        2 => Some(FullPinyinScheme::Luoma),
        3 => Some(FullPinyinScheme::SecondaryZhuyin),
        _ => None,
    }
}

/// The zhuyin-scheme dispatch over the header discriminants. Total over
/// 1..=9 — the `STANDARD_DVORAK` (7) abort slot parses nothing but is a
/// real header value; the C setters refuse it before it can be stored.
#[must_use]
pub const fn zhuyin_scheme(value: i32) -> Option<ZhuyinScheme> {
    match value {
        1 => Some(ZhuyinScheme::Standard),
        2 => Some(ZhuyinScheme::Hsu),
        3 => Some(ZhuyinScheme::Ibm),
        4 => Some(ZhuyinScheme::Ginyieh),
        5 => Some(ZhuyinScheme::Eten),
        6 => Some(ZhuyinScheme::Eten26),
        7 => Some(ZhuyinScheme::StandardDvorak),
        8 => Some(ZhuyinScheme::HsuDvorak),
        9 => Some(ZhuyinScheme::DachenCp26),
        _ => None,
    }
}

/// The double-pinyin-scheme dispatch over the header discriminants.
#[must_use]
pub const fn double_scheme(value: i32) -> Option<DoublePinyinScheme> {
    match value {
        1 => Some(DoublePinyinScheme::Zrm),
        2 => Some(DoublePinyinScheme::Ms),
        3 => Some(DoublePinyinScheme::Ziguang),
        4 => Some(DoublePinyinScheme::Abc),
        5 => Some(DoublePinyinScheme::Pyjj),
        6 => Some(DoublePinyinScheme::Xhe),
        _ => None,
    }
}

/// The per-key view [`exact_input`] needs: the resolved syllable and its
/// tone.
pub trait ExactKey {
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
pub fn exact_input(keys: &[impl ExactKey]) -> (String, Vec<ExactSegment>) {
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

impl InstanceCore {
    /// The full-pinyin batch seam — the `parse_more_full_pinyins` law
    /// both facades implement identically: continue or restart the
    /// parse, remask the session under the live option word, then for
    /// LUOMA / `SECONDARY_ZHUYIN` parse the raw input through the
    /// scheme's pinned index and drive the session with the canonical
    /// spellings, else (Hanyu) replace the raw buffer directly.
    ///
    /// Returns the original-input bytes consumed, 0 on failure or empty
    /// input.
    #[must_use]
    pub fn parse_full_more(&mut self, text: &str) -> usize {
        self.begin_parse(text.as_bytes());
        if self.session.set_options(self.options()).is_err() {
            return 0;
        }
        if text.is_empty() {
            return 0;
        }
        if let Some(scheme) = full_scheme(self.live.full_scheme.load(Ordering::Relaxed))
            && let Some(index) = scheme.index()
        {
            let use_tone = self.options().contains(USE_TONE);
            let parsed = parse_full_pinyin_index(text.as_bytes(), use_tone, index);
            let full = parsed.full_pinyin();
            if !full.is_empty() && self.session.replace_raw(&full).is_err() {
                return 0;
            }
            self.parsed_len = parsed.consumed();
            text.clone_into(&mut self.full_input);
            self.full_parse = Some(parsed);
            return self.parsed_len;
        }
        let consumed = match self.session.replace_raw(text) {
            Ok(()) => self.session.full_parsed_len(),
            Err(_) => 0,
        };
        self.parsed_len = consumed;
        consumed
    }

    /// The chewing batch seam — the shared skeleton (continue/restart,
    /// scheme dispatch, empty check, exact-input drive, state store) with
    /// the parser invocation selected by `forwarding` (see
    /// [`ToneForwarding`]).
    ///
    /// Returns the original-input bytes consumed, 0 on failure or empty
    /// input.
    #[must_use]
    pub fn parse_chewing_more(&mut self, text: &str, forwarding: ToneForwarding) -> usize {
        self.begin_parse(text.as_bytes());

        let Some(scheme) = zhuyin_scheme(self.live.zhuyin_scheme.load(Ordering::Relaxed)) else {
            return 0;
        };
        let parser = ZhuyinParser::with_scheme(scheme);
        let parsed = match forwarding {
            // Upstream passes the caller's option word through after
            // stripping the parser-owned corrections; `USE_TONE` and
            // `ZHUYIN_INCOMPLETE` reach the parser, `FORCE_TONE` does not
            // cross this facade's seam (the recorded open item).
            ToneForwarding::PinFacade => {
                let use_tone = self.live.use_tone.load(Ordering::Relaxed);
                let allow_incomplete = self.options().contains(oxpinyin_core::ZHUYIN_INCOMPLETE);
                parser.parse(text.as_bytes(), use_tone, allow_incomplete)
            }
            // The libzhuyin facade forwards the whole word, so the pin's
            // default `USE_TONE | FORCE_TONE` is honoured by the batch
            // law (`zhuyin.cpp:1061` at the pin).
            ToneForwarding::ZhuyinFacade => {
                parser.parse_with_options(text.as_bytes(), self.options().bits())
            }
        };

        if text.is_empty() {
            self.parsed_len = 0;
            return 0;
        }

        let (full, segments) = exact_input(parsed.keys());
        if !full.is_empty() && self.session.replace_raw_exact(&full, &segments).is_err() {
            return 0;
        }

        self.parsed_len = parsed.consumed();
        text.clone_into(&mut self.zhuyin_input);
        self.zhuyin_parse = Some(parsed);
        self.parsed_len
    }

    /// The double-pinyin batch seam — pinyin-facade-only (libzhuyin has
    /// no double-pinyin surface), carried here because it is the same
    /// orchestration shape: parse through the live scheme under the full
    /// option word (the frozen double-pinyin SPEC's Tone section), apply
    /// the live incomplete bit to the session, then drive the decoder
    /// with the exact segments.
    ///
    /// Returns the original-input bytes consumed, 0 on failure or empty
    /// input.
    #[must_use]
    pub fn parse_double_more(&mut self, text: &str) -> usize {
        self.begin_parse(text.as_bytes());

        let Some(scheme) = double_scheme(self.live.double_scheme.load(Ordering::Relaxed)) else {
            return 0;
        };
        let allow_incomplete = self.live.incomplete.load(Ordering::Relaxed);
        let parser = DoublePinyinParser::with_scheme(scheme);
        let parsed = parser.parse_with_options(text.as_bytes(), self.options().bits());

        if text.is_empty() {
            self.parsed_len = 0;
            return 0;
        }

        if self
            .session
            .set_incomplete_pinyin(allow_incomplete)
            .is_err()
        {
            return 0;
        }

        let (full, segments) = exact_input(parsed.keys());
        if !full.is_empty() && self.session.replace_raw_exact(&full, &segments).is_err() {
            return 0;
        }

        self.parsed_len = parsed.consumed();
        text.clone_into(&mut self.double_input);
        self.double_parse = Some(parsed);
        self.parsed_len
    }

    /// The one-key full-pinyin probe — the `parse_full_pinyin` law. The
    /// two facades' pins differ in exactly one bit: the libzhuyin facade
    /// masks `PINYIN_CORRECT_ALL` off the live word before the probe
    /// (`zhuyin.cpp:1013`), the pinyin facade passes the word raw
    /// (`pinyin.cpp:1484`). `None` is upstream's `false`.
    #[must_use]
    pub fn parse_one_full_pinyin(&self, text: &str, mask_corrections: bool) -> Option<ChewingKey> {
        let mut options = self.options().bits();
        if mask_corrections {
            options &= !PINYIN_CORRECT_ALL;
        }
        FullPinyinParser.parse_one_key(options, text.as_bytes())
    }

    /// The one-key double-pinyin probe — pinyin-facade-only, over the
    /// live double scheme and the raw option word. `None` is upstream's
    /// `false`.
    #[must_use]
    pub fn parse_one_double_pinyin(&self, text: &str) -> Option<ChewingKey> {
        let scheme = double_scheme(self.live.double_scheme.load(Ordering::Relaxed))?;
        DoublePinyinParser::with_scheme(scheme)
            .parse_one_key(self.options().bits(), text.as_bytes())
    }

    /// The one-key chewing probe — the `parse_chewing` law both facades
    /// implement identically: the live scheme parses after the API's
    /// `ZHUYIN_CORRECT_ALL` strip (the caller's corrections never reach
    /// the chewing parser). `None` is upstream's `false`.
    #[must_use]
    pub fn parse_one_chewing(&self, text: &str) -> Option<ChewingKey> {
        let scheme = zhuyin_scheme(self.live.zhuyin_scheme.load(Ordering::Relaxed))?;
        let options = self.options().bits() & !ZHUYIN_CORRECT_ALL;
        ZhuyinParser::with_scheme(scheme).parse_one_key(options, text.as_bytes())
    }

    /// The zhuyin symbol(s) one keystroke maps to — the mapping half of
    /// `in_chewing_keyboard`, both facades' law: the live scheme's
    /// symbols under the live `USE_TONE` flag. Empty means the key is
    /// not on the keyboard (upstream's `false`).
    #[must_use]
    pub fn in_keyboard(&self, key: u8) -> Vec<String> {
        let use_tone = self.live.use_tone.load(Ordering::Relaxed);
        let Some(scheme) = zhuyin_scheme(self.live.zhuyin_scheme.load(Ordering::Relaxed)) else {
            return Vec::new();
        };
        ZhuyinParser::with_scheme(scheme).symbols_for(key, use_tone)
    }
}
