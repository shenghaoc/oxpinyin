use super::*;

struct Row(TextSpan, *const c_char);

impl SnapshotRow for Row {
    fn text_span(&self) -> TextSpan {
        self.0
    }

    fn set_text(&mut self, span: TextSpan, ptr: *const c_char) {
        self.0 = span;
        self.1 = ptr;
    }
}

fn row(span: TextSpan) -> Row {
    Row(span, std::ptr::null())
}

#[test]
fn null_text_survives_growth_and_punctuation_prepend() {
    let mut predicted = CandidateSnapshot::<Row>::new();
    predicted.push(row(TextSpan::NULL));
    predicted.reserve(4, 1024);
    assert!(predicted[0].1.is_null());
    let mut punctuation = CandidateSnapshot::<Row>::new();
    let span = punctuation.intern("。".as_bytes()).expect("punctuation");
    punctuation.push(row(span));
    punctuation.append(predicted);
    assert_eq!(punctuation.len(), 2);
    assert!(punctuation[1].1.is_null());
    assert_eq!(punctuation[1].0, TextSpan::NULL);
    let empty = punctuation.intern(b"").expect("empty string");
    punctuation.push(row(empty));
    assert!(!punctuation[2].1.is_null());
    assert!(punctuation[1].1.is_null());
}

#[test]
fn interned_strings_are_terminated_and_addressable() {
    let mut snapshot = CandidateSnapshot::<Row>::new();
    let a = snapshot.intern("你好".as_bytes()).expect("interned");
    let b = snapshot.intern(b"").expect("interned");
    snapshot.push(row(a));
    snapshot.push(row(b));
    assert_eq!(snapshot.len(), 2);
    assert_eq!(
        snapshot[0].1,
        snapshot.text_ptr(a),
        "a pushed row points at its string"
    );
    assert_eq!(snapshot.text(a).to_bytes(), "你好".as_bytes());
    assert_eq!(snapshot.text(b).to_bytes(), b"");
    assert_eq!(snapshot.text_bytes(a), "你好".as_bytes());
    assert!(
        snapshot.intern(b"a\0b").is_none(),
        "an interior NUL is refused"
    );
    assert_eq!(snapshot.text_ptr(a), snapshot.text(a).as_ptr());
    snapshot.clear();
    assert!(snapshot.is_empty());
    assert_eq!(
        snapshot.text(a).to_bytes(),
        b"",
        "a stale span reads as empty"
    );
}

#[test]
fn a_span_not_written_here_points_outside_the_buffer() {
    let in_buffer = |snapshot: &CandidateSnapshot<Row>, span| {
        snapshot
            .text
            .as_ptr_range()
            .contains(&snapshot.text_ptr(span).cast())
    };
    let mut snapshot = CandidateSnapshot::<Row>::new();
    let held = snapshot.intern("你好".as_bytes()).expect("interned");
    assert_eq!(snapshot.text_ptr(held), snapshot.text.as_ptr().cast());
    let unterminated = TextSpan { start: 0, len: 3 };
    assert!(
        !in_buffer(&snapshot, unterminated),
        "no terminator after it"
    );
    let past_end = TextSpan { start: 0, len: 100 };
    assert!(!in_buffer(&snapshot, past_end), "beyond the buffer");
    assert!(!snapshot.text_ptr(past_end).is_null());
    assert!(snapshot.text_ptr(TextSpan::NULL).is_null());
}

#[test]
fn append_re_interns_the_other_rows() {
    let mut first = CandidateSnapshot::<Row>::new();
    let x = first.intern(b"x").expect("interned");
    first.push(row(x));
    let mut second = CandidateSnapshot::<Row>::new();
    let yy = second.intern(b"yy").expect("interned");
    second.push(row(yy));
    first.append(second);
    assert_eq!(first.len(), 2);
    assert_eq!(first.text(first[0].0).to_bytes(), b"x");
    assert_eq!(first.text(first[1].0).to_bytes(), b"yy");
    assert_eq!(
        first[0].1,
        first.text_ptr(first[0].0),
        "rows follow a moved buffer"
    );
    assert_eq!(first[1].1, first.text_ptr(first[1].0));
}

#[test]
fn growth_repoints_every_row() {
    let mut snapshot = CandidateSnapshot::<Row>::new();
    for _ in 0..64 {
        let span = snapshot.intern(b"abcdefgh").expect("interned");
        snapshot.push(row(span));
    }
    for item in snapshot.iter() {
        assert_eq!(item.1, snapshot.text_ptr(item.0));
    }
}
