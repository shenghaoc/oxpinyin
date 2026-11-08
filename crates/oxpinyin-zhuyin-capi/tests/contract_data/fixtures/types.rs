//! `Fixture` for the named types this target's fixture files build. Generated.

fixture_struct!(crate::bodies::before_cursor_dedup::BeforeCursor {
    parsed,
    guessed,
    ret,
    rows,
    chosen
});
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
fixture_enum!(crate::bodies::reading_256th::Entry { Item(f0), Count(f0, f1), Flag(f0), Note(f0) });
fixture_struct!(crate::bodies::reading_256th::ItemRead {
    token,
    n,
    unigram,
    nth,
    export
});
fixture_struct!(crate::readers::GuessRows { ret, rows });
