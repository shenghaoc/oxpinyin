//! The benchmark as a process and against real libraries.
//!
//! The first group needs no libpinyin and runs everywhere: any failure must be a
//! non-zero exit with nothing on stdout, never an empty or partial result a
//! caller could mistake for a measurement. The second group drives real
//! libraries and is `#[ignore]`d with the inputs it needs; per
//! `docs/testing/README.md` such tests **fail** on a missing input, they never
//! skip. Run them with `--ignored --test-threads=1` (libpinyin keeps
//! process-global state and is not thread-safe).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Mutex;
use std::time::Duration;

use oxpinyin_initbench::child::{self, ChildOptions};
use oxpinyin_initbench::ffi::Native;
use oxpinyin_initbench::fixtures;
use oxpinyin_initbench::memory::host_probe;
use oxpinyin_initbench::observe::{Observation, Verdict, compare, stable_files};
use oxpinyin_initbench::orchestrate::{CacheState, Plan, Record, SubjectSpec, run};
use oxpinyin_initbench::profile::{MANIFEST_FILE, Manifest, USER_SUBDIR, WITNESS_FILE};
use oxpinyin_initbench::sample::{SampleKind, StopPoint};
use oxpinyin_initbench::workload::{Bind, Facade};

const BIN: &str = env!("CARGO_BIN_EXE_initbench");

static ONE_LIBRARY_AT_A_TIME: Mutex<()> = Mutex::new(());

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("initbench-it-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

fn bin(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .output()
        .expect("the binary starts")
}

fn child_args<'a>(lib: &'a str, system: &'a str, user: &'a str) -> Vec<&'a str> {
    vec![
        "child",
        "--facade",
        "pinyin",
        "--lib",
        lib,
        "--system-dir",
        system,
        "--user-dir",
        user,
    ]
}

fn assert_refused(output: &Output) -> String {
    assert!(!output.status.success(), "the binary reported success");
    assert!(
        output.stdout.is_empty(),
        "a failed run printed to stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_missing_library_is_refused_with_nothing_on_stdout() {
    let dir = scratch("missing");
    let user = dir.join("user");
    fs::create_dir_all(&user).expect("user dir");
    let stderr = assert_refused(&bin(&child_args(
        dir.join("no-such.so").to_str().expect("utf-8"),
        dir.to_str().expect("utf-8"),
        user.to_str().expect("utf-8"),
    )));
    // Where there is no memory probe (anything but Linux and macOS) the child
    // refuses before it reaches the library; that is the documented refusal.
    let expected = if cfg!(any(target_os = "linux", target_os = "macos")) {
        "cannot load"
    } else {
        "unsupported on this host"
    };
    assert!(
        stderr.contains("initbench:") && stderr.contains(expected),
        "{stderr}"
    );
}

#[test]
fn a_file_that_is_not_a_shared_object_is_refused() {
    let dir = scratch("notlib");
    let fake = dir.join("libpinyin.so.15");
    fs::write(&fake, b"not an ELF file").expect("fake library");
    let user = dir.join("user");
    fs::create_dir_all(&user).expect("user dir");
    assert_refused(&bin(&child_args(
        fake.to_str().expect("utf-8"),
        dir.to_str().expect("utf-8"),
        user.to_str().expect("utf-8"),
    )));
}

/// A real, loadable shared object that is not a libpinyin lacks the facade's
/// symbols: the run must fail and name them, never proceed on what it has.
#[cfg(target_os = "linux")]
#[test]
fn a_real_library_without_the_symbols_is_refused_and_named() {
    let dir = scratch("nosymbols");
    let user = dir.join("user");
    fs::create_dir_all(&user).expect("user dir");
    for sub in ["child", "verify-child", "persist-child"] {
        let mut args = child_args(
            "libm.so.6",
            dir.to_str().expect("utf-8"),
            user.to_str().expect("utf-8"),
        );
        args[0] = sub;
        let stderr = assert_refused(&bin(&args));
        assert!(
            stderr.contains("lacks symbols") && stderr.contains("pinyin_init"),
            "{sub}: {stderr}"
        );
    }
}

#[test]
fn bad_invocations_are_refused() {
    assert_eq!(
        bin(&[]).status.code(),
        Some(2),
        "no subcommand is a usage error"
    );
    for args in [
        vec!["nonsense"],
        vec!["child"],
        vec!["child", "--lib"],
        vec!["child", "--bogus", "1"],
        vec![
            "child",
            "--facade",
            "klingon",
            "--lib",
            "x",
            "--system-dir",
            "y",
            "--user-dir",
            "z",
        ],
        vec![
            "child",
            "--facade",
            "pinyin",
            "--lib",
            "x",
            "--system-dir",
            "y",
            "--user-dir",
            "z",
            "--stop-after",
            "never",
        ],
        vec![
            "child",
            "--facade",
            "pinyin",
            "--lib",
            "x",
            "--system-dir",
            "y",
            "--user-dir",
            "z",
            "--kind",
            "both",
        ],
        vec![
            "child",
            "--facade",
            "pinyin",
            "--lib",
            "x",
            "--system-dir",
            "y",
            "--user-dir",
            "z",
            "--bind",
            "sometimes",
        ],
        vec!["run"],
        vec!["run", "--subject", "no-equals-sign"],
        vec!["report", "--input", "/nonexistent", "--reference", "pin"],
        vec!["profiles"],
    ] {
        let output = bin(&args);
        assert!(!output.status.success(), "{args:?} was accepted");
        assert!(output.stdout.is_empty(), "{args:?} printed to stdout");
    }
}

// ---- real libraries ------------------------------------------------------

fn env_path(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| {
        panic!("{name} must name the input this test needs (see docs/runbooks/initbench.md)")
    }))
}

