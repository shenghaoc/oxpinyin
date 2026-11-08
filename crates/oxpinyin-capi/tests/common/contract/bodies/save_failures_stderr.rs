//! The save_failures_stderr family's case bodies: each answers the result document the old case returned.
//!
//! Save failures and the pin's stderr (inventory row 59, #525, #545, #929):
//! blocked staging and final names, removed user directories, header writes
//! that fail, interrupted saves and reloads of a saved profile.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::facade;
use crate::fixtures::{
    ReuseStep, TOKEN_REUSE_PHRASES, crafted_bigram_context, gram_bytes, hex_bytes, index_file_rows,
    token_reuse_reading, token_reuse_sequence, write_profile,
};
use crate::readers::{c, lookup};
use crate::support::{J, Kit, P, Raw};

/// The case body for `id`.
/// The body of a case whose reference holds the pin-written handoff profile.
pub fn case_with_profile(k: &mut Kit, id: &str, profile: &Raw) -> J {
    match id {
        "save-929-logical-cross-open-pinyin" | "save-929-logical-cross-open-zhuyin" => {
            save_929_logical_roundtrip(k, profile)
        }
        other => panic!("no profile case {other}"),
    }
}

/// The case body for `id`.
pub fn case(k: &mut Kit, id: &str) -> J {
    if let Some(out) = save_929_case(k, id) {
        return out;
    }
    match id {
        "stderr-save-one-tmp-blocked" | "stderr-save-one-tmp-blocked-zhuyin" => {
            blocked_save(k, "user_pinyin_index.bin.tmp")
        }
        #[cfg(unix)]
        "abort-save-chunk-header-length" | "abort-zhuyin-save-chunk-header-length" => {
            save_header_dev_full(k)
        }
        #[cfg(unix)]
        "abort-save-chunk-header-checksum" | "abort-zhuyin-save-chunk-header-checksum" => {
            save_header_rlimit(k)
        }
        "stderr-fresh-user-dir" | "stderr-fresh-user-dir-zhuyin" => {
            let ctx = k.ctx();
            facade::fini(ctx);
            obj! { "init" => !ctx.is_null() }
        }
        "stderr-init-missing-system-dir" | "stderr-init-missing-system-dir-zhuyin" => {
            let ctx = k.init(Some(Path::new("/nonexistent-system-dir")), Some(&k.user));
            obj! { "ctx" => !ctx.is_null() }
        }
        "stderr-init-system-dir-without-table-conf" => {
            let empty = k.scratch.join("sys-X");
            std::fs::create_dir(&empty).expect("the empty system directory");
            let system = PathBuf::from(format!("{}//", empty.to_str().expect("UTF-8")));
            let ctx = k.init(Some(&system), Some(&k.user));
            obj! { "ctx" => !ctx.is_null() }
        }
        "stderr-save-dir-removed" | "stderr-save-dir-removed-zhuyin" => {
            save_after_training(k, true)
        }
        "stderr-fini-dir-removed" | "stderr-fini-dir-removed-zhuyin" => {
            let ctx = k.ctx();
            std::fs::remove_dir_all(&k.user).expect("remove the user directory");
            facade::fini(ctx);
            J::map()
        }
        #[cfg(unix)]
        "stderr-non-utf8-user-dir" => non_utf8_user_dir(k),
        "stderr-quiet-saves" => quiet_saves(k),
        "stderr-unmodified-save-dir-removed" => {
            let ctx = k.ctx();
            std::fs::remove_dir_all(&k.user).expect("remove the user directory");
            obj! { "save" => facade::save(ctx) }
        }
        other => panic!("no save_failures_stderr case {other}"),
    }
}

