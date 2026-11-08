//! The frozen pin contract cases' runner, shared by both facades' contract targets.
//!
//! Each case runs in a fresh child process, as `contract-diff.py` ran each case in a
//! fresh worker: the test binary is re-executed with `--exact` and the case's full
//! libtest name. The child builds a [`Kit`] over a fresh user directory, runs the
//! case body and asserts its typed result and GLib logs; the parent asserts how the
//! child ended, its stderr and the user directory it left. A child that ran no test
//! fails the parent, so a wrong name cannot pass.
//!
//! This is one source file and no crate: each facade's target includes it with
//! `#[path]` and supplies its own `facade` module, the C symbols under one prefix.
#![allow(dead_code)]

use std::ffi::{CStr, CString, c_char, c_void};
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// An opaque C pointer: a context, instance, candidate, iterator or key.
pub type P = *mut c_void;

/// The out-parameter sentinel the cases seed (`contract_abi.UNTOUCHED`).
pub const UNTOUCHED: u32 = 0x00AB_CDEF;

/// The DBM cell this binary was built for.
pub const CELL: &str = match oxpinyin_data::DEFAULT_STORE_EXT.as_bytes() {
    b"db" => "bdb",
    b"kct" => "kc",
    _ => "tkrzw",
};

/// The environment variable naming the case a re-executed child runs.
const CHILD: &str = "OXPINYIN_CONTRACT_CHILD";
/// The environment variable naming the child's scratch directory.
const SCRATCH: &str = "OXPINYIN_CONTRACT_SCRATCH";
/// The file a child writes before the case body, so a child that ran no test fails.
const RAN: &str = "ran";
/// How long a child may run: the old harness's 300-second case deadline.
const CHILD_LIMIT: Duration = Duration::from_secs(300);

/// `G_DEBUG=fatal-criticals`, the environment of a `fatal=True` case.
pub const FATAL_CRITICALS: &[(&str, &str)] = &[("G_DEBUG", "fatal-criticals")];

/// The repository root.
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Where `just fetch-contract-data` extracts the libpinyin-data version
/// `tools/oracle/libpinyin-data.json` pins: the cases' system data. A missing
/// package is a failure, not a skip.
pub fn contract_data_dir() -> PathBuf {
    let pin = std::fs::read_to_string(repo_root().join("tools/oracle/libpinyin-data.json"))
        .expect("the data pin is committed");
    let version = pin
        .lines()
        .find_map(|line| line.trim().strip_prefix("\"version\": \""))
        .and_then(|rest| rest.strip_suffix("\","))
        .expect("the data pin names a version");
    let dir = repo_root()
        .join("target/contract-data")
        .join(version)
        .join("libpinyin/data");
    assert!(
        dir.join("table.conf").is_file(),
        "pinned system data missing at {}; run just fetch-contract-data",
        dir.display()
    );
    dir
}

/// The kit's system data. The `contract_data` target reads the pinned package
/// ([`contract_data_dir`]). The `contract` cases read no system data, so they get
/// the nonexistent directory the old harness passed on every cell without the package.
fn system_data() -> PathBuf {
    if env!("CARGO_CRATE_NAME") == "contract_data" {
        contract_data_dir()
    } else {
        PathBuf::from("/nonexistent-contract-data")
    }
}

/// One GLib log record: the domain (`None` for the default domain) and the level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Log(pub Option<&'static str>, pub u32);

/// A case that logged nothing.
pub const NO_LOGS: &[Log] = &[];

/// One record the child's GLib handler saw: domain, level and message.
type Record = (Option<String>, u32, Option<String>);

/// What the child's GLib handler recorded, in order.
static LOGS: Mutex<Vec<Record>> = Mutex::new(Vec::new());

extern "C" fn record_log(
    domain: *const c_char,
    level: glib_sys::GLogLevelFlags,
    message: *const c_char,
    _data: glib_sys::gpointer,
) {
    let text = |pointer: *const c_char| {
        (!pointer.is_null()).then(|| {
            // SAFETY: GLib hands NUL-terminated strings that outlive the call.
            unsafe { CStr::from_ptr(pointer) }
                .to_string_lossy()
                .into_owned()
        })
    };
    LOGS.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push((
            text(domain).filter(|domain| !domain.is_empty()),
            level,
            text(message),
        ));
}