struct Real {
    pin_pinyin: PathBuf,
    pin_zhuyin: PathBuf,
    ox_pinyin: PathBuf,
    ox_zhuyin: PathBuf,
    data: PathBuf,
}

fn real() -> Real {
    Real {
        pin_pinyin: env_path("OXPINYIN_INITBENCH_PIN_PINYIN"),
        pin_zhuyin: env_path("OXPINYIN_INITBENCH_PIN_ZHUYIN"),
        ox_pinyin: env_path("OXPINYIN_INITBENCH_OX_PINYIN"),
        ox_zhuyin: env_path("OXPINYIN_INITBENCH_OX_ZHUYIN"),
        data: env_path("OXPINYIN_INITBENCH_DATA"),
    }
}

fn empty_fixture(root: &Path) {
    fs::create_dir_all(root.join(USER_SUBDIR)).expect("fixture user dir");
    fs::write(root.join(MANIFEST_FILE), Manifest::default().to_text()).expect("manifest");
    fs::write(root.join(WITNESS_FILE), "").expect("witness");
}

/// A populated fixture made the way a frontend makes one: import, save, close.
fn modest_fixture(root: &Path, pin: &Path, data: &Path) {
    let user = root.join(USER_SUBDIR);
    fs::create_dir_all(&user).expect("fixture user dir");
    let mut native = Native::open(pin, Facade::Pinyin, Bind::Now).expect("the pin loads");
    fixtures::populate(&mut native, data, &user, &fixtures::modest_rows()).expect("populate");
    drop(native);
    let manifest = Manifest::of_directory(&user).expect("manifest");
    assert!(!manifest.files.is_empty(), "nothing was saved");
    fs::write(root.join(MANIFEST_FILE), manifest.to_text()).expect("manifest file");
    fs::write(root.join(WITNESS_FILE), "ceshi\ncece\n").expect("witness");
}

fn plan(
    real: &Real,
    work: &Path,
    profiles: Vec<PathBuf>,
    subjects: Vec<SubjectSpec>,
    facades: Vec<Facade>,
) -> Plan {
    Plan {
        probe: PathBuf::from(BIN),
        subjects,
        facades,
        system_dir: real.data.clone(),
        profiles,
        rounds: 3,
        warmup: 1,
        kind: SampleKind::Timing,
        cache: CacheState::Warm,
        cpu: None,
        bind: Bind::Now,
        work_dir: work.to_path_buf(),
        max_skew: Duration::from_secs(5),
        max_load: None,
        timeout: Duration::from_secs(120),
    }
}

