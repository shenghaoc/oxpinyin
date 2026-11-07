//! The frozen sentence-surface fixtures bind to the declared residuals on every
//! backend: nothing here reads system data.

#[path = "common/sentence_fixture.rs"]
mod sentence_fixture;

/// Check fixture completeness and bind its subject expectations to the
/// existing frozen declaration.
#[test]
fn sentence_tie_fixture_uses_only_the_declared_residuals() {
    let _ = sentence_fixture::load_tie_cases(&sentence_fixture::repo_root());
}

#[test]
fn sentence_residual_fixture_binds_complete_pin_cases() {
    let root = sentence_fixture::repo_root();
    let pin = sentence_fixture::load_fixture(&root.join(sentence_fixture::SENTENCE_FIXTURE))
        .expect("pin fixture");
    let residuals = sentence_fixture::load_residuals(&root, &pin).expect("valid residual fixture");
    assert_eq!(residuals.len(), 9);
    for residual in residuals.values() {
        assert!(residual.guessed);
        assert!((1..=3).contains(&residual.sentences.len()));
        assert_eq!(residual.rows.len(), 6);
    }
}