/// The child's view of one case: its scratch directory, the fresh user directory,
/// the system data and the lazily opened context and instance (`contract_kit.Kit`).
pub struct Kit {
    pub scratch: PathBuf,
    pub user: PathBuf,
    pub data: PathBuf,
    ctx: P,
    inst: P,
}

impl Kit {
    fn new(scratch: &Path) -> Self {
        let user = scratch.join("user-X");
        std::fs::create_dir(&user).expect("the case's user directory");
        // The handlers the old kit installed: both facade domains, GLib's own and
        // the default domain, for every level (`0xFFFFFFFC`).
        for domain in [Some(c"libpinyin"), Some(c"libzhuyin"), Some(c"GLib"), None] {
            // SAFETY: the domain is a static C string or NULL and the handler is a
            // plain function that ignores its user data.
            unsafe {
                glib_sys::g_log_set_handler(
                    domain.map_or(std::ptr::null(), CStr::as_ptr),
                    0xFFFF_FFFC,
                    Some(record_log),
                    std::ptr::null_mut(),
                );
            }
        }
        Self {
            scratch: scratch.to_path_buf(),
            user,
            data: system_data(),
            ctx: std::ptr::null_mut(),
            inst: std::ptr::null_mut(),
        }
    }

    /// The facade's `init` on the kit's system data and user directory.
    pub fn init_default(&self) -> P {
        self.init(Some(&self.data), Some(&self.user))
    }

    /// The facade's `init` on the given directories; `None` passes NULL.
    pub fn init(&self, system: Option<&Path>, user: Option<&Path>) -> P {
        let system = system.map(path_c);
        let user = user.map(path_c);
        crate::facade::init(
            system.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
            user.as_ref().map_or(std::ptr::null(), |u| u.as_ptr()),
        )
    }

    /// The cached context, opened on first use.
    pub fn ctx(&mut self) -> P {
        if self.ctx.is_null() {
            self.ctx = self.init_default();
            assert!(!self.ctx.is_null(), "init failed");
        }
        self.ctx
    }

    /// A fresh instance of the cached context.
    pub fn alloc(&mut self) -> P {
        let ctx = self.ctx();
        let inst = crate::facade::alloc_instance(ctx);
        assert!(!inst.is_null(), "alloc_instance failed");
        inst
    }

    /// The cached instance, allocated on first use.
    pub fn inst(&mut self) -> P {
        if self.inst.is_null() {
            self.inst = self.alloc();
        }
        self.inst
    }

    /// `phrase_table_reopen`'s close: frees the cached instance if one was
    /// allocated and finalises the cached context, then forgets both.
    pub fn close(&mut self) {
        if !self.inst.is_null() {
            crate::facade::free_instance(self.inst);
        }
        crate::facade::fini(self.ctx);
        self.forget();
    }

    /// The cached instance, or NULL when none was allocated.
    pub fn inst_if_any(&self) -> P {
        self.inst
    }

    /// Forgets the cached context and instance without freeing them: the case
    /// freed them itself.
    pub fn forget(&mut self) {
        self.ctx = std::ptr::null_mut();
        self.inst = std::ptr::null_mut();
    }

    /// Clears the GLib records so far.
    pub fn clear_logs(&self) {
        LOGS.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    /// The domains and levels recorded so far.
    pub fn logs(&self) -> Vec<Log> {
        LOGS.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(domain, level, _)| Log(domain.as_deref().map(intern), *level))
            .collect()
    }

    /// The messages recorded so far.
    pub fn messages(&self) -> Vec<Option<String>> {
        LOGS.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(_, _, message)| message.clone())
            .collect()
    }
}

fn intern(domain: &str) -> &'static str {
    match domain {
        "libpinyin" => "libpinyin",
        "libzhuyin" => "libzhuyin",
        "GLib" => "GLib",
        other => panic!("unexpected log domain {other}"),
    }
}