/// The `save-929-*` cases, `None` for another case.
fn save_929_case(k: &mut Kit, id: &str) -> Option<J> {
    let suffix = if facade::ZHUYIN { "-zhuyin" } else { "-pinyin" };
    let stem = id.strip_prefix("save-929-")?.strip_suffix(suffix)?;
    Some(match stem {
        "rename-user_bigram.db.tmp" => save_bigram_tmp_blocked(k),
        "stale-bigram-tmp" => save_bigram_stale_tmp(k),
        "reload" => save_929_roundtrip(k),
        #[cfg(unix)]
        "sync-rejecting" => save_929_sync_rejecting(k),
        #[cfg(target_os = "linux")]
        "boundary-save-marker" => save_929_late_boundary(k, "save-marker"),
        #[cfg(target_os = "linux")]
        "boundary-after-save" => save_929_late_boundary(k, "after-save"),
        #[cfg(target_os = "linux")]
        "boundary-after-fini" => save_929_late_boundary(k, "after-fini"),
        #[cfg(target_os = "linux")]
        "boundary-fini-marker" => save_929_late_boundary(k, "fini-marker"),
        other => {
            if let Some(blocked) = other.strip_prefix("rename-") {
                save_929_failure(k, blocked)
            } else {
                panic!("no save_failures_stderr case {id}")
            }
        }
    })
}

/// The facade's parse of `LIBRARY_FACADES`' text: `nihao` or `su3cl3`.
fn parse_text(inst: P) {
    if facade::ZHUYIN {
        facade::parse_more_chewings(inst, c("su3cl3").as_ptr());
    } else {
        facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
    }
}

/// A fresh instance of the cached context, parsed and guessed.
fn guessed(k: &mut Kit) -> P {
    let inst = k.alloc();
    parse_text(inst);
    facade::guess_sentence(inst);
    inst
}

/// `save_after_training`: train a guessed sentence, optionally remove the user
/// directory, then save the cached context.
fn save_after_training(k: &mut Kit, remove_dir: bool) -> J {
    let ctx = k.ctx();
    let inst = guessed(k);
    let mut out = obj! { "train" => facade::train_top(inst) };
    if remove_dir {
        std::fs::remove_dir_all(&k.user).expect("remove the user directory");
    }
    out.set("save", facade::save(ctx));
    out
}

/// `_dirty_for_save`: parse, guess and train, answering the cached context.
fn dirty_for_save(k: &mut Kit) -> P {
    let ctx = k.ctx();
    let inst = guessed(k);
    facade::train_top(inst);
    ctx
}

/// `blocked_save`'s snapshot: each user entry by its sorted listing or contents.
fn snapshot(k: &Kit) -> BTreeMap<String, String> {
    use sha2::Digest as _;
    let mut files = BTreeMap::new();
    for entry in std::fs::read_dir(&k.user).expect("user directory") {
        let entry = entry.expect("user entry");
        let path = entry.path();
        let name = crate::support::os_text(&entry.file_name());
        let view = if path.is_dir() {
            let mut names: Vec<String> = std::fs::read_dir(&path)
                .expect("a listed directory")
                .map(|inner| crate::support::os_text(&inner.expect("entry").file_name()))
                .collect();
            names.sort();
            format!("dir:{}", names.join(","))
        } else {
            hex_bytes(&sha2::Sha256::digest(
                std::fs::read(&path).expect("a user file"),
            ))
        };
        files.insert(name, view);
    }
    files
}

/// `blocked_save`: a second, modified save with `blocked` replaced by a
/// non-empty directory, both saves' answers and which entries it updated.
fn blocked_save(k: &mut Kit, blocked: &str) -> J {
    let ctx = k.ctx();
    let inst = guessed(k);
    facade::train_top(inst);
    let mut out = obj! { "first" => facade::save(ctx) };
    let before = snapshot(k);
    facade::train_top(inst);
    let path = k.user.join(blocked);
    if path.exists() {
        std::fs::rename(&path, k.user.join(format!("{blocked}.moved"))).expect("move aside");
    }
    std::fs::create_dir(&path).expect("the blocking directory");
    std::fs::write(path.join("keep"), "x").expect("keep");
    out.set("second", facade::save(ctx));
    let after = snapshot(k);
    let mut files = J::map();
    for (name, view) in &after {
        if name.ends_with(".moved") {
            continue;
        }
        let state = match before.get(name) {
            None => "new",
            Some(old) if old != view => "changed",
            Some(_) => "same",
        };
        files.set(name, state);
    }
    out.set("files", files);
    out
}

/// `save_bigram_tmp_blocked`: block `user_bigram.db.tmp` and report the staged
/// and final names.
fn save_bigram_tmp_blocked(k: &mut Kit) -> J {
    let mut out = blocked_save(k, "user_bigram.db.tmp");
    out.set("tmp_is_dir", k.user.join("user_bigram.db.tmp").is_dir());
    out.set("final_is_file", k.user.join("user_bigram.db").is_file());
    out
}

