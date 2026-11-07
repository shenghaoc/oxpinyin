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

/// A file name as the old runner's text: UTF-8, with each byte that is not
/// valid UTF-8 written as Python's `surrogateescape` code point, spelled `\udcXX`.
pub fn os_text(name: &std::ffi::OsStr) -> String {
    surrogate_text(name.as_encoded_bytes())
}

/// Bytes as the old runner's text ([`os_text`]): valid UTF-8 as it is, each
/// other byte as `\udcXX`.
pub fn surrogate_text(bytes: &[u8]) -> String {
    let mut text = String::new();
    for chunk in bytes.utf8_chunks() {
        text.push_str(chunk.valid());
        for byte in chunk.invalid() {
            text.push_str(&format!("\\udc{byte:02x}"));
        }
    }
    text
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
/// The probes this child has run so far.
static PROBES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Runs `probe` where the pin's answer may be an abort (`contract_abi`'s forked
/// sentence probes): `None` when it aborts, else its encoded answer. The probe
/// runs in a nested child that replays the case to this point, so an abort ends
/// that child only. The probes before it ran in this child and returned, so the
/// replay reaches the same state; in the replay they run in-process.
pub fn isolated(probe: impl FnOnce() -> Vec<u8>) -> Option<Vec<u8>> {
    let sequence = PROBES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if let Some(target) = std::env::var_os(PROBE) {
        let target: usize = target
            .to_str()
            .and_then(|text| text.parse().ok())
            .expect("probe number");
        let answer = probe();
        if sequence == target {
            let scratch = PathBuf::from(std::env::var_os(SCRATCH).expect("the child's scratch"));
            std::fs::write(scratch.join(PROBE_ANSWER), &answer).expect("write the probe's answer");
            std::process::exit(0);
        }
        return Some(answer);
    }
    let name = CASE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .expect("a probe runs inside a case");
    let scratch = tempfile::Builder::new()
        .prefix("probe-")
        .tempdir()
        .expect("probe scratch");
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", &name, "--test-threads=1"])
        .env(CHILD, &name)
        .env(SCRATCH, scratch.path())
        .env(PROBE, sequence.to_string())
        .env(if cfg!(windows) { "TMP" } else { "TMPDIR" }, scratch.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("the probe child");
    if aborted(output) {
        return None;
    }
    assert!(
        output.success(),
        "{name}: probe {sequence} failed ({output:?})"
    );
    Some(std::fs::read(scratch.path().join(PROBE_ANSWER)).expect("the probe's answer"))
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
    let started = std::time::Instant::now();
    let output = assert_cmd::Command::from_std(command)
        .timeout(CHILD_LIMIT)
        .output()
        .unwrap_or_else(|error| panic!("{name}: the child did not start: {error}"));
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
    assert!(
        started.elapsed() < CHILD_LIMIT,
        "{name}: the child timed out"
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
        assert_eq!(listing(scratch.path()), files, "{name}: user files");
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

/// The user directories as the old runner listed them
/// (`contract_execution.user_dir_listing`): every `user-*` directory under the
/// scratch directory, in name order, flattened by file name so that a later
/// directory's file replaces an earlier one's; the result in name order.
fn listing(scratch: &Path) -> Vec<(String, UserFile)> {
    let mut directories: Vec<PathBuf> = std::fs::read_dir(scratch)
        .expect("the scratch directory")
        .map(|entry| entry.expect("scratch entry").path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .is_some_and(|name| name.as_encoded_bytes().starts_with(b"user-"))
        })
        .collect();
    directories.sort();
    let mut files = std::collections::BTreeMap::new();
    for user in directories {
        let mut names: Vec<_> = std::fs::read_dir(&user)
            .expect("the user directory")
            .map(|entry| entry.expect("user entry").file_name())
            .collect();
        names.sort();
        for name in names {
            let path = user.join(&name);
            files.insert(os_text(&name), path);
        }
    }
    files
        .into_iter()
        .map(|(name, path)| {
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
    /// A [`J`] map: its keys in order and their values.
    M(&'static [(&'static str, Raw)]),
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

/// A case's result as the old harness's JSON document held it: what the
/// families whose bodies return a document (`run_j`) compare. A Python tuple is
/// a list; a string that held a byte that is not UTF-8 spells it `\udcXX`
/// ([`surrogate_text`]).
#[derive(Clone, PartialEq, Eq)]
pub enum J {
    /// `null`: Python's `None`.
    Null,
    /// A boolean.
    B(bool),
    /// An integer.
    I(i64),
    /// A string.
    S(String),
    /// A list.
    L(Vec<J>),
    /// An object, by key.
    M(std::collections::BTreeMap<String, J>),
}

impl Debug for J {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            J::Null => f.write_str("null"),
            J::B(value) => write!(f, "{value}"),
            J::I(value) => write!(f, "{value}"),
            J::S(value) => write!(f, "{value:?}"),
            J::L(items) => {
                f.write_str("[")?;
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{item:?}")?;
                }
                f.write_str("]")
            }
            J::M(map) => {
                f.write_str("{")?;
                for (index, (key, value)) in map.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{key:?}: {value:?}")?;
                }
                f.write_str("}")
            }
        }
    }
}

