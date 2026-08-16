//! Sentence splitting into the line-oriented form `ngseg` reads.
//!
//! `ngseg` treats one line as one sentence: empty lines and the file tail
//! become `null_token` sentence boundaries, and bigram training resets at
//! each of them. The splitter therefore emits one sentence per line,
//! `\n`-terminated, non-empty, with boundary punctuation consumed.

/// Sentence-boundary punctuation.
const BOUNDARIES: &[char] = &['。', '！', '？', '；', '…'];

/// Punctuation trimmed from both ends of a fragment (never from the
/// middle).
const TRIM_PUNCT: &[char] = &[
    '，', '、', '：', '；', '。', '！', '？', '…', '·', '「', '」', '『', '』', '“', '”', '‘', '’',
    '（', '）', '(', ')', '【', '】', '《', '》', '〈', '〉', '[', ']', '—', '-', '"', '\'',
];

/// Whether a fragment contains at least one Han character (BMP CJK
/// unified, extension A, compatibility, and supplementary planes).
fn has_han(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(
            ch as u32,
            0x3400..=0x4DBF
                | 0x4E00..=0x9FFF
                | 0xF900..=0xFAFF
                | 0x20000..=0x2FA1F
        )
    })
}

/// Markup sequences that must never survive into `ngseg` input. The
/// stripper consumes balanced markup; this is the safety net for source
/// pages with unbalanced braces, which would otherwise leak `}}`-style
/// fragments into the corpus.
const MARKUP_RESIDUE: &[&str] = &["{{", "}}", "[[", "]]", "{|", "|}", "<ref"];

/// Splits cleaned text into one sentence per line. Lines are trimmed,
/// stripped of leading/trailing punctuation, filtered to fragments
/// containing at least one Han character, and freed of any
/// [`MARKUP_RESIDUE`].
pub fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut fragment = String::new();

    let flush = |fragment: &mut String, sentences: &mut Vec<String>| {
        let trimmed =
            fragment.trim_matches(|ch: char| ch.is_whitespace() || TRIM_PUNCT.contains(&ch));
        if has_han(trimmed) && !MARKUP_RESIDUE.iter().any(|marker| trimmed.contains(marker)) {
            sentences.push(trimmed.to_owned());
        }
        fragment.clear();
    };

    for ch in text.chars() {
        match ch {
            '\n' => flush(&mut fragment, &mut sentences),
            boundary if BOUNDARIES.contains(&boundary) => flush(&mut fragment, &mut sentences),
            _ => fragment.push(ch),
        }
    }
    flush(&mut fragment, &mut sentences);
    sentences
}

#[cfg(test)]
mod tests {
    use super::split_sentences;

    #[test]
    fn splits_on_boundary_punctuation_and_newlines() {
        assert_eq!(
            split_sentences("第一句。第二句！第三句？\n第四句；第五句…第六句"),
            ["第一句", "第二句", "第三句", "第四句", "第五句", "第六句"]
        );
    }

    #[test]
    fn runs_of_boundaries_emit_nothing() {
        assert_eq!(split_sentences("甲。。。乙"), ["甲", "乙"]);
    }

    #[test]
    fn drops_non_han_fragments_and_trims_punctuation() {
        assert_eq!(
            split_sentences("abc 123。　中文。，逗号开头，"),
            ["中文", "逗号开头"]
        );
    }

    #[test]
    fn keeps_interior_punctuation() {
        assert_eq!(split_sentences("甲，乙：丙。"), ["甲，乙：丙"]);
    }

    #[test]
    fn supplementary_plane_han_counts() {
        assert_eq!(split_sentences("𠀀𠀁。"), ["𠀀𠀁"]);
    }
}