/// A path as the C string the facade takes (Unix: its bytes).
pub fn path_c(path: &Path) -> CString {
    #[cfg(unix)]
    let bytes = std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str()).to_vec();
    #[cfg(not(unix))]
    let bytes = path.to_str().expect("UTF-8 path").as_bytes().to_vec();
    CString::new(bytes).expect("a path without NUL")
}

/// Decodes and frees a caller-owned GLib string (`Kit.text`): `None` for NULL,
/// invalid UTF-8 replaced.
pub fn take_text(pointer: *mut c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    // SAFETY: a non-NULL pointer the facade handed over is a NUL-terminated
    // GLib allocation the caller owns.
    let text = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: the caller owns the allocation and frees it once.
    unsafe { glib_sys::g_free(pointer.cast()) };
    Some(text)
}

/// Reads a borrowed C string, invalid UTF-8 replaced; `None` for NULL.
pub fn borrowed_text(pointer: *const c_char) -> Option<String> {
    (!pointer.is_null()).then(|| {
        // SAFETY: a non-NULL pointer the facade handed out is NUL-terminated
        // and lives until the next call that changes its owner.
        unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned()
    })
}

/// How the child must end.
#[derive(Clone, Copy, Debug)]
pub enum End {
    /// The body returned and the child exited 0.
    Exit,
    /// A reproduced pin `assert`/`abort()`: SIGABRT on Unix, fast-fail on Windows.
    Aborts,
}

/// What the parent checks once the child has ended.
#[derive(Clone, Copy, Debug)]
pub struct Expect {
    pub end: End,
    /// The child's stderr, with the scratch directory written `SCRATCH`. `None`
    /// where the old harness did not compare stderr.
    pub stderr: Option<&'static [u8]>,
    /// The user-directory listing. `None` where the old harness did not compare it.
    pub files: Option<&'static [(&'static str, UserFile)]>,
    /// Extra environment for the child.
    pub env: &'static [(&'static str, &'static str)],
}

impl Expect {
    /// An ordinary case: exit 0 and the result, nothing else.
    pub const RESULT: Self = Self {
        end: End::Exit,
        stderr: None,
        files: None,
        env: &[],
    };

    /// An ordinary case that also compared stderr.
    #[must_use]
    pub const fn stderr(stderr: &'static [u8]) -> Self {
        Self {
            stderr: Some(stderr),
            ..Self::RESULT
        }
    }

    /// A reproduced abort with the given stderr.
    #[must_use]
    pub const fn aborts(stderr: &'static [u8]) -> Self {
        Self {
            end: End::Aborts,
            stderr: Some(stderr),
            files: None,
            env: &[],
        }
    }

    /// The same expectation with the user-directory listing compared.
    #[must_use]
    pub const fn with_files(self, files: &'static [(&'static str, UserFile)]) -> Self {
        Self {
            files: Some(files),
            ..self
        }
    }

    /// The same expectation with extra child environment.
    #[must_use]
    pub const fn with_env(self, env: &'static [(&'static str, &'static str)]) -> Self {
        Self { env, ..self }
    }
}

