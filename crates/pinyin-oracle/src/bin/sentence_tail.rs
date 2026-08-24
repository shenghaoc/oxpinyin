//! Enumerate the W14 sentence-surface residual — the inputs where the port's
//! `guess_sentence` surface diverges from the pinned oracle's, per
//! `fixtures/w4/oracle-sentence-surface.txt`.
//!
//! Read-only, single-threaded, no oracle FFI. The measurement itself lives in
//! `pinyin_oracle::sentence_tail`, shared with the `sentence_surface_parity`
//! integration test so the printed numbers and the asserted numbers cannot
//! drift. This binary opens the session from the environment and prints; the
//! test asserts.
//!
//! Reports agreement at three named strictnesses over the 496 comparable
//! inputs — **1-best** (rank-0 decoded), n-best **distinct-set** (order- and
//! duplicate-insensitive), and n-best **ordered** (= first-6 candidate rows)
//! — then every diverging input. See `docs/findings/sentence-surface.md` §12:
//! the `488 / 385 / 379` there are those three strictnesses, not one measure;
//! the ordered residual is 117 (all trellis-side, 0 candidate leaks). The
//! sentence surface is a declared permanent Stage-1 residual, so this reports
//! it — it does not assert a pin.
//!
//! ```bash
//! PINYIN_EXPORT_DIR=/tmp/oxpinyin-export \
//! PINYIN_MODEL_DIR=<complete extracted model20> \
//! cargo run -p pinyin-oracle --release --bin sentence-tail
//! ```

use std::process::ExitCode;

use pinyin_oracle::sentence_tail::{self, SentenceTailReport};

fn run() -> Result<(), String> {
    let Some(mut session) = sentence_tail::open_session_from_env()? else {
        return Err(
            "exported tables or model cache absent; set PINYIN_EXPORT_DIR to the \
                    exported redb and PINYIN_MODEL_DIR to a complete extracted model20 dir"
                .to_owned(),
        );
    };
    let report = sentence_tail::measure(&mut session, &sentence_tail::repo_root())?;
    print_report(&report);
    Ok(())
}

fn print_report(report: &SentenceTailReport) {
    let n = report.comparable;
    println!("== W14 sentence-surface residual (port vs pinned oracle fixture) ==");
    println!("comparable inputs           {n}");
    println!("guessed disagreements       {}", report.guessed_disagree);
    println!(
        "row-0 (decoded 1-best)      {} / {n}  ({} miss)",
        report.row0_match,
        n - report.row0_match
    );
    println!(
        "n-best list, ordered        {} / {n}  ({} miss: {} order-only, {} set-diff)",
        report.list_ordered_match,
        n - report.list_ordered_match,
        report.list_order_only,
        report.list_set_diff,
    );
    println!(
        "n-best list, distinct-set   {} / {n}  ({} of the ordered misses name the same sentences)",
        report.distinct_set_match(),
        report.list_distinct_extra,
    );
    println!(
        "  ordered miss first at     row0 {}  row1 {}  row2 {}",
        report.list_diff_at_row0, report.list_diff_at_row1, report.list_diff_at_row2,
    );
    println!(
        "first-6 candidate rows      {} / {n}  ({} miss: {} order-only, {} set-diff)",
        report.rows_match,
        n - report.rows_match,
        report.rows_order_only,
        report.rows_set_diff,
    );
    println!(
        "  rows miss, sentence-only  {}   (only the n/* rows differ; phrase order intact)",
        report.rows_sentence_only
    );
    println!(
        "  rows miss, phrase-window  {}   (phrase slice shifts as the NBEST prefix length differs)",
        report.rows_phrase_window
    );

    println!("\n== row-0 misses ({}) ==", report.row0_misses.len());
    for row in &report.row0_misses {
        println!("{row}");
    }
    println!("\n== full-list diffs ({}) ==", report.list_diffs.len());
    for row in &report.list_diffs {
        println!("{row}");
    }
    println!("\n== first-6 row diffs ({}) ==", report.rows_diffs.len());
    for row in &report.rows_diffs {
        println!("{row}");
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