impl J {
    /// An empty object.
    #[must_use]
    pub fn map() -> Self {
        J::M(std::collections::BTreeMap::new())
    }

    /// Sets `key` in an object.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<J>) {
        match self {
            J::M(map) => {
                map.insert(key.into(), value.into());
            }
            other => panic!("set {:?} on a non-object {other:?}", key.into()),
        }
    }

    /// The value of `key` in an object.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&J> {
        match self {
            J::M(map) => map.get(key),
            _ => None,
        }
    }

    /// A list's items.
    #[must_use]
    pub fn items(&self) -> &[J] {
        match self {
            J::L(items) => items,
            other => panic!("{other:?} is not a list"),
        }
    }

    /// An integer's value.
    #[must_use]
    pub fn int(&self) -> i64 {
        match self {
            J::I(value) => *value,
            other => panic!("{other:?} is not an integer"),
        }
    }

    /// Python's truth value of the JSON value.
    #[must_use]
    pub fn truthy(&self) -> bool {
        match self {
            J::Null => false,
            J::B(value) => *value,
            J::I(value) => *value != 0,
            J::S(value) => !value.is_empty(),
            J::L(items) => !items.is_empty(),
            J::M(map) => !map.is_empty(),
        }
    }
}

impl From<J> for String {
    fn from(value: J) -> Self {
        match value {
            J::S(text) => text,
            other => panic!("{other:?} is not a string"),
        }
    }
}

impl From<bool> for J {
    fn from(value: bool) -> Self {
        J::B(value)
    }
}

macro_rules! j_integers {
    ($($t:ty),*) => {$(
        impl From<$t> for J {
            fn from(value: $t) -> Self {
                J::I(i64::try_from(value).expect("an integer in i64's range"))
            }
        }
    )*};
}
j_integers!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

impl From<&str> for J {
    fn from(value: &str) -> Self {
        J::S(value.to_owned())
    }
}

impl From<String> for J {
    fn from(value: String) -> Self {
        J::S(value)
    }
}

impl From<&String> for J {
    fn from(value: &String) -> Self {
        J::S(value.clone())
    }
}

impl<T: Into<J>> From<Option<T>> for J {
    fn from(value: Option<T>) -> Self {
        value.map_or(J::Null, Into::into)
    }
}

impl<T: Into<J>> From<Vec<T>> for J {
    fn from(value: Vec<T>) -> Self {
        J::L(value.into_iter().map(Into::into).collect())
    }
}

impl<T: Into<J> + Clone> From<&[T]> for J {
    fn from(value: &[T]) -> Self {
        J::L(value.iter().cloned().map(Into::into).collect())
    }
}

impl<T: Into<J>> From<std::collections::BTreeMap<String, T>> for J {
    fn from(value: std::collections::BTreeMap<String, T>) -> Self {
        J::M(
            value
                .into_iter()
                .map(|(key, item)| (key, item.into()))
                .collect(),
        )
    }
}