/// One entry of the user-directory listing (`contract_execution.user_dir_listing`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserFile {
    /// `user.conf`, its newlines written `|`.
    Conf(&'static str),
    /// A chunk log or `.tmp` file: its size and SHA-256.
    Chunk(u64, &'static str),
    /// A DBM container or other file: its size.
    Size(u64),
    /// Not a regular file.
    NotFile,
}

/// Runs a case whose body returns a result: `name` is the test's full libtest
/// name, `expected` the result and `logs` the GLib records the body leaves
/// (`None` where the old harness did not compare them: a pin crash site's
/// declared fields).
pub fn run<T: Debug + PartialEq>(
    name: &str,
    expect: Expect,
    body: impl FnOnce(&mut Kit) -> T,
    expected: T,
    logs: Option<&[Log]>,
) {
    if let Some(scratch) = child_scratch(name) {
        let mut kit = Kit::new(&scratch);
        let result = body(&mut kit);
        assert_eq!(result, expected, "{name}: result");
        if let Some(logs) = logs {
            assert_eq!(kit.logs(), logs, "{name}: logs");
        }
        std::process::exit(0);
    }
    parent(name, expect);
}

/// Runs a case whose body must not return: a reproduced abort.
pub fn run_aborting(name: &str, expect: Expect, body: impl FnOnce(&mut Kit)) {
    if let Some(scratch) = child_scratch(name) {
        let mut kit = Kit::new(&scratch);
        body(&mut kit);
        // Reached only when the body did not abort.
        std::process::exit(0);
    }
    parent(name, expect);
}

/// The case this child runs, for [`isolated`] probes.
static CASE: Mutex<Option<String>> = Mutex::new(None);

/// In the child for `name`: marks the run and answers its scratch directory.
fn child_scratch(name: &str) -> Option<PathBuf> {
    if std::env::var_os(CHILD).is_none_or(|child| child != name) {
        return None;
    }
    let scratch = PathBuf::from(std::env::var_os(SCRATCH).expect("the child's scratch"));
    std::fs::write(scratch.join(RAN), name).expect("mark the run");
    *CASE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(name.to_owned());
    Some(scratch)
}

/// The environment variable naming the probe a replaying child answers.
const PROBE: &str = "OXPINYIN_CONTRACT_PROBE";
/// The file a probe's answer is written to.
const PROBE_ANSWER: &str = "probe";
/// The file holding the earlier probes' answers, which a replay returns instead
/// of running those probes again.
const PROBE_EARLIER: &str = "probe-earlier";
/// The file a replay writes just before it runs its target probe: an abort is
/// the target's answer only once the target was reached.
const PROBE_ENTERED: &str = "probe-entered";
/// How long a probe's replay may run.
const PROBE_LIMIT: Duration = Duration::from_secs(120);
/// The probes this child has run so far.
static PROBES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// The answers of this child's probes so far, in order.
static ANSWERS: Mutex<Vec<Option<Vec<u8>>>> = Mutex::new(Vec::new());

/// Runs `probe` where the pin's answer may be an abort (`contract_abi`'s forked
/// sentence probes): `None` when it aborts, else its encoded answer. The probe
/// runs in a nested child that replays the case to this point, so an abort ends
/// that child only. The replay does not run the earlier probes again: the case's
/// child hands it their answers, and it returns those, `None` for an abort, as
/// the case's child saw them. An abort is the target's `None` only once the
/// replay has written [`PROBE_ENTERED`]; any other abort fails the harness.
pub fn isolated(probe: impl FnOnce() -> Vec<u8>) -> Option<Vec<u8>> {
    let sequence = PROBES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if let Some(target) = std::env::var_os(PROBE) {
        return replayed(sequence, &target, probe);
    }
    let answer = probe_child(sequence);
    ANSWERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(answer.clone());
    answer
}

/// In a replay: an earlier probe's recorded answer, or the target probe's run,
/// whose answer is written before the replay exits.
fn replayed(
    sequence: usize,
    target: &std::ffi::OsStr,
    probe: impl FnOnce() -> Vec<u8>,
) -> Option<Vec<u8>> {
    let target: usize = target
        .to_str()
        .and_then(|text| text.parse().ok())
        .expect("probe number");
    let scratch = PathBuf::from(std::env::var_os(SCRATCH).expect("the child's scratch"));
    if sequence < target {
        let earlier = std::fs::read(scratch.join(PROBE_EARLIER)).expect("the earlier answers");
        return decode_answers(&earlier)
            .into_iter()
            .nth(sequence)
            .expect("an earlier probe's answer");
    }
    assert_eq!(sequence, target, "the replay passed its target probe");
    std::fs::write(scratch.join(PROBE_ENTERED), b"").expect("mark the target probe");
    let answer = probe();
    std::fs::write(scratch.join(PROBE_ANSWER), &answer).expect("write the probe's answer");
    std::process::exit(0);
}

/// In the case's child: probe `sequence`'s answer from a replaying child.
fn probe_child(sequence: usize) -> Option<Vec<u8>> {
    let name = CASE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .expect("a probe runs inside a case");
    let scratch = tempfile::Builder::new()
        .prefix("probe-")
        .tempdir()
        .expect("probe scratch");
    let earlier = ANSWERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(earlier.len(), sequence, "{name}: the probes' answers");
    std::fs::write(scratch.path().join(PROBE_EARLIER), encode_answers(&earlier))
        .expect("write the earlier answers");
    let mut command = std::process::Command::new(std::env::current_exe().expect("test binary"));
    command
        .args(["--exact", &name, "--test-threads=1"])
        .env(CHILD, &name)
        .env(SCRATCH, scratch.path())
        .env(PROBE, sequence.to_string())
        .env(if cfg!(windows) { "TMP" } else { "TMPDIR" }, scratch.path())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let what = format!("{name}: probe {sequence}");
    let status = Bounded::spawn(&what, &mut command, PROBE_LIMIT)
        .wait()
        .unwrap_or_else(|overdue| panic!("{overdue}"));
    if aborted(status) {
        assert!(
            scratch.path().join(PROBE_ENTERED).exists(),
            "{what}: the replay aborted before it reached the probe ({status:?})"
        );
        return None;
    }
    assert!(status.success(), "{what} failed ({status:?})");
    Some(std::fs::read(scratch.path().join(PROBE_ANSWER)).expect("the probe's answer"))
}

/// Probe answers as bytes: per answer `N`, or `S`, its length (`u64`, little
/// endian) and its bytes.
fn encode_answers(answers: &[Option<Vec<u8>>]) -> Vec<u8> {
    let mut out = Vec::new();
    for answer in answers {
        match answer {
            None => out.push(b'N'),
            Some(bytes) => {
                out.push(b'S');
                out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
                out.extend_from_slice(bytes);
            }
        }
    }
    out
}

/// The answers [`encode_answers`] wrote.
fn decode_answers(mut bytes: &[u8]) -> Vec<Option<Vec<u8>>> {
    let mut answers = Vec::new();
    while let Some((&tag, rest)) = bytes.split_first() {
        if tag == b'N' {
            answers.push(None);
            bytes = rest;
            continue;
        }
        assert_eq!(tag, b'S', "a probe answer's tag");
        let (length, rest) = rest.split_at(8);
        let length = usize::try_from(u64::from_le_bytes(length.try_into().expect("8 bytes")))
            .expect("a probe answer's length");
        let (answer, rest) = rest.split_at(length);
        answers.push(Some(answer.to_vec()));
        bytes = rest;
    }
    answers
}

/// How often [`Bounded::wait`] looks at its child.
const POLL: Duration = Duration::from_millis(10);

/// A nested child process with a deadline. [`Bounded::wait`] kills and reaps
/// it once the deadline passes or waiting fails; one dropped while it still
/// runs (a panic between spawn and wait) is killed and reaped too.
pub struct Bounded {
    child: Option<std::process::Child>,
    what: String,
    deadline: std::time::Instant,
}

/// A nested child that outlived its deadline, or that could not be waited for:
/// it was killed, and `reaped` is the wait that followed.
#[derive(Debug)]
pub struct Overdue {
    /// What the child was.
    pub what: String,
    /// The error that ended the wait; `None` when the deadline passed.
    pub error: Option<std::io::Error>,
    /// The killed child's status.
    pub reaped: std::io::Result<std::process::ExitStatus>,
}

impl std::fmt::Display for Overdue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.error {
            None => write!(f, "{}: timed out; killed ({:?})", self.what, self.reaped),
            Some(error) => write!(f, "{}: {error}; killed ({:?})", self.what, self.reaped),
        }
    }
}

