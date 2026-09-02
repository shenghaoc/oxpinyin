//! KMM text format: `export`/`import` and `k_mixture_model_to_interpolation`.
//!
//! In this Rust re-expression the persistent model *is* the textual export
//! format (upstream's `export_k_mixture_model` output), replacing the
//! `FlexibleBigram` DBM. So [`export`] and [`import`] are inverse
//! canonicalisers, and the pipeline's `.db` files are these text files. The
//! format is byte-identical to upstream (`export_k_mixture_model.cpp:35-110`
//! / `import_k_mixture_model.cpp`), so the differential compares bytes.
//!
//! [`kmm_text_to_interpolation`] is the streaming
//! `k_mixture_model_to_interpolation` transform
//! (`k_mixture_model_to_interpolation.cpp:59-217`): KMM text in,
//! `interpolation2.text` out, dropping `sentence_start` and zero-freq
//! unigrams.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::error::KmmError;
use crate::model::{ArrayItem, KMixtureModel, SENTENCE_START};

const KMM_MODEL_NAME: &str = "k mixture model";

/// Renders a model in the KMM text format (`export_k_mixture_model`).
///
/// Records whose phrase text does not resolve are skipped, matching
/// `taglib_token_to_string` returning `NULL` (`:59-63`, `:86-96`). Walks
/// are token-ascending (the ordered maps), matching `get_all_items` /
/// `retrieve_all`.
#[must_use]
pub fn export(model: &KMixtureModel) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "\\data model \"{KMM_MODEL_NAME}\" count {} N {} total_freq {}",
        model.wc, model.n, model.total_freq
    );

    out.push_str("\\1-gram\n");
    for (&token, gram) in &model.grams {
        if let Some(phrase) = model.text(token) {
            let _ = writeln!(
                out,
                "\\item {token} {phrase} count {} freq {}",
                gram.header_wc, gram.header_freq
            );
        }
    }

    out.push_str("\\2-gram\n");
    for (&token1, gram) in &model.grams {
        let Some(word1) = model.text(token1) else {
            continue;
        };
        for (&token2, item) in &gram.items {
            let Some(word2) = model.text(token2) else {
                continue;
            };
            let _ = writeln!(
                out,
                "\\item {token1} {word1} {token2} {word2} count {} T {} N_n_0 {} n_1 {} Mr {}",
                item.wc, item.wc, item.n_n_0, item.n_1, item.mr
            );
        }
    }

    out.push_str("\\end\n");
    out
}

/// Parses the KMM text format back into a model (inverse of [`export`]).
///
/// # Errors
///
/// Returns [`KmmError::Malformed`] when the header or an `\item` line does
/// not match the KMM grammar.
pub fn import(text: &str) -> Result<KMixtureModel, KmmError> {
    let mut lines = text.lines();
    let header = lines.next().ok_or_else(|| KmmError::Malformed {
        detail: "empty KMM text".to_owned(),
    })?;
    let mut model = parse_header(header)?;

    let mut section = Section::None;
    for line in lines {
        match line {
            "\\1-gram" => section = Section::Unigram,
            "\\2-gram" => section = Section::Bigram,
            "\\end" => break,
            _ if line.starts_with("\\item") => match section {
                Section::Unigram => parse_unigram_item(&mut model, line)?,
                Section::Bigram => parse_bigram_item(&mut model, line)?,
                Section::None => {
                    return Err(KmmError::Malformed {
                        detail: format!("\\item before a section header: {line:?}"),
                    });
                }
            },
            "" => {}
            _ => {
                return Err(KmmError::Malformed {
                    detail: format!("unexpected line: {line:?}"),
                });
            }
        }
    }
    Ok(model)
}

#[derive(Clone, Copy)]
enum Section {
    None,
    Unigram,
    Bigram,
}

