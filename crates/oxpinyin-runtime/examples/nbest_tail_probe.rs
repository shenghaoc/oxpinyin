//! Diagnostic: why the n-best trellis never produces the imported
//! user-phrase tail (residue A, `docs/findings/probe-coverage-abi.md`).
//!
//! `cargo run -p oxpinyin-runtime --example nbest_tail_probe -- <system_dir> <user_dir>`
//!
//! Reproduces the ABI probe's import state through the runtime's own
//! seams — 你好/5, 你好世界/9, 测试/3 into the user dictionary of a fresh
//! `user_dir` — then prints, for `nihaoshijie`:
//!
//! - the user tokens the import created and their unigram frequencies;
//! - what the merged dictionary answers for the whole-input key path
//!   `ni'hao'shi'jie` and whether the widen probe lets the span reach it;
//! - the n-best step costs from `sentence_start` for every token the
//!   whole-input path spells, next to the same costs for the system
//!   你好 token — the expansion gate `crate::nbest::expand_entry` reads;
//! - the sentence rows the session decodes, with their trellis costs in
//!   the core's millibit scale and in nats (`cost / 1000 · ln 2`).
//!
//! Prints and exits 0; nothing here asserts a class.

use std::path::Path;

use oxpinyin_core::{Dictionary, LanguageModel, OptionBits, PhraseToken, SyllableKey};
use oxpinyin_engine::{CandidateKind, EmptyConfigSource};
use oxpinyin_runtime::Runtime;

/// `novel_types.h`'s `sentence_start` — the trellis seed.
const SENTENCE_START: u32 = 1;
/// The ABI probe's parity word (`tools/bisection/abi-probe-diff.c`).
const PARITY_WORD: u32 = 0x18a;

const IMPORTS: [(&str, &str, u64); 3] = [
    ("你好", "ni'hao", 5),
    ("你好世界", "ni'hao'shi'jie", 9),
    ("测试", "ce'shi", 3),
];

fn nats(cost: i64) -> f64 {
    cost as f64 / 1000.0 * std::f64::consts::LN_2
}

fn fmt_cost(cost: Option<i64>) -> String {
    cost.map_or_else(
        || "None".to_owned(),
        |c| format!("Some({c} = {:.6} nats)", nats(c)),
    )
}

fn keys(pinyin: &str) -> Vec<SyllableKey> {
    pinyin
        .split('\'')
        .map(|syllable| {
            SyllableKey::from_text(syllable)
                .unwrap_or_else(|| panic!("`{syllable}` is not a frozen syllable"))
        })
        .collect()
}