/// `save_bigram_stale_tmp`: a stale `user_bigram.db.tmp` before a second save,
/// then the final bigram's rows.
fn save_bigram_stale_tmp(k: &mut Kit) -> J {
    let ctx = dirty_for_save(k);
    let first = facade::save(ctx);
    std::fs::write(k.user.join("user_bigram.db.tmp"), b"stale staging bytes").expect("stale");
    let inst = guessed(k);
    facade::train_top(inst);
    let second = facade::save(ctx);
    obj! {
        "first" => first,
        "second" => second,
        "tmp_exists" => k.user.join("user_bigram.db.tmp").exists(),
        "rows" => index_file_rows(&k.user.join("user_bigram.db"), true),
    }
}

/// `save_929_failure`: block a final name, then check the soft failure and the
/// clean-save guard.
fn save_929_failure(k: &mut Kit, blocked: &str) -> J {
    let mut out = blocked_save(k, blocked);
    out.set("clean_save", facade::save(k.ctx()));
    out.set(
        "staged_retained",
        k.user.join(format!("{blocked}.tmp")).is_file(),
    );
    out.set(
        "destination_retained",
        std::fs::read_to_string(k.user.join(blocked).join("keep")).expect("keep"),
    );
    out
}

/// `save_929_roundtrip`: saved imports survive reopen; an unsaved suffix does not survive fini.
fn save_929_roundtrip(k: &mut Kit) -> J {
    use ReuseStep::{Add, Reopen, Save};
    token_reuse_sequence(
        k,
        &[Add(0), Save, Reopen, Add(1), Reopen, Add(2), Save, Reopen],
        None,
        7,
    )
}

/// `save_929_sync_rejecting`: a `user.bin.tmp` linked to `/dev/null`, whose
/// sync fails, cannot stop the save. Unix only: `/dev/null` and symbolic links.
#[cfg(unix)]
fn save_929_sync_rejecting(k: &mut Kit) -> J {
    let null = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/null")
        .expect("/dev/null");
    assert!(
        null.sync_all().is_err(),
        "/dev/null must reject sync on this host"
    );
    drop(null);
    let ctx = dirty_for_save(k);
    std::os::unix::fs::symlink("/dev/null", k.user.join("user.bin.tmp")).expect("link");
    let saved = facade::save(ctx);
    let clean = facade::save(ctx);
    let user_bin = k.user.join("user.bin");
    let renamed = user_bin.is_symlink().then(|| {
        std::fs::read_link(&user_bin)
            .expect("readlink")
            .to_str()
            .expect("UTF-8")
            .to_owned()
    });
    obj! {
        "saved" => saved,
        "clean" => clean,
        "renamed_target" => renamed,
        "bigram_exists" => k.user.join("user_bigram.db").is_file(),
    }
}

/// `save_header_dev_full`: `user.bin.tmp` linked to `/dev/full`, so the first
/// header word's write fails (`memory_chunk.h:543`). Unix only: `/dev/full`
/// (Linux) and symbolic links.
#[cfg(unix)]
fn save_header_dev_full(k: &mut Kit) -> J {
    let ctx = dirty_for_save(k);
    std::os::unix::fs::symlink("/dev/full", k.user.join("user.bin.tmp")).expect("link");
    obj! { "ret" => facade::save(ctx) }
}

/// `save_header_rlimit`: a four-byte `RLIMIT_FSIZE`, so the checksum word past
/// the length word is refused (`memory_chunk.h:547`). Unix only: the resource limit.
#[cfg(unix)]
fn save_header_rlimit(k: &mut Kit) -> J {
    let ctx = dirty_for_save(k);
    let limit = libc::rlimit {
        rlim_cur: 4,
        rlim_max: 4,
    };
    // SAFETY: ignores SIGXFSZ and lowers this child's own file-size limit, as the
    // old case did; nothing restores them.
    unsafe {
        libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
        assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &raw const limit), 0);
    }
    obj! { "ret" => facade::save(ctx) }
}