/// Parses `\data model "k mixture model" count <wc> N <n> total_freq <tf>`.
fn parse_header(line: &str) -> Result<KMixtureModel, KmmError> {
    // Split off the quoted model name first.
    let rest = line
        .strip_prefix("\\data model \"")
        .ok_or_else(|| malformed(line, "not a KMM \\data header"))?;
    let (name, tail) = rest
        .split_once('"')
        .ok_or_else(|| malformed(line, "unterminated model name"))?;
    if name != KMM_MODEL_NAME {
        return Err(malformed(line, "expected the k mixture model header"));
    }
    let fields: Vec<&str> = tail.split_whitespace().collect();
    // count <wc> N <n> total_freq <tf>
    if fields.len() != 6 || fields[0] != "count" || fields[2] != "N" || fields[4] != "total_freq" {
        return Err(malformed(line, "malformed KMM header fields"));
    }
    let mut model = KMixtureModel::new();
    model.wc = parse_u32(fields[1], line)?;
    model.n = parse_u32(fields[3], line)?;
    model.total_freq = parse_u32(fields[5], line)?;
    Ok(model)
}

/// Parses `\item <token> <phrase> count <wc> freq <freq>`.
fn parse_unigram_item(model: &mut KMixtureModel, line: &str) -> Result<(), KmmError> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() != 7 || fields[3] != "count" || fields[5] != "freq" {
        return Err(malformed(line, "malformed \\1-gram \\item"));
    }
    let token = parse_u32(fields[1], line)?;
    let phrase = fields[2];
    let header_wc = parse_u32(fields[4], line)?;
    let header_freq = parse_u32(fields[6], line)?;
    model.record_text(token, phrase);
    let gram = model.grams.entry(token).or_default();
    gram.header_wc = header_wc;
    gram.header_freq = header_freq;
    Ok(())
}

/// Parses `\item <t1> <w1> <t2> <w2> count <wc> T <t> N_n_0 <n> n_1 <n> Mr <m>`.
fn parse_bigram_item(model: &mut KMixtureModel, line: &str) -> Result<(), KmmError> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() != 15
        || fields[5] != "count"
        || fields[7] != "T"
        || fields[9] != "N_n_0"
        || fields[11] != "n_1"
        || fields[13] != "Mr"
    {
        return Err(malformed(line, "malformed \\2-gram \\item"));
    }
    let token1 = parse_u32(fields[1], line)?;
    let word1 = fields[2];
    let token2 = parse_u32(fields[3], line)?;
    let word2 = fields[4];
    let item = ArrayItem {
        wc: parse_u32(fields[6], line)?,
        n_n_0: parse_u32(fields[10], line)?,
        n_1: parse_u32(fields[12], line)?,
        mr: parse_u32(fields[14], line)?,
    };
    model.record_text(token1, word1);
    model.record_text(token2, word2);
    let gram = model.grams.entry(token1).or_default();
    gram.items.insert(token2, item);
    Ok(())
}

/// Streaming `k_mixture_model_to_interpolation`: KMM text → interpolation
/// text. The `\1-gram` `count` becomes the KMM `freq` field (the unigram
/// frequency), dropping `sentence_start` and zero-freq rows; the `\2-gram`
/// `count` is the KMM pair `count` (`m_WC`).
///
/// # Errors
///
/// Returns [`KmmError::Malformed`] when the input is not KMM text.
pub fn kmm_text_to_interpolation(text: &str) -> Result<String, KmmError> {
    let mut lines = text.lines();
    let header = lines.next().ok_or_else(|| KmmError::Malformed {
        detail: "empty file input".to_owned(),
    })?;
    // Validate the KMM header (parse_header rejects a non-KMM file).
    parse_header(header)?;

    let mut out = String::new();
    out.push_str("\\data model interpolation\n");
    let mut section = Section::None;
    for line in lines {
        match line {
            "\\1-gram" => {
                out.push_str("\\1-gram\n");
                section = Section::Unigram;
            }
            "\\2-gram" => {
                out.push_str("\\2-gram\n");
                section = Section::Bigram;
            }
            "\\end" => break,
            _ if line.starts_with("\\item") => match section {
                Section::Unigram => {
                    let fields: Vec<&str> = line.split_whitespace().collect();
                    if fields.len() != 7 || fields[3] != "count" || fields[5] != "freq" {
                        return Err(malformed(line, "malformed \\1-gram \\item"));
                    }
                    let token = parse_u32(fields[1], line)?;
                    let word = fields[2];
                    let freq = parse_u32(fields[6], line)?;
                    // Drop <start> and zero-freq unigrams.
                    if token != SENTENCE_START && freq != 0 {
                        let _ = writeln!(out, "\\item {token} {word} count {freq}");
                    }
                }
                Section::Bigram => {
                    let fields: Vec<&str> = line.split_whitespace().collect();
                    if fields.len() != 15
                        || fields[5] != "count"
                        || fields[7] != "T"
                        || fields[9] != "N_n_0"
                        || fields[11] != "n_1"
                        || fields[13] != "Mr"
                    {
                        return Err(malformed(line, "malformed \\2-gram \\item"));
                    }
                    let token1 = parse_u32(fields[1], line)?;
                    let word1 = fields[2];
                    let token2 = parse_u32(fields[3], line)?;
                    let word2 = fields[4];
                    let count = parse_u32(fields[6], line)?;
                    let _ = writeln!(
                        out,
                        "\\item {token1} {word1} {token2} {word2} count {count}"
                    );
                }
                Section::None => {
                    return Err(malformed(line, "\\item before a section header"));
                }
            },
            "" => {}
            _ => return Err(malformed(line, "unexpected line")),
        }
    }
    out.push_str("\\end\n");
    Ok(out)
}

