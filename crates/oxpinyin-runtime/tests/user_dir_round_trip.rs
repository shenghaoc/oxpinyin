//! The user-dir round trip with the pin-built libpinyin (drop-in task
//! 9's differential, Rust half).
//!
//! Driven by `tools/oracle/user-dir-round-trip.sh`: the pin trains a
//! profile, this test opens it through the production [`Runtime`] and
//! saves it back **in place** — a pure load→save of the pin's own data,
//! no oxpinyin training, so no decode or training divergence ((a) the
//! n-best trellis, (b) the pin's stale-pinyin export buffer) can enter.
//! The script then has the *pin* render the kept original and the
//! rewritten profile and diffs the two: same data, same renderer, and
//! the only variable is oxpinyin's read+write. A non-empty diff is a
//! file-I/O defect; an empty diff is the seamless-swap claim measured.
//!
//! This test additionally pins what the script cannot see: that the
//! save wrote the pin's own file names (never a `user_store.<ext>`
//! scratch the pin could not open), that the profile re-reads
//! value-stably, and that the bigram grams survived byte-for-byte.
#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use oxpinyin_runtime::Runtime;
use oxpinyin_user::persistence;
use oxpinyin_user::{SystemVersions, system_originals};

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("user-dir round trip: ${name} is required"))
}

#[test]
#[ignore = "needs the pin-built oracle; run via tools/oracle/user-dir-round-trip.sh"]
fn oxpinyin_trains_and_saves_its_own_profile() {
    // The reverse direction's Rust half: a profile oxpinyin created from
    // nothing, through the production Runtime. The script then has the
    // *pin* open and render this directory (Phase D) — the failure it
    // exists to catch is a file oxpinyin wrote that the pin cannot
    // parse, which no oxpinyin-only assertion can see.
    let system = env("OX_SYSTEM_DIR");
    let owned = env("OX_OWNED_DIR");

    let runtime = Runtime::open(Path::new(&system), Some(Path::new(&owned)))
        .expect("the runtime opens the fresh dir");
    let mut store = runtime.user_store().expect("a fresh profile backs a store");

    // Train through the engine session — choose, then a sentence train —
    // so the profile carries a bigram. The token set is whatever this
    // runtime decodes; the pin's Phase-D check is "can it read and
    // render", not "did it train identically" (the n-best trellis is a
    // class-(a) divergence).
    let inputs: Vec<String> = env("OX_INPUTS")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let mut expected_phrases: Vec<String> = Vec::with_capacity(inputs.len());
    for input in &inputs {
        // One session per input, the way a committed sentence clears the
        // composition: accumulating two inputs on one session would build
        // a compound phrase no consumer sends.
        let session = &mut runtime
            .new_session(&oxpinyin_engine::EmptyConfigSource)
            .expect("a session allocates");
        session.type_pinyin(input).expect("input processes");
        session.guess_sentence().expect("guess");
        session.select(0).expect("candidate 0 selects");
        session.train(&mut store).expect("train");
        // ibus's remember-every-input path (§6): the committed text also
        // becomes a user phrase through the composition's own keys — the
        // same shape `pinyin_remember_user_input` drives — which is what
        // makes user.bin and the two index trees non-empty. Phase D
        // counts the phrase rows the pin renders off the back of this.
        // Remember-input preparation is part of what Phase D validates:
        // a failed composition read or an out-of-range key index fails
        // the test rather than silently skipping the phrase (a skip
        // would leave user.bin unexercised and the phase green for
        // nothing).
        let committed = session.preedit().text().to_owned();
        assert!(!committed.is_empty(), "a committed sentence exists");
        let keys: Vec<oxpinyin_user::PinyinKey> = session
            .composition_keys()
            .expect("composition keys read")
            .into_iter()
            .map(|key| {
                u16::try_from(key.index()).expect("the syllable id fits the store's PinyinKey")
            })
            .collect();
        store
            .add_phrase(&committed, &keys, None)
            .expect("the committed sentence is remembered");
        expected_phrases.push(committed);
    }

    // Hand the remembered phrase set to the driving script: Phase D
    // diffs the pin's rendered rows against exactly these, so a profile
    // that lost all but one row fails instead of reporting READABLE.
    if let Ok(path) = std::env::var("OX_EXPECTED_PHRASES") {
        std::fs::write(path, format!("{}\n", expected_phrases.join("\n")))
            .expect("expected-phrases file writes");
    }

    // Mark and save: the dirty gate is the pin's (§4), so the forced
    // write is explicit here.
    store.mark_modified();
    assert!(
        store.save().expect("save succeeds"),
        "the dirty save writes"
    );

    for name in [
        "user_bigram.db",
        "user_pinyin_index.bin",
        "user_phrase_index.bin",
        "user.bin",
        "user.conf",
    ] {
        // The name the save used: libpinyin's own on the drop-in set,
        // the backend-named twin on redb/LMDB. One of the two must
        // exist for every file in the pin's inventory.
        let stem = name.trim_end_matches(".db").trim_end_matches(".bin");
        let file = Path::new(&owned).join(name);
        let alt = Path::new(&owned).join(format!("{stem}.{}", oxpinyin_data::DEFAULT_STORE_EXT));
        assert!(
            file.exists() || alt.exists(),
            "the save did not write {name} (or its backend-named twin)"
        );
    }
}