/// The user directory with a `0xFF` byte after its `user-` prefix.
#[cfg(unix)]
fn non_utf8_user(k: &Kit) -> PathBuf {
    use std::os::unix::ffi::OsStrExt as _;
    let base = k.user.file_name().expect("a base name").as_bytes();
    let name = [&b"user-\xff"[..], &base[5..]].concat();
    k.scratch.join(std::ffi::OsStr::from_bytes(&name))
}

/// `stderr-non-utf8-user-dir`: train in a user directory whose name holds
/// `0xFF`, remove it, then save and finalise. Unix only: a byte-string
/// directory name.
#[cfg(unix)]
fn non_utf8_user_dir(k: &mut Kit) -> J {
    let user = non_utf8_user(k);
    std::fs::create_dir(&user).expect("the user directory");
    let ctx = k.init(Some(&k.data), Some(&user));
    let inst = facade::alloc_instance(ctx);
    facade::parse_more_full_pinyins(inst, c("nihao").as_ptr());
    facade::guess_sentence(inst);
    let mut out = obj! { "train" => facade::train_top(inst) };
    std::fs::remove_dir_all(&user).expect("remove the user directory");
    out.set("save", facade::save(ctx));
    facade::fini(ctx);
    out
}

/// `stderr-quiet-saves`: unmodified and trained saves, then a reopen and save
/// of the persisted profile.
fn quiet_saves(k: &mut Kit) -> J {
    let ctx = k.ctx();
    let mut out = obj! { "unmodified save" => facade::save(ctx) };
    let trained = save_after_training(k, false);
    for key in ["train", "save"] {
        out.set(key, trained.get(key).expect("a trained field").clone());
    }
    facade::fini(ctx);
    let again = k.init_default();
    out.set("reopen", !again.is_null());
    out.set("second save", facade::save(again));
    facade::fini(again);
    out
}

/// `save_929_import`: imports `TOKEN_REUSE_PHRASES[number]` into library 7.
fn save_929_import(ctx: P, number: usize, count: i32) {
    let it = facade::begin_add_phrases(ctx, 7);
    assert!(!it.is_null(), "the import iterator");
    let text = c(TOKEN_REUSE_PHRASES[number].0);
    let reading = c(token_reuse_reading(number));
    assert!(facade::iterator_add_phrase(
        it,
        text.as_ptr(),
        reading.as_ptr(),
        count
    ));
    facade::end_add_phrases(it);
}

/// `save_929_tokens`: the tokens of the first three phrases on a fresh instance.
fn save_929_tokens(ctx: P) -> Vec<Vec<u32>> {
    let inst = facade::alloc_instance(ctx);
    assert!(!inst.is_null(), "alloc_instance failed");
    let tokens = TOKEN_REUSE_PHRASES[..3]
        .iter()
        .map(|(text, _, _)| lookup(inst, text.as_bytes()))
        .collect();
    facade::free_instance(inst);
    tokens
}