fn spec(label: &str, pinyin: &Path, zhuyin: &Path) -> SubjectSpec {
    SubjectSpec {
        label: label.into(),
        pinyin: Some(pinyin.to_path_buf()),
        zhuyin: Some(zhuyin.to_path_buf()),
    }
}

fn collect(plan: &Plan) -> Vec<Record> {
    let mut records = Vec::new();
    run(plan, &mut |r: &Record| {
        records.push(r.clone());
        Ok(())
    })
    .expect("the run completes");
    records
}

/// The acceptance criterion for a ratio: every observation-phase verdict of the
/// shipping oxpinyin against the pin is `comparable`, on both facades and on a
/// fresh and a populated (saved and reopened) profile, and the save-and-reopen
/// scenario agrees. Hostile-input differences are reported separately and do not
/// gate this (no timing runs those inputs).
#[test]
#[ignore = "needs OXPINYIN_INITBENCH_{PIN,OX}_{PINYIN,ZHUYIN} and OXPINYIN_INITBENCH_DATA (see docs/runbooks/initbench.md)"]
fn live_pin_and_oxpinyin_are_comparable_on_fresh_and_populated_profiles() {
    let _guard = ONE_LIBRARY_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let real = real();
    let dir = scratch("live-verdicts");
    let (empty, modest) = (dir.join("empty"), dir.join("modest"));
    empty_fixture(&empty);
    modest_fixture(&modest, &real.pin_pinyin, &real.data);
    let mut p = plan(
        &real,
        &dir.join("work"),
        vec![empty, modest],
        vec![
            spec("pin", &real.pin_pinyin, &real.pin_zhuyin),
            spec("ox", &real.ox_pinyin, &real.ox_zhuyin),
        ],
        vec![Facade::Pinyin, Facade::Zhuyin],
    );
    p.rounds = 0;
    let records = collect(&p);
    let verdicts: Vec<_> = records
        .iter()
        .filter_map(|r| {
            if let Record::Verdict(v) = r {
                Some(v)
            } else {
                None
            }
        })
        .collect();
    // empty + modest + the persistence scenario, for two facades.
    assert_eq!(verdicts.len(), 6, "{verdicts:#?}");
    for v in verdicts {
        assert!(
            v.comparable,
            "{} / {} is NOT COMPARABLE: {:?}",
            v.facade.prefix(),
            v.profile,
            v.reasons
        );
        assert!(
            v.transcript_lines > 100,
            "the transcript is not a complete observation: {}",
            v.transcript_lines
        );
    }
    // Hostile inputs ran and were recorded for both subjects and both facades.
    let probes = records
        .iter()
        .filter(|r| matches!(r, Record::Probe(_)))
        .count();
    assert_eq!(
        probes,
        2 * 2 * oxpinyin_initbench::observe::HOSTILE_INPUTS.len()
    );
}

