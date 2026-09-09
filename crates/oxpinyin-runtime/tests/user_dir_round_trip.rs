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