#[test]
#[ignore = "needs the pin-built oracle; run via tools/oracle/user-dir-round-trip.sh"]
fn a_pin_profile_loads_and_saves_back_in_place() {
    let system = env("OX_SYSTEM_DIR");
    let pin_dir = env("OX_PIN_DIR");

    // The production open path: Runtime::open runs check_format over
    // user.conf, loads the bigram hash, the USER_FILE chunk stores and
    // replays the .dbin logs onto the system originals — exactly what
    // `pinyin_init(systemdir, userdir)` does upstream.
    let runtime = Runtime::open(Path::new(&system), Some(Path::new(&pin_dir)))
        .expect("the runtime opens the pin's system dir and profile");
    let mut store = runtime
        .user_store()
        .expect("the profile backs a user store");

    // A load alone is not "modified" (§4); the round trip forces the
    // writer over the freshly-loaded values.
    store.mark_modified();
    assert!(
        store.save().expect("save succeeds"),
        "the forced save must write"
    );

    // The pin's own file names are what landed — never a
    // `user_store.<ext>` scratch the pin could never open.
    for name in [
        "user_bigram.db",
        "user_pinyin_index.bin",
        "user_phrase_index.bin",
        "user.bin",
        "user.conf",
    ] {
        assert!(
            Path::new(&pin_dir).join(name).exists(),
            "the save did not write {name}"
        );
    }

    // The rewrite re-reads value-stably: same bigram grams, same user
    // items. This is the load→save→load fixed point; the script's
    // pin-rendered diff is the cross-library authority on top of it.
    let versions = SystemVersions::from_table_conf(
        &fs::read_to_string(Path::new(&system).join("table.conf")).unwrap_or_default(),
    );
    let originals: BTreeMap<u8, _> = system_originals(runtime.dict().system().libraries());
    let first =
        persistence::load(Path::new(&pin_dir), &originals, &versions).expect("the rewrite loads");
    assert!(
        !first.state.bigram.is_empty(),
        "the pin profile carried grams that did not survive the round trip"
    );
    persistence::save(
        Path::new(&pin_dir),
        &first.state,
        &originals,
        &versions,
        first.open_counter,
    )
    .expect("the rewrite saves again");
    let second = persistence::load(Path::new(&pin_dir), &originals, &versions)
        .expect("the second rewrite loads");
    assert_eq!(
        second.state, first.state,
        "load→save is not a value fixed point on a pin profile"
    );
}