/// A timed run against the real libraries ends in samples whose trace digests
/// equal the verified ones, for every subject.
#[test]
#[ignore = "needs OXPINYIN_INITBENCH_{PIN,OX}_{PINYIN,ZHUYIN} and OXPINYIN_INITBENCH_DATA (see docs/runbooks/initbench.md)"]
fn live_timed_samples_reproduce_the_verified_workload() {
    let _guard = ONE_LIBRARY_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let real = real();
    let dir = scratch("live-timed");
    let empty = dir.join("empty");
    empty_fixture(&empty);
    let p = plan(
        &real,
        &dir.join("work"),
        vec![empty.clone()],
        vec![
            spec("pin", &real.pin_pinyin, &real.pin_zhuyin),
            spec("ox", &real.ox_pinyin, &real.ox_zhuyin),
        ],
        vec![Facade::Pinyin, Facade::Zhuyin],
    );
    let records = collect(&p);
    let samples: Vec<_> = records
        .iter()
        .filter_map(|r| {
            if let Record::Sample(s) = r {
                Some(s)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        samples.len(),
        (3 + 1) * 2 * 2,
        "3 rounds + 1 warm-up, 2 subjects, 2 facades"
    );
    for s in &samples {
        assert!(s.sample.ttfc_ns.is_some());
        let first = s
            .sample
            .phases
            .first_candidate
            .as_ref()
            .expect("a first candidate");
        assert!(!first.text.is_empty() && first.count > 0);
    }
    assert!(
        !records
            .iter()
            .any(|r| matches!(r, Record::Verdict(v) if !v.comparable)),
        "a timed run contradicted its verification"
    );
    // The fixture was never touched.
    assert!(
        Manifest::of_directory(&empty.join(USER_SUBDIR))
            .expect("fixture")
            .files
            .is_empty()
    );
}

fn observe_pin(real: &Real, user: &Path, witness: &[String]) -> Observation {
    let probe = host_probe().expect("a memory probe");
    let options = ChildOptions {
        facade: Facade::Pinyin,
        system_dir: real.data.clone(),
        user_dir: user.to_path_buf(),
        kind: SampleKind::Timing,
        stop_after: StopPoint::End,
        bind: Bind::Now,
    };
    let lib = real.pin_pinyin.clone();
    child::verify(
        || Native::open(&lib, Facade::Pinyin, Bind::Now),
        &options,
        "p",
        witness,
        probe.as_ref(),
    )
    .expect("the pin verifies")
}

/// Real observations, mutated: a changed workload, a tampered answer and a
/// different profile are each detected; an unchanged re-run is not.
#[test]
#[ignore = "needs OXPINYIN_INITBENCH_{PIN,OX}_{PINYIN,ZHUYIN} and OXPINYIN_INITBENCH_DATA (see docs/runbooks/initbench.md)"]
fn live_intentional_mutations_are_detected() {
    let _guard = ONE_LIBRARY_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let real = real();
    let dir = scratch("live-mutations");
    let user = dir.join("user");
    fs::create_dir_all(&user).expect("user dir");
    let base = observe_pin(&real, &user, &[]);
    let _ = fs::remove_dir_all(&user);
    fs::create_dir_all(&user).expect("user dir");
    let again = observe_pin(&real, &user, &[]);
    let stable = stable_files(&base, &again);
    assert_eq!(
        compare(&base, &again, &stable),
        Verdict::Comparable,
        "an unchanged re-run must compare equal"
    );

    // A different workload (one more input) is a different transcript.
    let _ = fs::remove_dir_all(&user);
    fs::create_dir_all(&user).expect("user dir");
    let mutated = observe_pin(&real, &user, &["zhongguo".to_owned()]);
    assert!(
        !compare(&base, &mutated, &stable).is_comparable(),
        "a mutated workload went unnoticed"
    );

    // A tampered answer is detected, and the difference names the line.
    let mut tampered = again.clone();
    let candidate = tampered
        .lines
        .iter_mut()
        .find(|l| l.op.ends_with(": candidate"))
        .expect("a candidate line");
    candidate.value.push('!');
    let Verdict::NotComparable(why) = compare(&base, &tampered, &stable) else {
        panic!("a tampered answer went unnoticed")
    };
    assert!(why[0].contains("line "), "{why:?}");

    // A different option word would change the unit trace: simulate its digest.
    let mut other_trace = again.clone();
    other_trace.unit_trace_digest.push('0');
    assert!(!compare(&base, &other_trace, &stable).is_comparable());

    // A populated profile is a different observation from the empty one (so the
    // fixture really is loaded), and the pin agrees with itself on it.
    let modest = dir.join("modest");
    modest_fixture(&modest, &real.pin_pinyin, &real.data);
    let witness = vec!["ceshi".to_owned(), "cece".to_owned()];
    let populated_user = dir.join("populated-user");
    for _ in 0..2 {
        let _ = fs::remove_dir_all(&populated_user);
        oxpinyin_initbench::profile::copy_tree(&modest.join(USER_SUBDIR), &populated_user)
            .expect("copy");
    }
    let populated = observe_pin(&real, &populated_user, &witness);
    let empty_user = dir.join("empty-user");
    fs::create_dir_all(&empty_user).expect("empty user dir");
    let empty = observe_pin(&real, &empty_user, &witness);
    let differing = fixtures::discriminating(&empty.lines, &populated.lines);
    assert!(
        !differing.is_empty(),
        "the saved-and-reopened profile answered exactly like the empty one"
    );
}
