use super::SelectionRecord;
use crate::vocab::PhraseText;

/// Chosen phrases and raw gaps join in order, an n-best row replaces
/// the text, and a rebuild keeps the gaps its runs skip over.
#[test]
fn the_selected_text_keeps_raw_gaps() {
    let mut record = SelectionRecord::default();
    record.append_selected("", &PhraseText::from("你"));
    record.append_selected("ha", &PhraseText::from("好"));
    assert_eq!(record.selected(), &PhraseText::from("你ha好"));

    record.set_selected(&PhraseText::from("你好"));
    assert_eq!(record.selected(), &PhraseText::from("你好"));

    record.rebuild_from_constraints("nihao", &[(2, 5, 7, PhraseText::from("好"))]);
    assert_eq!(record.selected(), &PhraseText::from("ni好"));
    assert_eq!(record.history(), [7]);

    record.clear();
    assert!(record.selected().is_empty());
}