/// `save_929_late_boundary`: save and finalise through the facade in a save
/// child, stop it at the named boundary, then reopen the profile and read it.
/// The pin writes the marker after the data renames (`src/pinyin.cpp:1141-1146`,
/// `src/zhuyin.cpp:695-698`); fini never saves dirty dictionaries and only
/// pinyin's writes a marker (`src/pinyin.cpp:1194-1200`, `src/zhuyin.cpp:741-757`).
/// The save child is this test binary run again for the case ([`SaveChild`]):
/// it initialises its own context on this case's user directory and makes
/// every facade call, while this process reads the marker between its steps
/// and, at a marker boundary, holds the lease that stops it. Linux only: the
/// `F_SETLEASE` read lease that finds the marker's write-open.
#[cfg(target_os = "linux")]
fn save_929_late_boundary(k: &mut Kit, boundary: &str) -> J {
    if std::env::var_os(SAVE_CHILD).is_some() {
        save_child(k, boundary);
    }
    let marker = k.user.join("user.conf");
    let mut child = SaveChild::start(&k.user);
    child.reached(SAVE_FIRST);
    let marker_before = std::fs::read(&marker).expect("the marker");
    child.go();
    child.reached(SAVE_READY);
    let stopped = if matches!(boundary, "save-marker" | "fini-marker") {
        let stopped = marker_interrupt(&marker, child);
        assert_eq!(std::fs::read(&marker).expect("the marker"), marker_before);
        stopped
    } else {
        child.go();
        exit_73(child)
    };
    let saved = physical_files(&k.user);
    assert!(saved.keys().all(|name| !name.ends_with(".tmp")));
    let reopened = k.ctx();
    let tokens = save_929_tokens(reopened);
    assert_eq!(tokens, [vec![0x0700_0001], vec![0x0700_0002], vec![]]);
    let after_open = physical_files(&k.user);
    facade::fini(reopened);
    k.forget();
    let after_fini = physical_files(&k.user);
    assert!(saved.keys().eq(after_open.keys()) && saved.keys().eq(after_fini.keys()));
    for (name, bytes) in &saved {
        if name != "user.conf" {
            assert_eq!(after_open.get(name), Some(bytes), "{name} changed on open");
            assert_eq!(after_fini.get(name), Some(bytes), "{name} changed on fini");
        }
    }
    let unchanged = |later: &BTreeMap<String, Vec<u8>>| {
        let mut out = J::map();
        for (name, bytes) in &saved {
            if name != "user.conf" {
                out.set(name, later.get(name) == Some(bytes));
            }
        }
        out
    };
    obj! {
        "boundary" => stopped,
        "tokens" => tokens,
        "files" => saved.keys().cloned().collect::<Vec<String>>(),
        "marker" => String::from_utf8(marker_before).expect("a UTF-8 marker"),
        "data_unchanged_on_open" => unchanged(&after_open),
        "data_unchanged_on_fini" => unchanged(&after_fini),
        "final_marker" => std::fs::read_to_string(&marker).expect("the final marker"),
    }
}

/// `save_929_physical_files`: every regular file's complete bytes, without
/// normalization; a non-regular entry is an error, not an omitted observation.
#[cfg(target_os = "linux")]
fn physical_files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(dir)
        .expect("the user directory")
        .map(|entry| {
            let entry = entry.expect("a user entry");
            let kind = entry.file_type().expect("a file type");
            assert!(
                kind.is_file() && !kind.is_symlink(),
                "nonregular saved profile entry: {}",
                entry.path().display()
            );
            (
                crate::support::os_text(&entry.file_name()),
                std::fs::read(entry.path()).expect("a saved file"),
            )
        })
        .collect()
}

/// Set in a save child: the case's user directory, where it saves.
#[cfg(target_os = "linux")]
const SAVE_CHILD: &str = "OXPINYIN_CONTRACT_SAVE_929_USER";
/// The file a save child writes after its first save, then waits for the
/// go-ahead on its stdin.
#[cfg(target_os = "linux")]
const SAVE_FIRST: &str = "save-first";
/// The file a save child writes once only the interrupted steps are left,
/// then waits for the go-ahead on its stdin.
#[cfg(target_os = "linux")]
const SAVE_READY: &str = "save-ready";
/// How long a save child may run.
#[cfg(target_os = "linux")]
const SAVE_LIMIT: std::time::Duration = std::time::Duration::from_secs(120);

/// In a save child: the old case's facade calls on the case's user directory.
/// Import and save a phrase; then (`fini-marker`) a second; then, once let go,
/// commit a second phrase (other boundaries), leave a third unsaved and
/// (`fini-marker`, `after-fini`) finalise. It exits 73 without unwinding or
/// teardown, as the old forked child did, or 74 on a panic or, at a marker
/// boundary, when the boundary was missed.
#[cfg(target_os = "linux")]
fn save_child(k: &mut Kit, boundary: &str) -> ! {
    let scratch = k.scratch.clone();
    let ready = |name: &str| {
        std::fs::write(scratch.join(name), b"").expect("mark the step");
        let mut go = [0_u8];
        std::io::Read::read_exact(&mut std::io::stdin(), &mut go).expect("the go-ahead");
    };
    k.user = PathBuf::from(std::env::var_os(SAVE_CHILD).expect("the case's user directory"));
    let ctx = k.ctx();
    let steps = || {
        save_929_import(ctx, 0, 100);
        assert!(facade::save(ctx));
        ready(SAVE_FIRST);
        if boundary == "fini-marker" {
            save_929_import(ctx, 1, 100);
            assert!(facade::save(ctx));
        }
        ready(SAVE_READY);
        if boundary != "fini-marker" {
            save_929_import(ctx, 1, 100);
            assert!(facade::save(ctx));
        }
        save_929_import(ctx, 2, 100);
        if matches!(boundary, "fini-marker" | "after-fini") {
            facade::fini(ctx);
        }
    };
    let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(steps)).is_ok();
    let code = if ok && matches!(boundary, "after-save" | "after-fini") {
        73
    } else {
        74
    };
    // SAFETY: `_exit` ends the process at once, without unwinding or atexit
    // handlers.
    unsafe { libc::_exit(code) }
}

