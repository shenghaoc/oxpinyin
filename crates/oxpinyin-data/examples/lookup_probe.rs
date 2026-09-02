//! Prints what the production dictionary answers for a few syllable
//! sequences over one system data directory — a debugging aid for the
//! same-data differentials.
//!
//! ```text
//! cargo run -p oxpinyin-data --example lookup_probe -- <system-dir> ni'hao shi jie
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use oxpinyin_core::{Dictionary, LanguageModel, PhraseToken, SyllableKey};
use oxpinyin_data::{BigramLanguageModel, SystemDbm, SystemDictionary};

fn main() {
    let mut args = std::env::args().skip(1);
    let dir: PathBuf = args.next().map(PathBuf::from).expect("system dir");
    let dict = SystemDictionary::open(&dir).expect("open");
    let lm = BigramLanguageModel::open(
        &dir.join(SystemDbm::Bigram.file_name()),
        Arc::clone(dict.libraries()),
    )
    .expect("lm");
    println!("items {} · total {}", dict.item_count(), lm.unigram_total());
    for spelling in args {
        let keys: Vec<SyllableKey> = spelling
            .split('\'')
            .filter_map(SyllableKey::from_text)
            .collect();
        let entries = dict.lookup(&keys).expect("lookup");
        let prefix = dict.phrase_prefix_exists(&keys).expect("probe");
        println!(
            "{spelling}: {} entries, prefix_exists={prefix}",
            entries.len()
        );
        for entry in entries.iter().take(12) {
            let token = entry.token();
            println!(
                "  {:#010x} {} poss={:?} unigram={:?} prons={:?}",
                token.value(),
                entry.text(),
                entry.pronunciation_possibility(),
                LanguageModel::unigram_freq(&lm, &PhraseToken::new(token.value()))
                    .ok()
                    .flatten(),
                dict.pronunciations(token.value())
            );
        }
    }
}
