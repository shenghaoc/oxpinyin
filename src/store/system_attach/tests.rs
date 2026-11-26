use rstest::rstest;

use super::{AccessMode, SystemAttachment};
use crate::store::StoreError;

// 074a2219 ngram_bdb.cpp:110-124, ngram_kyotodb.cpp:110-120, ngram_tkrzwdb.cpp:86-99 and the pre-open error contract.
#[rstest]
#[case::missing_reader("bigram.db", AccessMode::ReadOnly, false, Some(false))]
#[case::create_writer("bigram.db", AccessMode::ReadWrite, false, Some(true))]
#[case::existing_reader("bigram.db", AccessMode::ReadOnly, true, Some(true))]
#[case::invalid_path("bad\0name.db", AccessMode::ReadOnly, false, None)]
fn attach_bigram_answers_a_failed_open_apart_from_an_earlier_failure(
    #[case] name: &str,
    #[case] access: AccessMode,
    #[case] existing: bool,
    #[case] opened: Option<bool>,
) {
    let scratch = tempfile::tempdir().unwrap();
    let path = scratch.path().join(name);
    if existing {
        drop(SystemAttachment::attach_bigram(&path, AccessMode::ReadWrite).unwrap());
    }
    let result = SystemAttachment::attach_bigram(&path, access);
    if let Some(opened) = opened {
        assert_eq!(matches!(result.unwrap(), SystemAttachment::Open(_)), opened);
        assert_eq!(path.exists(), opened);
    } else {
        #[cfg(feature = "kyotocabinet")]
        let message = "store path contains NUL";
        #[cfg(not(feature = "kyotocabinet"))]
        let message = "path contains NUL";
        assert!(matches!(result, Err(StoreError::InvalidInput(reason)) if reason == message));
    }
}