/// A save child: this test binary run again for the case, with the case's user
/// directory, within [`SAVE_LIMIT`].
#[cfg(target_os = "linux")]
struct SaveChild {
    child: crate::support::Bounded,
    go: std::process::ChildStdin,
    scratch: tempfile::TempDir,
}

#[cfg(target_os = "linux")]
impl SaveChild {
    fn start(user: &Path) -> Self {
        let scratch = tempfile::Builder::new()
            .prefix("save-")
            .tempdir()
            .expect("the save child's scratch");
        let mut command = crate::support::case_command(scratch.path());
        command
            .env(SAVE_CHILD, user)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit());
        let mut child = crate::support::Bounded::spawn("the save child", &mut command, SAVE_LIMIT);
        let go = child.child().stdin.take().expect("the save child's stdin");
        Self { child, go, scratch }
    }

    /// Waits until the child has written `step`; it fails if the child exits
    /// first or its deadline passes.
    fn reached(&mut self, step: &str) {
        while !self.scratch.path().join(step).exists() {
            if let Some(status) = self
                .child
                .poll()
                .unwrap_or_else(|overdue| panic!("{overdue}"))
            {
                panic!("the save child exited before {step} ({status:?})");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// Lets the child take its next steps.
    fn go(&mut self) {
        std::io::Write::write_all(&mut self.go, b"g").expect("let the save child go");
    }
}

/// Lets the save child finish and expects it to exit 73 without unwinding (74
/// on a panic), as the old case did to leave fini and teardown out; answers
/// `exit-73` once it has.
#[cfg(target_os = "linux")]
fn exit_73(child: SaveChild) -> &'static str {
    let status = child
        .child
        .wait()
        .unwrap_or_else(|overdue| panic!("{overdue}"));
    assert_eq!(
        status.code(),
        Some(73),
        "the save child's status was {status:?}"
    );
    "exit-73"
}

/// `save_929_marker_interrupt`: kills the save child at the marker's
/// write-open, notified by a Linux file lease. This process holds a read lease
/// on the marker before it lets the child go; the lease stops the child's
/// write-open from completing, and `SIGIO` is the kernel's lease-break
/// notification, directed at this thread, not a timed poll of the file system.
/// The child is killed and reaped before the lease is released; the bounded
/// signal wait fails if the boundary is never reached. No production hook or
/// preload shim.
#[cfg(target_os = "linux")]
fn marker_interrupt(path: &Path, mut child: SaveChild) -> &'static str {
    use std::os::unix::ffi::OsStrExt as _;
    // By number: the libc crate lacks `F_SETOWN_EX` and `F_OWNER_TID`
    // (`fcntl.h`: 15 and 0).
    const F_SETOWN_EX: libc::c_int = 15;
    const F_OWNER_TID: libc::c_int = 0;
    /// `struct f_owner_ex`.
    #[repr(C)]
    struct FOwnerEx {
        kind: libc::c_int,
        pid: libc::pid_t,
    }
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).expect("a path without NUL");
    // SAFETY: plain libc calls on this thread's signal mask and on the lease
    // descriptor; the save child is a separate process that never sees the
    // descriptor. The child is killed and reaped, the lease released and the
    // mask restored before this returns.
    let (got, status) = unsafe {
        let mut only_sigio: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&raw mut only_sigio);
        libc::sigaddset(&raw mut only_sigio, libc::SIGIO);
        let mut old_mask: libc::sigset_t = std::mem::zeroed();
        assert_eq!(
            libc::pthread_sigmask(libc::SIG_BLOCK, &raw const only_sigio, &raw mut old_mask),
            0,
            "block SIGIO"
        );
        let fd = libc::open(c_path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC);
        assert!(fd >= 0, "open the marker");
        // The lease break's SIGIO goes to this thread.
        let owner = FOwnerEx {
            kind: F_OWNER_TID,
            pid: libc::gettid(),
        };
        assert_eq!(
            libc::fcntl(fd, F_SETOWN_EX, &raw const owner),
            0,
            "F_SETOWN_EX"
        );
        assert_eq!(
            libc::fcntl(fd, libc::F_SETLEASE, libc::F_RDLCK),
            0,
            "F_SETLEASE"
        );
        child.go();
        let timeout = libc::timespec {
            tv_sec: 10,
            tv_nsec: 0,
        };
        let mut info: libc::siginfo_t = std::mem::zeroed();
        let got = libc::sigtimedwait(&raw const only_sigio, &raw mut info, &raw const timeout);
        let status = child.child.kill();
        libc::fcntl(fd, libc::F_SETLEASE, libc::F_UNLCK);
        libc::close(fd);
        libc::pthread_sigmask(libc::SIG_SETMASK, &raw const old_mask, std::ptr::null_mut());
        (got, status)
    };
    assert_eq!(
        got,
        libc::SIGIO,
        "marker write-open did not request a lease break"
    );
    let status = status.expect("the save child was reaped");
    assert_eq!(
        std::os::unix::process::ExitStatusExt::signal(&status),
        Some(libc::SIGKILL),
        "the save child's status was {status:?}"
    );
    "marker-open"
}

