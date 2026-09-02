//! Open-time and resident-memory profile of the production data readers.
//!
//! Measurement only — not part of the decode path. Drive it with a system
//! data directory for the compiled-in backend (a libpinyin install's
//! `data/` on Kyoto Cabinet and tkrzw, an `oxpinyin-datagen compile`
//! output anywhere):
//!
//! ```text
//! PINYIN_EXPORT_DIR=/opt/libpinyin-kc/lib/libpinyin/data \
//! cargo run -p oxpinyin-data --release --example open_profile
//! ```
//!
//! Reports wall time and, on Linux, the `/proc/self/status` counters
//! (`VmRSS`, `RssAnon`, `RssFile`, `VmHWM`) after each step: open the
//! dictionary (DBM handles + chunk mappings), open the language model and
//! punctuation table, then a batch of lookups. The split between
//! `RssAnon` and `RssFile` is the point: the readers hold file-backed
//! mappings and DBM pages, not reconstructed heap.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use oxpinyin_core::{Dictionary, LanguageModel, PhraseToken, SyllableKey};
use oxpinyin_data::{BigramLanguageModel, PunctTable, SystemDbm, SystemDictionary};

fn status_line() -> String {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return "(no /proc/self/status on this platform)".to_owned();
    };
    status
        .lines()
        .filter(|line| {
            ["VmRSS", "RssAnon", "RssFile", "RssShmem", "VmHWM"]
                .iter()
                .any(|key| line.starts_with(key))
        })
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn report(label: &str, prev: &mut Instant) {
    let elapsed = prev.elapsed();
    println!(
        "{label:<48} {:>9.3} ms   {}",
        elapsed.as_secs_f64() * 1e3,
        status_line()
    );
    *prev = Instant::now();
}

fn syllables(text: &str) -> Vec<SyllableKey> {
    text.split('\'')
        .filter_map(SyllableKey::from_text)
        .collect()
}

fn main() {
    let dir: PathBuf = std::env::var_os("PINYIN_EXPORT_DIR")
        .map_or_else(|| PathBuf::from("/tmp/oxpinyin-export"), PathBuf::from);
    println!("system dir: {}", dir.display());
    println!(
        "start                                           {:>9}      {}",
        "",
        status_line()
    );
    let mut prev = Instant::now();

    let dict = SystemDictionary::open(&dir).expect("dictionary opens");
    report("SystemDictionary::open (2 DBMs + chunk maps)", &mut prev);

    let mut lm = BigramLanguageModel::open(
        &dir.join(SystemDbm::Bigram.file_name()),
        Arc::clone(dict.libraries()),
    )
    .expect("language model opens");
    lm.set_lambda_from_table_conf(&dir.join("table.conf"));
    let punct = PunctTable::open_optional(&dir.join(SystemDbm::Punct.file_name()));
    report("BigramLanguageModel::open + PunctTable", &mut prev);

    println!(
        "  items {} · unigram total {} · punct open {}",
        dict.item_count(),
        lm.unigram_total(),
        punct.is_open()
    );
    report("item count (first offset-array tally)", &mut prev);

    let probes = [
        "ni'hao",
        "zhong'guo",
        "wo'men",
        "shi'jie",
        "xi'an",
        "bei'jing",
        "ren'min",
        "ke'yi",
    ];
    let mut hits = 0_usize;
    let mut scored: i64 = 0;
    for _ in 0..100 {
        for probe in probes {
            let keys = syllables(probe);
            let entries = dict.lookup(&keys).expect("lookup");
            hits += entries.len();
            if let Some(first) = entries.first() {
                scored = scored.wrapping_add(
                    lm.score(&[PhraseToken::new(0x0100_0001)], &first.token(), 0)
                        .expect("score"),
                );
            }
            let _ = dict.phrase_prefix_exists(&keys[..1]).expect("probe");
        }
    }
    report("800 lookups + prefix probes + scores", &mut prev);
    println!("  hits {hits} · score checksum {scored}");
}