impl Bounded {
    /// Starts `command`, which must end within `limit`.
    pub fn spawn(what: &str, command: &mut std::process::Command, limit: Duration) -> Self {
        let child = command
            .spawn()
            .unwrap_or_else(|error| panic!("{what}: the child did not start: {error}"));
        Self {
            child: Some(child),
            what: what.to_owned(),
            deadline: std::time::Instant::now() + limit,
        }
    }

    /// The running child, to take its pipes.
    pub fn child(&mut self) -> &mut std::process::Child {
        self.child.as_mut().expect("a running child")
    }

    /// The child's status once it has exited; `None` while it runs within its
    /// deadline.
    pub fn poll(&mut self) -> Result<Option<std::process::ExitStatus>, Overdue> {
        match self.child().try_wait() {
            Ok(Some(status)) => {
                self.child = None;
                Ok(Some(status))
            }
            Ok(None) if std::time::Instant::now() < self.deadline => Ok(None),
            Ok(None) => Err(self.overdue(None)),
            Err(error) => Err(self.overdue(Some(error))),
        }
    }

    /// Waits for the child within its deadline.
    pub fn wait(mut self) -> Result<std::process::ExitStatus, Overdue> {
        loop {
            if let Some(status) = self.poll()? {
                return Ok(status);
            }
            std::thread::sleep(POLL);
        }
    }

