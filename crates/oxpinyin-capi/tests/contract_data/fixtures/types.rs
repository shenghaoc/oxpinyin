//! `Fixture` for the named types this target's fixture files build. Generated.

fixture_struct!(crate::bodies::before_cursor_dedup::Longer {
    parsed,
    at_end,
    without_sentence,
    guess_sentence,
    with_sentence,
    sentence_suppressed
});
fixture_struct!(crate::bodies::before_cursor_dedup::Promotion { steps, files });
fixture_enum!(crate::bodies::before_cursor_dedup::Step { Promote(f0, f1), Tokens(f0), Save(f0), Mask(f0) });
fixture_enum!(crate::bodies::before_cursor_dedup::UserFileView { Rows(f0), NonEmpty(f0), Hex(f0) });
fixture_struct!(crate::bodies::case_helpers::Trained {
    token,
    parsed,
    before,
    training,
    after
});
fixture_struct!(crate::bodies::case_helpers::Training {
    chosen,
    kind,
    cursor,
    after_choose,
    guess,
    train
});
fixture_struct!(crate::bodies::glib_arrays::Arrays {
    ret,
    original,
    refused,
    cleared,
    caller_mutation
});
fixture_struct!(crate::bodies::glib_arrays::Lifecycle { tokens, keys });
fixture_struct!(crate::bodies::glib_arrays::NullArray { found, missing });
fixture_struct!(crate::bodies::glib_arrays::NullProbe {
    ret,
    logs,
    messages
});
fixture_struct!(crate::bodies::issue542_remaining::AliasPrefix {
    with_prefix_borrowed_text,
    with_prefix,
    sentences,
    owned_prefix_recovery,
    owned_prefix_candidate_count
});
fixture_struct!(crate::bodies::issue542_remaining::TrainSave {
    parse,
    guess,
    train,
    save,
    files
});
fixture_struct!(crate::bodies::non_utf8_directories::Session {
    init,
    guess,
    train,
    save,
    user_files
});
fixture_struct!(crate::bodies::phrase_table_mask::Masked {
    loaded,
    export_before,
    masked,
    trained,
    saved,
    stored,
    export_after,
    reopened,
    export_reopened
});
fixture_struct!(crate::bodies::phrase_table_mask::NeverLoaded { lookup, prediction });
fixture_struct!(crate::bodies::phrase_table_mask::Roundtrip {
    absent,
    added,
    found,
    added_again,
    found_again,
    saved,
    reopened,
    removed,
    after_remove,
    readded,
    after_readd,
    saved_again,
    final_,
    export
});
fixture_struct!(crate::bodies::phrase_table_mask::SavedIndex {
    added,
    saved,
    user_pinyin_index,
    user_phrase_index,
    reopened,
    export
});
fixture_struct!(crate::bodies::predicted_types::PredictedTypes {
    before,
    punctuation,
    punctuation_again,
    prefix
});
fixture_enum!(crate::bodies::reading_256th::Entry { Item(f0), Count(f0, f1), Flag(f0), Note(f0) });
fixture_struct!(crate::bodies::reading_256th::ItemRead {
    token,
    n,
    unigram,
    nth,
    export
});
fixture_struct!(crate::bodies::stored_total_overflow::Headers { total, facade });
fixture_struct!(crate::bodies::stored_total_overflow::Overflow {
    before,
    add,
    after,
    facade,
    import,
    saved,
    add_saved,
    after_saved,
    facade_saved
});
fixture_struct!(crate::readers::FacadeOut {
    parsed,
    guessed,
    sentences,
    candidates,
    segment
});
fixture_struct!(crate::readers::GuessRows { ret, rows });
fixture_struct!(crate::readers::Predicted { ret, rows });
