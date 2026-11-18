use super::EngineError;

#[test]
fn only_a_toned_first_initial_only_key_is_the_table_assert() {
    let key = |text| crate::SyllableKey::from_text(text).expect("key");
    let (n, ni, h) = (key("n"), key("ni"), key("h"));
    assert_eq!(
        EngineError::check_toned_incomplete(&[n], &[4]),
        Err(EngineError::TonedIncompleteKey)
    );
    assert_eq!(
        EngineError::check_toned_incomplete(&[ni, n], &[3, 4]),
        Err(EngineError::TonedIncompleteKey)
    );
    // `contains_incomplete_pinyin` returns at the first initial-only key
    // (`pinyin_phrase3.h:148-154`): a later toned one is never checked.
    assert_eq!(
        EngineError::check_toned_incomplete(&[h, n], &[0, 4]),
        Ok(())
    );
    assert_eq!(EngineError::check_toned_incomplete(&[n], &[0]), Ok(()));
    assert_eq!(EngineError::check_toned_incomplete(&[ni], &[4]), Ok(()));
}

#[test]
fn core_errors_convert_at_the_engine_boundary() {
    use crate::graph::GraphError;
    use crate::scoring::ScoringError;

    let graph: EngineError = GraphError::InputTooLong { len: 1, limit: 0 }.into();
    assert!(matches!(graph, EngineError::Graph(_)));
    let scoring: EngineError = ScoringError::Dictionary("closed".to_owned()).into();
    assert!(matches!(scoring, EngineError::Scoring(_)));
}

#[test]
fn wrapped_backend_errors_chain_a_source() {
    use crate::graph::GraphError;
    use std::error::Error as _;

    let graph = EngineError::Graph(GraphError::InputTooLong { len: 1, limit: 0 });
    assert!(graph.source().is_some());
    // String-erased and index variants terminate the chain.
    assert!(
        EngineError::UserModel("closed".to_owned())
            .source()
            .is_none()
    );
    assert!(
        EngineError::CandidateIndexOutOfRange { index: 9, len: 3 }
            .source()
            .is_none()
    );
}