    /// Kills the child and reaps it.
    pub fn kill(mut self) -> std::io::Result<std::process::ExitStatus> {
        kill_and_reap(self.child.take().expect("a running child"))
    }

    fn overdue(&mut self, error: Option<std::io::Error>) -> Overdue {
        let reaped = kill_and_reap(self.child.take().expect("a running child"));
        Overdue {
            what: self.what.clone(),
            error,
            reaped,
        }
    }
}

impl Drop for Bounded {
    fn drop(&mut self) {
        if let Some(child) = self.child.take() {
            let _ = kill_and_reap(child);
        }
    }
}

/// Kills `child` (it may have exited already) and waits for it.
fn kill_and_reap(mut child: std::process::Child) -> std::io::Result<std::process::ExitStatus> {
    let _ = child.kill();
    child.wait()
}

/// Reads `pipe` to its end on a thread: the bytes arrive on the receiver.
fn drain(mut pipe: impl std::io::Read + Send + 'static) -> std::sync::mpsc::Receiver<Vec<u8>> {
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        let _ = send.send(bytes);
    });
    receive
}

/// How long a child's output may stay open once the child has exited.
const OUTPUT_GRACE: Duration = Duration::from_secs(30);

/// Runs the case's child within [`CHILD_LIMIT`], with its stdout and stderr.
fn case_child(name: &str, command: &mut std::process::Command) -> std::process::Output {
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = Bounded::spawn(name, command, CHILD_LIMIT);
    let stdout = drain(child.child().stdout.take().expect("the child's stdout"));
    let stderr = drain(child.child().stderr.take().expect("the child's stderr"));
    let status = child.wait().unwrap_or_else(|overdue| panic!("{overdue}"));
    let collected = |receiver: std::sync::mpsc::Receiver<Vec<u8>>| {
        receiver
            .recv_timeout(OUTPUT_GRACE)
            .unwrap_or_else(|error| panic!("{name}: the child's output stayed open: {error}"))
    };
    std::process::Output {
        status,
        stdout: collected(stdout),
        stderr: collected(stderr),
    }
}

fn parent(name: &str, expect: Expect) {
    let scratch = tempfile::Builder::new()
        .prefix("contract-")
        .tempdir()
        .expect("case scratch");
    let mut command = std::process::Command::new(std::env::current_exe().expect("test binary"));
    command
        .args(["--exact", name, "--test-threads=1"])
        .env(CHILD, name)
        .env(SCRATCH, scratch.path())
        .env(if cfg!(windows) { "TMP" } else { "TMPDIR" }, scratch.path());
    for (key, value) in expect.env {
        command.env(key, value);
    }
    let output = case_child(name, &mut command);
    let report = || {
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    };
    assert!(
        std::fs::read(scratch.path().join(RAN)).is_ok_and(|ran| ran == name.as_bytes()),
        "{name}: the child ran no test of this name: {}",
        report()
    );
    match expect.end {
        End::Exit => assert!(
            output.status.success(),
            "{name}: the child failed ({:?}): {}",
            output.status,
            report()
        ),
        End::Aborts => assert!(
            aborted(output.status),
            "{name} must abort ({:?}): {}",
            output.status,
            report()
        ),
    }
    if let Some(stderr) = expect.stderr {
        let shown = scratch_relative(&output.stderr, scratch.path());
        assert_eq!(
            String::from_utf8_lossy(&shown),
            String::from_utf8_lossy(stderr),
            "{name}: stderr"
        );
        assert_eq!(shown, stderr, "{name}: stderr bytes");
    }
    if let Some(files) = expect.files {
        let files: Vec<(String, UserFile)> = files
            .iter()
            .map(|(file, entry)| ((*file).to_owned(), *entry))
            .collect();
        assert_eq!(
            listing(&scratch.path().join("user-X")),
            files,
            "{name}: user files"
        );
    }
}