macro_rules! j_tuples {
    ($(($($t:ident $i:tt),+)),*) => {$(
        impl<$($t: Into<J>),+> From<($($t,)+)> for J {
            fn from(value: ($($t,)+)) -> Self {
                J::L(vec![$(value.$i.into()),+])
            }
        }
    )*};
}
j_tuples!(
    (A 0),
    (A 0, B 1),
    (A 0, B 1, C 2),
    (A 0, B 1, C 2, D 3),
    (A 0, B 1, C 2, D 3, E 4),
    (A 0, B 1, C 2, D 3, E 4, F 5),
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6),
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7)
);

impl From<Log> for J {
    fn from(value: Log) -> Self {
        J::L(vec![value.0.into(), value.1.into()])
    }
}

impl Fixture for J {
    fn build(raw: &Raw) -> Self {
        match raw {
            Raw::Nil => J::Null,
            Raw::B(value) => J::B(*value),
            Raw::I(value) => J::I(*value),
            Raw::S(value) => J::S((*value).to_owned()),
            Raw::Y(bytes) => J::L(bytes.iter().map(|byte| J::I(i64::from(*byte))).collect()),
            Raw::L(items) => J::L(items.iter().map(J::build).collect()),
            Raw::M(pairs) => J::M(
                pairs
                    .iter()
                    .map(|(key, value)| ((*key).to_owned(), J::build(value)))
                    .collect(),
            ),
        }
    }
}

/// A [`J`] object: `obj! { "key" => value, ... }`.
#[allow(unused_macros)]
macro_rules! obj {
    ($($key:expr => $value:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut object = $crate::support::J::map();
        $( object.set($key, $value); )*
        object
    }};
}

/// The [`J`] a fixture's static data describes: `raw!(L(&[...]))` or
/// `raw!(include!("fixtures/<family>/<digest>.rs"))`.
#[allow(unused_macros)]
macro_rules! raw {
    ($raw:expr) => {{
        #[allow(unused_imports)]
        use $crate::support::Raw::*;
        static RAW: $crate::support::Raw = $raw;
        $crate::support::fixture::<$crate::support::J>(&RAW)
    }};
}

/// The GLib records so far as the old worker added them to every result
/// document (`result['logs']`): `[domain or null, level]` pairs.
pub fn logs_j(kit: &Kit) -> J {
    J::L(kit.logs().into_iter().map(J::from).collect())
}

/// The messages so far (`result['log_messages']`, the axis C cases).
pub fn messages_j(kit: &Kit) -> J {
    J::L(kit.messages().into_iter().map(J::from).collect())
}

/// Runs a case whose body returns its result document, as the old worker ran
/// it: the body's object gains `logs` ([`logs_j`]), and its keys that start with
/// `~` are left out of the comparison (`contract_verdict.observed`). With
/// `keys`, only those keys are compared: a pin crash site's or a not-run pin's
/// declared fields.
pub fn run_j(
    name: &str,
    expect: Expect,
    body: impl FnOnce(&mut Kit) -> J,
    expected: J,
    keys: Option<&[&str]>,
) {
    if let Some(scratch) = child_scratch(name) {
        let mut kit = Kit::new(&scratch);
        let mut result = body(&mut kit);
        result.set("logs", logs_j(&kit));
        let J::M(map) = result else {
            panic!("{name}: the body's result is not an object")
        };
        let observed: std::collections::BTreeMap<String, J> = map
            .into_iter()
            .filter(|(key, _)| !key.starts_with('~'))
            .filter(|(key, _)| keys.is_none_or(|keys| keys.contains(&key.as_str())))
            .collect();
        assert_eq!(J::M(observed), expected, "{name}: result");
        std::process::exit(0);
    }
    parent(name, expect);
}

/// File descriptor 2 redirected to an anonymous file until [`StderrCapture::finish`].
pub struct StderrCapture {
    saved: i32,
    file: std::fs::File,
}