/// `save_929_logical_roundtrip`'s reload half: the three user containers' rows,
/// the tokens, a count-zero re-import and a save that changes no record.
fn logical_reload(k: &mut Kit, bigram: &(Vec<u8>, Vec<u8>)) -> J {
    let rows = |k: &Kit| {
        let mut map = J::map();
        for name in [
            "user_pinyin_index.bin",
            "user_phrase_index.bin",
            "user_bigram.db",
        ] {
            map.set(
                name,
                index_file_rows(&k.user.join(name), name == "user_bigram.db"),
            );
        }
        map
    };
    let before = rows(k);
    assert_eq!(
        before.get("user_bigram.db"),
        Some(&J::from(vec![(hex_bytes(&bigram.0), hex_bytes(&bigram.1))]))
    );
    let ctx = k.ctx();
    let tokens = save_929_tokens(ctx);
    assert_eq!(tokens, [vec![0x0700_0001], vec![0x0700_0002], vec![]]);
    save_929_import(ctx, 0, 0);
    assert!(facade::save(ctx));
    let after = rows(k);
    assert_eq!(before, after, "facade reopen/save changed logical records");
    facade::fini(ctx);
    k.forget();
    obj! { "rows" => after, "tokens" => tokens }
}

/// `save_929_logical_roundtrip`: a donor profile in this side's handoff
/// directory (crafted bigram row, two saved imports and an unsaved one), then
/// its reload, as the old verify ran it. The pin's profile is then reloaded:
/// its rows and tokens must equal this side's reload, and that reload, with its
/// GLib records alone, is `cross_reload` (the old verify's fresh reload worker).
fn save_929_logical_roundtrip(k: &mut Kit, profile: &Raw) -> J {
    let own = k.scratch.join("handoff-subject-X");
    std::fs::create_dir(&own).expect("the handoff profile");
    k.user = own;
    let bigram = (
        0x0700_0001_u32.to_le_bytes().to_vec(),
        gram_bytes(37, &[(0x0700_0002, 37)]),
    );
    let ctx = crafted_bigram_context(k, std::slice::from_ref(&bigram));
    save_929_import(ctx, 0, 100);
    save_929_import(ctx, 1, 100);
    assert!(facade::save(ctx));
    save_929_import(ctx, 2, 100);
    facade::fini(ctx);
    let mut out = logical_reload(k, &bigram);
    let donor = k.scratch.join("handoff-pin-X");
    std::fs::create_dir(&donor).expect("the pin's handoff profile");
    write_profile(profile, &donor);
    k.user = donor;
    let seen = k.logs().len();
    let mut cross = logical_reload(k, &bigram);
    assert_eq!(cross, out, "cross-open changed logical records");
    let logs: Vec<J> = k.logs().into_iter().skip(seen).map(J::from).collect();
    cross.set("logs", logs);
    out.set("cross_reload", cross);
    out
}