/// SIGABRT on Unix; the fast-fail `std::process::abort` raises on Windows.
fn aborted(status: std::process::ExitStatus) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::process::ExitStatusExt::signal(&status) == Some(6)
    }
    #[cfg(windows)]
    {
        // `__fastfail` raises STATUS_STACK_BUFFER_OVERRUN (0xC0000409).
        status.code() == Some(i32::from_ne_bytes(0xC000_0409_u32.to_ne_bytes()))
    }
}

/// `stderr` with the scratch directory written `SCRATCH`, as the old runner showed it.
fn scratch_relative(stderr: &[u8], scratch: &Path) -> Vec<u8> {
    let root = path_c(scratch);
    let root = root.as_bytes();
    let mut out = Vec::with_capacity(stderr.len());
    let mut rest = stderr;
    while let Some(at) = rest.windows(root.len()).position(|window| window == root) {
        out.extend_from_slice(&rest[..at]);
        out.extend_from_slice(b"SCRATCH");
        rest = &rest[at + root.len()..];
    }
    out.extend_from_slice(rest);
    out
}

/// The user directory as the old runner listed it, in name order.
fn listing(user: &Path) -> Vec<(String, UserFile)> {
    let mut names: Vec<_> = std::fs::read_dir(user)
        .expect("the user directory")
        .map(|entry| {
            entry
                .expect("user entry")
                .file_name()
                .into_string()
                .expect("UTF-8 name")
        })
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let path = user.join(&name);
            let entry = if !path.is_file() {
                UserFile::NotFile
            } else if name == "user.conf" {
                let text = std::fs::read_to_string(&path)
                    .expect("user.conf")
                    .replace('\n', "|");
                UserFile::Conf(Box::leak(text.into_boxed_str()))
            } else {
                let size = std::fs::metadata(&path).expect("size").len();
                if name.ends_with(".tmp")
                    || (!name.starts_with("user_")
                        && (name.ends_with(".bin") || name.ends_with(".dbin")))
                {
                    let digest = sha256_hex(&std::fs::read(&path).expect("chunk"));
                    UserFile::Chunk(size, Box::leak(digest.into_boxed_str()))
                } else {
                    UserFile::Size(size)
                }
            };
            (name, entry)
        })
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A large expected value as static data: strings, integers and nested slices,
/// which compile as constant data rather than as construction code.
#[derive(Debug)]
pub enum Raw {
    /// A `bool`.
    B(bool),
    /// Any integer.
    I(i64),
    /// A string.
    S(&'static str),
    /// A byte vector.
    Y(&'static [u8]),
    /// A tuple, struct, vector or map (a slice of key-value pairs), or a
    /// present `Option` (one element) or enum variant (the index, then its fields).
    L(&'static [Raw]),
    /// An absent `Option`.
    Nil,
}

impl Raw {
    fn items(&self) -> &'static [Raw] {
        match self {
            Raw::L(items) => items,
            other => panic!("a fixture shape mismatch: {other:?} is not a list"),
        }
    }
}

/// A typed value built from its [`Raw`] fixture data.
pub trait Fixture: Sized {
    /// Builds the value.
    fn build(raw: &Raw) -> Self;
}

/// The typed expectation a fixture file's static data describes.
pub fn fixture<T: Fixture>(raw: &Raw) -> T {
    T::build(raw)
}

impl Fixture for bool {
    fn build(raw: &Raw) -> Self {
        match raw {
            Raw::B(value) => *value,
            other => panic!("a fixture shape mismatch: {other:?} is not a bool"),
        }
    }
}

macro_rules! fixture_integers {
    ($($t:ty),*) => {$(
        impl Fixture for $t {
            fn build(raw: &Raw) -> Self {
                match raw {
                    Raw::I(value) => <$t>::try_from(*value).expect("a fixture integer in range"),
                    other => panic!("a fixture shape mismatch: {other:?} is not an integer"),
                }
            }
        }
    )*};
}
fixture_integers!(u8, u16, u32, u64, usize, i32, i64);

impl Fixture for &'static str {
    fn build(raw: &Raw) -> Self {
        match raw {
            Raw::S(value) => value,
            other => panic!("a fixture shape mismatch: {other:?} is not a string"),
        }
    }
}

impl Fixture for String {
    fn build(raw: &Raw) -> Self {
        <&str>::build(raw).to_owned()
    }
}

impl<T: Fixture> Fixture for Vec<T> {
    fn build(raw: &Raw) -> Self {
        match raw {
            Raw::Y(bytes) => bytes
                .iter()
                .map(|byte| T::build(&Raw::I(i64::from(*byte))))
                .collect(),
            _ => raw.items().iter().map(T::build).collect(),
        }
    }
}

impl<T: Fixture> Fixture for Option<T> {
    fn build(raw: &Raw) -> Self {
        match raw {
            Raw::Nil => None,
            _ => match raw.items() {
                [value] => Some(T::build(value)),
                other => panic!("a fixture shape mismatch: Some of {} items", other.len()),
            },
        }
    }
}

impl<T: Fixture> Fixture for std::collections::BTreeMap<String, T> {
    fn build(raw: &Raw) -> Self {
        raw.items().iter().map(<(String, T)>::build).collect()
    }
}

macro_rules! fixture_tuples {
    ($(($($t:ident $i:tt),+)),*) => {$(
        impl<$($t: Fixture),+> Fixture for ($($t,)+) {
            fn build(raw: &Raw) -> Self {
                let items = raw.items();
                assert_eq!(items.len(), [$($i),+].len(), "a fixture tuple's length");
                ($($t::build(&items[$i]),)+)
            }
        }
    )*};
}
fixture_tuples!(
    (A 0),
    (A 0, B 1),
    (A 0, B 1, C 2),
    (A 0, B 1, C 2, D 3),
    (A 0, B 1, C 2, D 3, E 4),
    (A 0, B 1, C 2, D 3, E 4, F 5)
);

impl Fixture for Log {
    fn build(raw: &Raw) -> Self {
        let (domain, level) = Fixture::build(raw);
        Log(domain, level)
    }
}

/// [`Fixture`] for a struct: its fields in declaration order.
#[allow(unused_macros)] // targets without fixture files never use it
macro_rules! fixture_struct {
    ($t:path { $($field:ident),+ $(,)? }) => {
        impl $crate::support::Fixture for $t {
            fn build(raw: &$crate::support::Raw) -> Self {
                let mut fields = match raw {
                    $crate::support::Raw::L(items) => items.iter(),
                    other => panic!("a fixture shape mismatch: {other:?} is not a struct"),
                };
                let value = Self {
                    $($field: $crate::support::Fixture::build(
                        fields.next().expect("a fixture struct field"),
                    ),)+
                };
                assert!(fields.next().is_none(), "a fixture struct's field count");
                value
            }
        }
    };
}

/// [`Fixture`] for an enum: the variant's index in this list, then its fields.
#[allow(unused_macros)] // targets without fixture files never use it
macro_rules! fixture_enum {
    ($t:path { $($variant:ident($($field:ident),*)),+ $(,)? }) => {
        impl $crate::support::Fixture for $t {
            #[allow(unused_assignments, unused_mut, unused_variables)]
            fn build(raw: &$crate::support::Raw) -> Self {
                let items = match raw {
                    $crate::support::Raw::L(items) => *items,
                    other => panic!("a fixture shape mismatch: {other:?} is not an enum"),
                };
                let (tag, fields) = items.split_first().expect("a fixture enum's variant index");
                let tag: usize = $crate::support::Fixture::build(tag);
                let mut index = 0;
                $(
                    if tag == index {
                        let mut fields = fields.iter();
                        let value = Self::$variant($({
                            let $field = $crate::support::Fixture::build(
                                fields.next().expect("a fixture enum field"),
                            );
                            $field
                        }),*);
                        assert!(fields.next().is_none(), "a fixture enum's field count");
                        return value;
                    }
                    index += 1;
                )+
                panic!("a fixture enum index out of range: {tag}")
            }
        }
    };
}
