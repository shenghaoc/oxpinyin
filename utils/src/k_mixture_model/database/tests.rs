use super::attached;
use oxpinyin::store::StoreError;
use oxpinyin_tool_support::flexible_ngram::LoadError;

/// Set in the child: route a store error instead of the test body.
const CHILD: &str = "OXPINYIN_KMM_ATTACH_ERROR_CHILD";

#[test]
fn an_attach_error_is_never_a_missing_object() {
    if std::env::var_os(CHILD).is_some() {
        let _ = attached(Err(StoreError::Backend("unsupported library 6.2".into())));
        return;
    }
    let output = crate::test_child::run_exact(
        "k_mixture_model::database::tests::an_attach_error_is_never_a_missing_object",
        [(CHILD, "1")],
    );
    assert_eq!(output.status.code(), Some(1), "the utility exits");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unsupported library 6.2"), "{stderr}");
}

/// Set in the child to its scratch directory: the child builds its
/// containers there and runs the site under test instead of the body.
const CHILD_DIR: &str = "OXPINYIN_KMM_CHILD_DIR";

/// The token whose single gram these tests plant.
pub(crate) const TOKEN: u32 = 7;

/// A stored value shorter than the eight-byte array header.
pub(crate) const SHORT: &[u8] = &[1, 2, 3, 4];

/// A whole single gram: a zero array header and no items.
pub(crate) const WHOLE: &[u8] = &[0; 8];

/// The exit status of a child whose site under test panicked. A panic
/// that reaches a non-unwinding `extern "C"` function or cxx's unwind
/// guard aborts the process, and libtest's capture discards its message,
/// so without this status an unrelated panic passes as the pin's abort.
const OPERATION_PANICKED: i32 = 86;

/// The scratch directory when this process is the child that
/// [`run_child`] started. In that child it also arms the strict check:
/// from here on, a panic writes its message to fd 2 and exits with
/// [`OPERATION_PANICKED`].
pub(crate) fn child_dir() -> Option<std::path::PathBuf> {
    let dir = std::env::var_os(CHILD_DIR).map(std::path::PathBuf::from)?;
    std::panic::set_hook(Box::new(|info| {
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), "death-test operation panicked: {info}");
        std::process::exit(OPERATION_PANICKED);
    }));
    Some(dir)
}

/// Run the test `name` (its `--exact` path) again as a child with a
/// scratch [`child_dir`], and require no Rust panic text on either
/// stream: the child's libtest reports a captured panic on stdout and
/// the default hook writes it to stderr.
fn run_child(name: &str) -> std::process::Output {
    let dir = tempfile::tempdir().expect("scratch directory");
    let output = crate::test_child::run_exact(name, [(CHILD_DIR, dir.path())]);
    assert_ne!(
        output.status.code(),
        Some(OPERATION_PANICKED),
        "{name}: the operation panicked: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    for stream in [&output.stdout, &output.stderr] {
        let text = String::from_utf8_lossy(stream);
        assert!(!text.contains("panicked"), "{name}: {text}");
    }
    output
}

/// Require the child test `name` to abort where the pin asserts:
/// SIGABRT on Unix, and on Windows Rust's abort fast-fails with
/// STATUS_STACK_BUFFER_OVERRUN, as the store's death tests check.
pub(crate) fn assert_aborts(name: &str) {
    let output = run_child(name);
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        assert_eq!(output.status.signal(), Some(6), "{name}: {output:?}");
    }
    #[cfg(windows)]
    assert_eq!(
        output.status.code(),
        Some(i32::from_ne_bytes(0xC000_0409_u32.to_ne_bytes())),
        "{name}: {output:?}"
    );
}

/// Require the child test `name` to take the tools' store-error exit for
/// a short single gram: status 1 and exactly that one stderr line. The
/// line ends with the host's text newline: C's text-mode stderr writes
/// CRLF on Windows.
pub(crate) fn assert_short_single_gram_exit(name: &str) {
    let output = run_child(name);
    assert_eq!(output.status.code(), Some(1), "{name}: {output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!(
            "{}{}",
            LoadError::ShortArrayHeader,
            oxpinyin::__internal::TEXT_NEWLINE
        ),
        "{name}"
    );
}

/// Create a `KMMP` container at `dir/name` holding `rows`, written as
/// raw values, and answer its file name for `attach`.
pub(crate) fn plant(dir: &std::path::Path, name: &str, rows: &[(u32, &[u8])]) -> std::ffi::CString {
    use oxpinyin::store::{DefaultStore, WriteStore as _, WriteTxn as _};
    use oxpinyin_tool_support::flexible_ngram::MAGIC_KEY;
    let path = dir.join(name);
    let db = DefaultStore::create_hash(&path).expect("hash create");
    db.write(|txn| {
        txn.put_raw(&MAGIC_KEY, b"KMMP")?;
        for (token, value) in rows {
            txn.put_raw(&token.to_ne_bytes(), value)?;
        }
        Ok(())
    })
    .expect("planted rows");
    std::ffi::CString::new(path.into_os_string().into_encoded_bytes()).expect("path without NUL")
}

/// `check_result(load(…))` on a short single gram, as export, validate,
/// prune and estimate's deleted model make it: the pin's `load` succeeds
/// and the gram is then read outside its record, so the
/// tools take the store-error exit.
#[test]
fn a_checked_load_of_a_short_row_takes_the_store_error_exit() {
    if let Some(dir) = child_dir() {
        let model = plant(&dir, "short.db", &[(TOKEN, SHORT)]);
        let _ = super::Database::open_read_only(Some(&model)).load_checked(TOKEN);
        return;
    }
    assert_short_single_gram_exit(
        "k_mixture_model::database::tests::a_checked_load_of_a_short_row_takes_the_store_error_exit",
    );
}

/// A panic that reaches this non-unwinding function aborts the process.
extern "C" fn panic_in_a_c_boundary() {
    panic!("an unrelated panic");
}

/// The witness: a death test whose site panics at a C boundary fails
/// with its own message, not as the pin's abort.
#[test]
fn a_panic_at_the_c_boundary_is_not_the_pins_abort() {
    if child_dir().is_some() {
        panic_in_a_c_boundary();
        return;
    }
    let parent = std::panic::catch_unwind(|| {
        assert_aborts(
            "k_mixture_model::database::tests::a_panic_at_the_c_boundary_is_not_the_pins_abort",
        );
    });
    let message = parent.expect_err("a panic must not pass as an abort");
    let message = message
        .downcast_ref::<String>()
        .expect("the helper's assertion message");
    assert!(message.contains("the operation panicked"), "{message}");
    assert!(message.contains("an unrelated panic"), "{message}");
}