fn parse_u32(field: &str, line: &str) -> Result<u32, KmmError> {
    field
        .parse::<u32>()
        .map_err(|_| malformed(line, "expected an unsigned integer"))
}

fn malformed(line: &str, why: &str) -> KmmError {
    KmmError::Malformed {
        detail: format!("{why}: {line:?}"),
    }
}

/// Canonicalises a KMM text file: `import` then `export`. Used by the
/// `export`/`import` CLI subcommands, which in the text-native model are
/// the same round-trip (upstream's `.db`↔text directions collapse).
///
/// # Errors
///
/// Returns [`KmmError::Malformed`] when the input is not KMM text.
pub fn canonicalize(text: &str) -> Result<String, KmmError> {
    Ok(export(&import(text)?))
}

/// Merges the `texts` columns of two models (used by `merge`).
pub(crate) fn merge_texts(into: &mut BTreeMap<u32, String>, from: &BTreeMap<u32, String>) {
    for (token, text) in from {
        into.entry(*token).or_insert_with(|| text.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::{export, import, kmm_text_to_interpolation};
    use crate::generate::GenerateParams;
    use crate::model::KMixtureModel;

    fn sample_model() -> KMixtureModel {
        let doc = "10 甲\n20 乙\n10 甲\n20 乙\n";
        let mut model = KMixtureModel::new();
        model
            .add_document(doc, GenerateParams::default())
            .expect("count");
        model
    }

    #[test]
    fn export_grammar_matches_upstream() {
        let model = sample_model();
        let text = export(&model);
        assert_eq!(
            text,
            "\\data model \"k mixture model\" count 4 N 1 total_freq 4\n\
             \\1-gram\n\
             \\item 1 <start> count 1 freq 0\n\
             \\item 10 甲 count 2 freq 2\n\
             \\item 20 乙 count 1 freq 2\n\
             \\2-gram\n\
             \\item 1 <start> 10 甲 count 1 T 1 N_n_0 1 n_1 1 Mr 1\n\
             \\item 10 甲 20 乙 count 2 T 2 N_n_0 1 n_1 0 Mr 2\n\
             \\item 20 乙 10 甲 count 1 T 1 N_n_0 1 n_1 1 Mr 1\n\
             \\end\n"
        );
    }

    #[test]
    fn export_import_round_trips() {
        let model = sample_model();
        let text = export(&model);
        let reparsed = import(&text).expect("import");
        assert_eq!(reparsed, model);
        assert_eq!(export(&reparsed), text);
    }

    #[test]
    fn to_interpolation_drops_start_and_zero_freq() {
        let model = sample_model();
        let kmm = export(&model);
        let interp = kmm_text_to_interpolation(&kmm).expect("convert");
        assert_eq!(
            interp,
            "\\data model interpolation\n\
             \\1-gram\n\
             \\item 10 甲 count 2\n\
             \\item 20 乙 count 2\n\
             \\2-gram\n\
             \\item 1 <start> 10 甲 count 1\n\
             \\item 10 甲 20 乙 count 2\n\
             \\item 20 乙 10 甲 count 1\n\
             \\end\n"
        );
    }

    #[test]
    fn import_rejects_a_non_kmm_header() {
        let err = import("\\data model interpolation\n\\end\n").unwrap_err();
        assert!(matches!(err, crate::error::KmmError::Malformed { .. }));
    }
}