impl StderrCapture {
    /// Redirects file descriptor 2.
    #[cfg(unix)]
    #[must_use]
    pub fn start() -> Self {
        use std::os::fd::AsRawFd as _;
        let file = tempfile::tempfile().expect("a stderr capture file");
        // SAFETY: duplicates the process's own stderr descriptor; `finish` restores
        // and closes the copy.
        let saved = unsafe { libc::dup(2) };
        assert!(saved >= 0, "dup stderr");
        // SAFETY: both descriptors are open for the call.
        let redirected = unsafe { libc::dup2(file.as_raw_fd(), 2) };
        assert!(redirected >= 0, "redirect stderr");
        Self { saved, file }
    }

    /// Redirects file descriptor 2.
    #[cfg(not(unix))]
    #[must_use]
    pub fn start() -> Self {
        unimplemented!("the stderr capture uses dup2, which this platform lacks")
    }

    /// Restores stderr and answers what was written, as the old runner decoded it.
    #[must_use]
    pub fn finish(mut self) -> String {
        use std::io::{Read as _, Seek as _};
        // SAFETY: `saved` is the stderr descriptor `start` duplicated, restored and
        // closed once.
        unsafe {
            assert!(libc::dup2(self.saved, 2) >= 0, "restore stderr");
            libc::close(self.saved);
        }
        let mut bytes = Vec::new();
        self.file
            .seek(std::io::SeekFrom::Start(0))
            .expect("rewind the capture");
        self.file.read_to_end(&mut bytes).expect("read the capture");
        surrogate_text(&bytes)
    }
}

/// What a libpinyin export ([`libpinyin_export`]) answered.
#[derive(Debug)]
pub struct Export {
    /// What it wrote to stderr, with the user directory written `USER`.
    pub stderr: String,
    /// Its document: `rows`, `logs` and `log_messages`.
    pub document: J,
}

/// Exports user library 7 of a saved user directory through libpinyin's C API
/// (`crate::libpinyin`), as the old `axis-c-export-user` worker did: libzhuyin
/// has no export iterator. Opens `data` and `user`, walks
/// `pinyin_begin_get_phrases(context, 7)` as `[ret, phrase, reading, count]` rows
/// and leaves the context open, as the worker did. The document's `logs` and
/// `log_messages` are the export's own: the case's records before it are set
/// aside and restored after it, and the export's are not added to them.
pub fn libpinyin_export(data: &Path, user: &Path) -> Export {
    let before = std::mem::take(
        &mut *LOGS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    let capture = StderrCapture::start();
    let system = path_c(data);
    let user_c = path_c(user);
    let ctx = crate::libpinyin::init(system.as_ptr(), user_c.as_ptr());
    assert!(!ctx.is_null(), "the export's init failed");
    let iter = crate::libpinyin::begin_get_phrases(ctx, 7);
    let mut rows = Vec::new();
    while crate::libpinyin::iterator_has_next_phrase(iter) {
        let mut phrase: *mut c_char = std::ptr::null_mut();
        let mut reading: *mut c_char = std::ptr::null_mut();
        let mut count = i32::from_ne_bytes(UNTOUCHED.to_ne_bytes());
        let ret = crate::libpinyin::iterator_get_next_phrase(
            iter,
            &raw mut phrase,
            &raw mut reading,
            &raw mut count,
        );
        rows.push(J::from((ret, take_text(phrase), take_text(reading), count)));
    }
    crate::libpinyin::end_get_phrases(iter);
    let stderr = capture.finish();
    let records = std::mem::replace(
        &mut *LOGS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        before,
    );
    let logs: Vec<J> = records
        .iter()
        .map(|(domain, level, _)| J::from((domain.clone(), *level)))
        .collect();
    let messages: Vec<J> = records
        .into_iter()
        .map(|(_, _, message)| J::from(message))
        .collect();
    let mut document = J::map();
    document.set("rows", rows);
    document.set("logs", logs);
    document.set("log_messages", messages);
    let user_text = surrogate_text(path_c(user).as_bytes());
    Export {
        stderr: stderr.replace(&user_text, "USER"),
        document,
    }
}