fn key_text(path: &[SyllableKey]) -> String {
    path.iter()
        .map(|key| key.text().to_owned())
        .collect::<Vec<_>>()
        .join("'")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(system_dir), Some(user_dir)) = (args.get(1), args.get(2)) else {
        eprintln!("usage: nbest_tail_probe <system_dir> <user_dir>");
        std::process::exit(2);
    };
    let runtime = match Runtime::open(Path::new(system_dir), Some(Path::new(user_dir))) {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("open FAILED: {error}");
            std::process::exit(1);
        }
    };
    let Some(mut store) = runtime.user_store() else {
        eprintln!("the user dir did not open as a user store");
        std::process::exit(1);
    };
    let dict = runtime.dict();
    let lm = runtime.lm();

    println!("=== import ===");
    let mut user_tokens: Vec<(String, u32)> = Vec::new();
    for (phrase, pinyin, count) in IMPORTS {
        let key_path: Vec<u16> = keys(pinyin)
            .iter()
            .map(|key| u16::try_from(key.index()).expect("syllable indices fit u16"))
            .collect();
        match store.add_phrase(phrase, &key_path, Some(count)) {
            Ok(token) => {
                println!("add({phrase}/{pinyin}/{count}) = token 0x{token:08x}");
                user_tokens.push((phrase.to_owned(), token));
            }
            Err(error) => println!("add({phrase}/{pinyin}/{count}) FAILED: {error}"),
        }
    }
    match store.save() {
        Ok(saved) => println!("save = {saved}"),
        Err(error) => println!("save FAILED: {error}"),
    }
    println!(
        "has_real_unigrams = {}; unigram_total = {:?}",
        lm.has_real_unigrams(),
        lm.unigram_total().ok().flatten()
    );
    for (phrase, token) in &user_tokens {
        println!(
            "user token 0x{token:08x} ({phrase}): unigram_freq = {:?}; tokens_for_text = {:?}",
            lm.unigram_freq(&PhraseToken::new(*token)).ok().flatten(),
            dict.tokens_for_text(phrase)
                .iter()
                .map(|t| format!("0x{:08x}", t.value()))
                .collect::<Vec<_>>()
        );
    }

    println!("=== dictionary over the whole-input path ===");
    let start = PhraseToken::new(SENTENCE_START);
    let full = keys("ni'hao'shi'jie");
    for prefix_len in 1..=full.len() {
        let path = &full[..prefix_len];
        let extends = dict.phrase_prefix_exists(path).unwrap_or(false);
        let entries = dict.lookup(path).unwrap_or_default();
        println!(
            "lookup({}) = {} entries; phrase_prefix_exists = {extends}",
            key_text(path),
            entries.len()
        );
        // The whole-input span and the two-key span carry the tokens the
        // tails are made of; the others are context.
        if prefix_len == full.len() || prefix_len == 2 {
            for entry in &entries {
                let token = entry.token();
                let lib = (token.value() >> 24) & 0xff;
                if lib != 7 && prefix_len == 2 && entry.text() != "你好" {
                    continue;
                }
                let step = lm.nbest_step_costs(&start, &token).ok();
                println!(
                    "  entry 0x{:08x} lib={lib} text={} pronunciation={:?} unigram_freq={:?} \
                     step_costs(sentence_start -> token): unigram={} blended={}",
                    token.value(),
                    entry.text(),
                    entry.pronunciation_possibility(),
                    lm.unigram_freq(&token).ok().flatten(),
                    fmt_cost(step.as_ref().and_then(|s| s.unigram)),
                    fmt_cost(step.as_ref().and_then(|s| s.blended)),
                );
            }
        }
    }
    // The second step of the two-token path the port does produce.
    let tail = keys("shi'jie");
    let heads: Vec<PhraseToken> = dict
        .lookup(&full[..2])
        .unwrap_or_default()
        .iter()
        .filter(|entry| entry.text() == "你好")
        .map(|entry| entry.token())
        .collect();
    for entry in dict.lookup(&tail).unwrap_or_default() {
        if entry.text() != "世界" && entry.text() != "时节" {
            continue;
        }
        for head in &heads {
            let step = lm.nbest_step_costs(head, &entry.token()).ok();
            println!(
                "step_costs(0x{:08x} 你好 -> 0x{:08x} {}): unigram={} blended={}",
                head.value(),
                entry.token().value(),
                entry.text(),
                fmt_cost(step.as_ref().and_then(|s| s.unigram)),
                fmt_cost(step.as_ref().and_then(|s| s.blended)),
            );
        }
    }

    println!("=== session rows ===");
    let mut session = match runtime.new_session(&EmptyConfigSource) {
        Ok(session) => session,
        Err(error) => {
            eprintln!("session FAILED: {error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = session.set_options(OptionBits::from_bits(PARITY_WORD)) {
        eprintln!("set_options FAILED: {error}");
        std::process::exit(1);
    }
    if let Err(error) = session.type_pinyin("nihaoshijie") {
        eprintln!("type_pinyin FAILED: {error}");
        std::process::exit(1);
    }
    match session.guess_sentence() {
        Ok(ran) => println!("guess_sentence = {ran}"),
        Err(error) => {
            eprintln!("guess_sentence FAILED: {error}");
            std::process::exit(1);
        }
    }
    for index in 0..3u8 {
        println!(
            "sentence_text({index}) = {:?}",
            session.sentence_text(index)
        );
    }
    match session.candidates_at(0) {
        Ok(list) => {
            for candidate in list.iter() {
                if candidate.kind() != CandidateKind::Sentence {
                    continue;
                }
                println!(
                    "row rank={:?} text={} keys={} cost={} nats={:.9}",
                    candidate.nbest_row(),
                    candidate.text(),
                    candidate.consumed_keys(),
                    candidate.cost(),
                    nats(candidate.cost()),
                );
            }
            println!("candidates n = {}", list.len());
        }
        Err(error) => eprintln!("candidates FAILED: {error}"),
    }
}
