//! Wikitext → raw-text stripping.
//!
//! Two passes over the char stream:
//!
//! 1. `remove_tables` — deletes `{| … |}` blocks, tracking template depth
//!    so `{|`/`|}` inside `{{…}}` arguments are not mistaken for table
//!    delimiters.
//! 2. `strip_wikitext` — the main cleaner: tags, templates, links,
//!    entities, emphasis, headings, list markers.
//!
//! The rules are ordinary data engineering, documented in
//! `docs/testing/corpus-pipeline.md`; the tier-1 invariants
//! (`tests/invariants.rs`) assert no markup syntax survives. Nothing here
//! is a libpinyin reproduction; libpinyin has no corpus-prep tool.

/// Content of these paired tags is dropped entirely (non-prose).
const DROP_CONTENT_TAGS: &[&str] = &[
    "ref",
    "references",
    "gallery",
    "math",
    "chem",
    "score",
    "timeline",
    "mapframe",
    "maplink",
    "graph",
    "hiero",
    "inputbox",
    "categorytree",
    "includeonly",
    "noinclude",
    "onlyinclude",
    "pre",
    "code",
    "tt",
    "syntaxhighlight",
    "source",
    "imagemap",
    "choose",
    "quiz",
    "poll",
];

/// Template keep-list: names whose last `|`-separated argument is prose
/// (the displayed text) rather than boilerplate. `lang-zh-hans`,
/// `lang-zh-hant`, … match the `lang-*` prefix rule.
const KEEP_TEMPLATES: &[&str] = &["lang", "lang-zh"];

/// Named HTML entities decoded after the XML layer.
const NAMED_ENTITIES: &[(&str, &str)] = &[
    ("nbsp", " "),
    ("amp", "&"),
    ("lt", "<"),
    ("gt", ">"),
    ("quot", "\""),
    ("apos", "'"),
    ("mdash", "—"),
    ("ndash", "–"),
    ("hellip", "…"),
    ("ldquo", "“"),
    ("rdquo", "”"),
    ("lsquo", "‘"),
    ("rsquo", "’"),
    ("middot", "·"),
    ("times", "×"),
    ("minus", "−"),
];

/// A `{| … |}` block is deleted whole, including `{{…}}` templates and
/// `[[…]]` links nested inside. `{|`/`|}` are only table delimiters at
/// template depth zero, so template arguments ending in `|}}` cannot
/// open or close a table.
fn remove_tables(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(chars.len());
    let mut i = 0_usize;
    let mut table_depth = 0_usize;
    let mut template_depth = 0_usize;

    while i < chars.len() {
        let two = &chars[i..];
        if two.starts_with(&['{', '{']) {
            template_depth += 1;
            if table_depth == 0 {
                out.push_str("{{");
            }
            i += 2;
            continue;
        }
        if two.starts_with(&['}', '}']) {
            template_depth = template_depth.saturating_sub(1);
            if table_depth == 0 {
                out.push_str("}}");
            }
            i += 2;
            continue;
        }
        if two.starts_with(&['{', '|']) && template_depth == 0 {
            table_depth += 1;
            i += 2;
            continue;
        }
        if two.starts_with(&['|', '}']) && template_depth == 0 && table_depth > 0 {
            table_depth -= 1;
            i += 2;
            continue;
        }
        if table_depth == 0 {
            out.push(chars[i]);
        }
        i += 1;
    }
    out
}

/// The scanner's line buffer — lets trailing heading `=` markers and
/// line-start list markers be handled with one rule each.
struct Line {
    text: String,
}

impl Line {
    fn push(&mut self, ch: char) {
        self.text.push(ch);
    }

    fn push_str(&mut self, text: &str) {
        self.text.push_str(text);
    }

    /// Finishes the line: trims, strips trailing and leading `=+` heading
    /// runs, drops empty parenthesis pairs left by dropped templates, and
    /// returns the line if non-empty.
    fn finish(&mut self) -> Option<String> {
        let without_headings = self
            .text
            .trim()
            .trim_end_matches('=')
            .trim_start_matches('=')
            .trim();
        let without_empty_parens = drop_empty_parens(without_headings);
        let result =
            (!without_empty_parens.is_empty()).then(|| without_empty_parens.trim().to_owned());
        self.text.clear();
        result
    }
}

/// Cached delimiter searches for one `strip_wikitext` pass.
///
/// The scanner restarts a forward search from every unmatched opener
/// (`</…`, `<!--`, `<name …`, `[[`, `{{`). Unclosed openers — `<` and `{`
/// are literal text in many pages — would each rescan to the end of the
/// page, quadratic overall. Every cache below is exact: it answers what
/// the uncached scan would, from state the earlier scans already paid for.
#[derive(Default)]
struct Memo {
    /// Next `>` (closing tags).
    gt: NextHit,
    /// Next `-->` (comments).
    comment: NextHit,
    /// Next `]]` (wiki links).
    link: NextHit,
    /// `(position, quote state)` pairs from which an open-tag scan is known
    /// to reach the end of input without an unquoted `>`; one bit per
    /// quote state. Empty until the first failed tag scan.
    tag_dead: Vec<u8>,
    /// Matching `}}` position + 1 for each `{{` visited by a failed
    /// template scan (`TEMPLATE_UNCLOSED` = never closes, 0 = unknown).
    /// Empty until the first failed template scan.
    template: Vec<u32>,
}

/// Marker in `Memo::template`: the `{{` has no matching `}}`.
const TEMPLATE_UNCLOSED: u32 = u32::MAX;

/// The last forward search for one needle. Search starts only ever move
/// right within a pass, so the first hit at or after an earlier start is
/// also the first hit at or after any later start up to that hit, and "no
/// hit" stays "no hit".
#[derive(Default)]
struct NextHit {
    last: Option<(usize, Option<usize>)>,
}

impl NextHit {
    fn find(&mut self, from: usize, search: impl FnOnce() -> Option<usize>) -> Option<usize> {
        if let Some((searched_from, found)) = self.last
            && searched_from <= from
        {
            match found {
                None => return None,
                Some(hit) if hit >= from => return Some(hit),
                Some(_) => {}
            }
        }
        let found = search();
        self.last = Some((from, found));
        found
    }
}

/// Strips wiki markup from one page's wikitext, producing raw text with
/// `\n` line breaks. The caller converts and sentence-splits the result.
#[must_use]
pub fn strip_wikitext(text: &str) -> String {
    let table_free = remove_tables(text);
    let chars: Vec<char> = table_free.chars().collect();
    let mut out = String::new();
    let mut line = Line {
        text: String::new(),
    };
    let mut i = 0_usize;
    let mut memo = Memo::default();

    while i < chars.len() {
        let at_line_start = i == 0 || chars[i - 1] == '\n';
        match chars[i] {
            '<' => i = on_tag(&chars, i, &mut out, &mut line, &mut memo),
            '{' => i = on_template(&chars, i, &mut line, &mut memo),
            '[' => i = on_link(&chars, i, &mut line, &mut memo),
            '\'' => {
                let run = take_run(&chars, i, '\'');
                if run >= 2 {
                    // Bold / italic markers: any run of two or more quotes.
                    i += run;
                } else {
                    line.push('\'');
                    i += 1;
                }
            }
            '*' | '#' | ':' | ';' if at_line_start => {
                // List markers: skip the run; a following space too.
                let mut j = i;
                while j < chars.len() && matches!(chars[j], '*' | '#' | ':' | ';') {
                    j += 1;
                }
                if j < chars.len() && chars[j] == ' ' {
                    j += 1;
                }
                i = j;
            }
            '=' if at_line_start => {
                i += take_run(&chars, i, '=');
            }
            '&' => {
                if let Some((end, decoded)) = scan_entity(&chars, i) {
                    line.push_str(&decoded);
                    i = end;
                } else {
                    line.push('&');
                    i += 1;
                }
            }
            '_' if chars[i + 1..].starts_with(&['_']) => {
                // Magic words (`__TOC__`, `__NOTOC__`): drop the token.
                if let Some(end) = find_str(&chars, i + 2, "__") {
                    i = end + 2;
                } else {
                    line.push('_');
                    i += 1;
                }
            }
            '\n' => {
                flush_line(&mut out, &mut line);
                i += 1;
            }
            ch => {
                line.push(ch);
                i += 1;
            }
        }
    }
    flush_line(&mut out, &mut line);
    out
}

/// Handles one `<` at `chars[i]`: comments, closing and opening tags with
/// their content drops, and the `br` line flushes; a bare `<` that starts
/// nothing is literal. Returns the next index.
fn on_tag(chars: &[char], i: usize, out: &mut String, line: &mut Line, memo: &mut Memo) -> usize {
    if chars[i + 1..].starts_with(&['!', '-', '-']) {
        memo.comment
            .find(i, || find_str(chars, i, "-->"))
            .map_or_else(
                || {
                    line.push('<');
                    i + 1
                },
                |end| end + 3,
            )
    } else if chars[i + 1..].starts_with(&['/']) {
        memo.gt.find(i, || find_char(chars, i, '>')).map_or_else(
            || {
                line.push('<');
                i + 1
            },
            |end| end + 1,
        )
    } else if let Some((name, open_end, self_closing)) = scan_tag(chars, i, &mut memo.tag_dead) {
        if self_closing {
            if name == "br" {
                flush_line(out, line);
            }
            open_end
        } else if name == "br" {
            flush_line(out, line);
            open_end
        } else if DROP_CONTENT_TAGS.contains(&name.as_str()) {
            let close = format!("</{name}>");
            // Unclosed: drop the rest of the page rather than emit markup.
            find_str(chars, open_end, &close).map_or(chars.len(), |end| end + close.len())
        } else {
            open_end
        }
    } else {
        line.push('<');
        i + 1
    }
}

/// Handles one `{` at `chars[i]`: kept templates contribute their last
/// argument, recursively stripped; everything else is literal. Returns the
/// next index.
fn on_template(chars: &[char], i: usize, line: &mut Line, memo: &mut Memo) -> usize {
    if chars[i + 1..].starts_with(&['{']) {
        if let Some((end, template)) = scan_template(chars, i, &mut memo.template) {
            let (name, last_arg) = template_parts(template);
            if is_kept_template(&name)
                && let Some(arg) = last_arg
            {
                // The kept argument is itself wikitext
                // (e.g. `{{lang|la|'''''x'''''}}`):
                // strip it recursively; the recursive
                // flush's trailing newline is dropped so
                // the outer line stays unsplit.
                let arg_text: String = arg.iter().collect();
                let stripped = strip_wikitext(&arg_text);
                line.push_str(stripped.trim_end_matches('\n'));
            }
            end
        } else {
            line.push('{');
            i + 1
        }
    } else {
        line.push('{');
        i + 1
    }
}

/// Handles one `[` at `chars[i]`: wiki and external links contribute their
/// display text; a bare `[` that starts nothing is literal. Returns the
/// next index.
fn on_link(chars: &[char], i: usize, line: &mut Line, memo: &mut Memo) -> usize {
    if chars[i + 1..].starts_with(&['[']) {
        if let Some((end, text)) = scan_link(chars, i, &mut memo.link) {
            if let Some(text) = text {
                line.push_str(&text.iter().collect::<String>());
            }
            end
        } else {
            line.push('[');
            i + 1
        }
    } else if let Some((end, text)) = scan_external(chars, i) {
        if let Some(text) = text {
            line.push_str(&text.iter().collect::<String>());
        }
        end
    } else {
        line.push('[');
        i + 1
    }
}

/// Emits the pending line and resets it.
fn flush_line(out: &mut String, line: &mut Line) {
    if let Some(finished) = line.finish() {
        out.push_str(&finished);
        out.push('\n');
    }
}

/// Drops empty parenthesis pairs left behind by dropped templates —
/// `（）`, `( )`, `（ ，）` — while keeping pairs with real content.
fn drop_empty_parens(text: &str) -> String {
    const FILL: &str = "，、：；。！？…·–—-—";
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(chars.len());
    let mut i = 0_usize;
    while i < chars.len() {
        let open = chars[i];
        if open == '（' || open == '(' {
            let close = if open == '（' { '）' } else { ')' };
            let mut j = i + 1;
            while j < chars.len() && j - i <= 16 && chars[j] != close {
                j += 1;
            }
            if j < chars.len()
                && chars[j] == close
                && chars[i + 1..j]
                    .iter()
                    .all(|&ch| ch.is_whitespace() || FILL.contains(ch))
            {
                i = j + 1;
                continue;
            }
        }
        out.push(open);
        i += 1;
    }
    out
}

/// `&nbsp;`-style entity at `start` → decoded text and end index.
fn scan_entity(chars: &[char], start: usize) -> Option<(usize, String)> {
    let mut end = start + 1;
    let mut name = String::new();
    while end < chars.len() && end - start <= 32 {
        if chars[end] == ';' {
            for (key, value) in NAMED_ENTITIES {
                if name == *key {
                    return Some((end + 1, (*value).to_owned()));
                }
            }
            if let Some(stripped) = name.strip_prefix('#') {
                let parsed = stripped
                    .strip_prefix('x')
                    .or_else(|| stripped.strip_prefix('X'))
                    .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                    .or_else(|| stripped.parse::<u32>().ok());
                if let Some(codepoint) = parsed.and_then(char::from_u32) {
                    return Some((end + 1, codepoint.to_string()));
                }
            }
            return None;
        }
        name.push(chars[end]);
        end += 1;
    }
    None
}

/// Finds `needle` at or after `start`.
fn find_str(chars: &[char], start: usize, needle: &str) -> Option<usize> {
    let needle: Vec<char> = needle.chars().collect();
    if chars.len() - start < needle.len() {
        return None;
    }
    (start..=chars.len() - needle.len())
        .find(|&pos| &chars[pos..pos + needle.len()] == needle.as_slice())
}

/// Finds `needle` at or after `start`.
fn find_char(chars: &[char], start: usize, needle: char) -> Option<usize> {
    chars[start..]
        .iter()
        .position(|&ch| ch == needle)
        .map(|pos| pos + start)
}

/// Length of the run of `ch` starting at `start` (at least 1).
fn take_run(chars: &[char], start: usize, ch: char) -> usize {
    let mut end = start;
    while end < chars.len() && chars[end] == ch {
        end += 1;
    }
    end - start
}

/// Bit for one quote state in `Memo::tag_dead`.
fn quote_bit(quote: Option<char>) -> u8 {
    match quote {
        None => 1,
        Some('"') => 2,
        Some(_) => 4,
    }
}

/// Quote state after `ch` inside an open tag.
fn next_quote(quote: Option<char>, ch: char) -> Option<char> {
    match ch {
        '"' | '\'' if quote.is_none() => Some(ch),
        quoted if quote == Some(quoted) => None,
        _ => quote,
    }
}

/// Scans an open tag `<name …>` starting at `<`. Returns the lowercased
/// name, the index just past the closing `>`, and whether the tag is
/// self-closing (`/>`).
///
/// A scan that reaches the end of input without an unquoted `>` marks
/// every `(position, quote state)` it passed in `dead`; later scans stop
/// when they join a marked path, so repeated unclosed `<name` openers cost
/// linear time in total instead of one full rescan each.
fn scan_tag(chars: &[char], start: usize, dead: &mut Vec<u8>) -> Option<(String, usize, bool)> {
    let mut cursor = start + 1;
    let mut name = String::new();
    while cursor < chars.len() && (chars[cursor].is_ascii_alphanumeric() || chars[cursor] == '_') {
        name.push(chars[cursor].to_ascii_lowercase());
        cursor += 1;
    }
    if name.is_empty() {
        return None;
    }
    let begin = cursor;
    let mut quote = None;
    let mut prev_non_ws = ' ';
    while cursor < chars.len() {
        if dead
            .get(cursor)
            .is_some_and(|bits| bits & quote_bit(quote) != 0)
        {
            break;
        }
        let ch = chars[cursor];
        if ch == '>' && quote.is_none() {
            return Some((name, cursor + 1, prev_non_ws == '/'));
        }
        let next = next_quote(quote, ch);
        // A quote delimiter is not a "last non-space" character.
        if next == quote && !ch.is_whitespace() {
            prev_non_ws = ch;
        }
        quote = next;
        cursor += 1;
    }
    dead.resize(chars.len(), 0);
    let mut quote = None;
    for (mark, &ch) in dead[begin..cursor].iter_mut().zip(&chars[begin..cursor]) {
        *mark |= quote_bit(quote);
        quote = next_quote(quote, ch);
    }
    None
}

fn is_template_open(chars: &[char], at: usize) -> bool {
    chars[at] == '{' && chars[at + 1..].starts_with(&['{'])
}

fn is_template_close(chars: &[char], at: usize) -> bool {
    chars[at] == '}' && chars[at + 1..].starts_with(&['}'])
}

/// Scans a `{{…}}` template starting at `{{`, tracking nesting. Returns
/// the index past `}}` and the raw inner content.
///
/// Tokenisation (`{{` / `}}` pairs, else one char) depends only on the
/// position, so the `}}` matching a given `{{` does not depend on which
/// scan reaches it. The first failed scan therefore switches to the
/// memoised matcher below, which records the match (or "never closes")
/// of every `{{` it passes in `closes`; repeated unclosed `{{` openers
/// then cost linear time in total instead of one full rescan each.
fn scan_template<'a>(
    chars: &'a [char],
    start: usize,
    closes: &mut Vec<u32>,
) -> Option<(usize, &'a [char])> {
    if closes.is_empty() {
        if let Some(found) = scan_template_plain(chars, start) {
            return Some(found);
        }
        // Entries hold positions + 1, so the page must fit below the marker.
        if u32::try_from(chars.len()).is_ok_and(|len| len < TEMPLATE_UNCLOSED) {
            closes.resize(chars.len(), 0);
        } else {
            return None;
        }
    }
    if closes[start] == 0 {
        match_templates(chars, start, closes);
    }
    match closes[start] {
        TEMPLATE_UNCLOSED => None,
        encoded => {
            let close = usize::try_from(encoded).ok()? - 1;
            Some((close + 2, &chars[start + 2..close]))
        }
    }
}

/// Fills `closes` for the `{{` at `start` and every `{{` nested in it.
fn match_templates(chars: &[char], start: usize, closes: &mut [u32]) {
    let mut open = vec![start];
    let mut cursor = start + 2;
    while cursor < chars.len() {
        if is_template_open(chars, cursor) {
            match closes[cursor] {
                0 => {
                    open.push(cursor);
                    cursor += 2;
                }
                // An unclosed inner template leaves every outer one unclosed.
                TEMPLATE_UNCLOSED => break,
                encoded => cursor = usize::try_from(encoded).map_or(chars.len(), |end| end + 1),
            }
        } else if is_template_close(chars, cursor) {
            if let Some(opener) = open.pop() {
                closes[opener] = u32::try_from(cursor + 1).unwrap_or(TEMPLATE_UNCLOSED);
            }
            if open.is_empty() {
                return;
            }
            cursor += 2;
        } else {
            cursor += 1;
        }
    }
    for opener in open {
        closes[opener] = TEMPLATE_UNCLOSED;
    }
}

/// The uncached `{{…}}` scan: tracks nesting from `start` and returns the
/// index past the matching `}}` and the raw inner content.
fn scan_template_plain(chars: &[char], start: usize) -> Option<(usize, &[char])> {
    let inner_start = start + 2;
    let mut depth = 0_usize;
    let mut cursor = start + 2;
    while cursor < chars.len() {
        if chars[cursor] == '{' && chars[cursor + 1..].starts_with(&['{']) {
            depth += 1;
            cursor += 2;
        } else if chars[cursor] == '}' && chars[cursor + 1..].starts_with(&['}']) {
            if depth == 0 {
                return Some((cursor + 2, &chars[inner_start..cursor]));
            }
            depth -= 1;
            cursor += 2;
        } else {
            cursor += 1;
        }
    }
    None
}

/// Whether a template name (lowercased, trimmed) is on the keep-list.
fn is_kept_template(name: &str) -> bool {
    KEEP_TEMPLATES.iter().any(|keep| {
        name == *keep
            || name
                .strip_prefix(keep)
                .is_some_and(|rest| rest.starts_with('-'))
    })
}

/// Splits template inner content into the (lowercased, trimmed) name and
/// the last `|`-separated argument (the displayed text for keep-list
/// templates). A `|` inside a `[[…]]` link does not separate arguments:
/// the whole `[[target|display]]` stays one argument.
fn template_parts(inner: &[char]) -> (String, Option<&[char]>) {
    let mut first_pipe = None;
    let mut last_pipe = None;
    let mut link_depth = 0_usize;
    for (index, ch) in inner.iter().enumerate() {
        match ch {
            '[' if inner[index + 1..].starts_with(&['[']) => link_depth += 1,
            ']' if inner[index + 1..].starts_with(&[']']) && link_depth > 0 => link_depth -= 1,
            '|' if link_depth == 0 => {
                if first_pipe.is_none() {
                    first_pipe = Some(index);
                }
                last_pipe = Some(index);
            }
            _ => {}
        }
    }
    let name: String = first_pipe
        .map_or(inner, |pos| &inner[..pos])
        .iter()
        .collect();
    let name = name.trim().to_ascii_lowercase();
    let last = last_pipe.map(|pos| &inner[pos + 1..]);
    (name, last)
}

/// Scans `[[target|display]]` starting at `[[`. Returns the index past
/// `]]` and the text to emit (`None` = drop), or `None` when
/// unterminated.
fn scan_link<'a>(
    chars: &'a [char],
    start: usize,
    close: &mut NextHit,
) -> Option<(usize, Option<&'a [char]>)> {
    let end = close.find(start + 2, || find_str(chars, start + 2, "]]"))?;
    let inner = &chars[start + 2..end];
    let mut parts = inner.split(|&ch| ch == '|');
    let target = parts.next().unwrap_or(&[]);
    let lower: String = target.iter().map(char::to_ascii_lowercase).collect();
    for prefix in [
        "file:",
        "image:",
        "category:",
        "分類:",
        "分类:",
        "檔案:",
        "文件:",
    ] {
        if lower.starts_with(prefix) {
            return Some((end + 2, None));
        }
    }
    let display = parts.next_back().unwrap_or(target);
    Some((end + 2, Some(display)))
}

/// Scans a single-bracket `[url display]` or citation `[12]` starting at
/// `[`. Returns the index past `]` and the text to emit (`None` = drop),
/// or `None` when this `[` should stay literal.
/// How far past a `[` the external-link scan looks for its `]` before
/// giving up and leaving the bracket literal. Real wikitext links are a
/// few hundred characters at most; the bound keeps a stray `[` in a long
/// paragraph from turning the rest of the article into one link.
const EXTERNAL_LINK_LOOKAHEAD: usize = 2048;

fn scan_external(chars: &[char], start: usize) -> Option<(usize, Option<&[char]>)> {
    let limit = (start + 1 + EXTERNAL_LINK_LOOKAHEAD).min(chars.len());
    let end = chars[start + 1..limit].iter().position(|&ch| ch == ']')? + start + 1;
    let inner = &chars[start + 1..end];
    if inner.contains(&'\n') {
        return None;
    }
    let inner_text: String = inner.iter().collect();
    if inner_text.trim().is_empty() {
        return None;
    }
    // A citation marker like `[12]`: drop.
    if inner_text.trim().chars().all(|ch| ch.is_ascii_digit()) {
        return Some((end + 1, None));
    }
    let has_scheme = inner_text.split_once(':').is_some_and(|(scheme, _)| {
        !scheme.is_empty() && scheme.chars().all(|ch| ch.is_ascii_alphabetic())
    });
    let has_space = inner_text.chars().any(char::is_whitespace);
    // A bare URL or `[mailto:…]`: drop.
    if has_scheme && !has_space {
        return Some((end + 1, None));
    }
    if has_space {
        // Keep the display text: everything after the first whitespace
        // following the URL — MediaWiki treats the whole remainder of the
        // bracket body as display text (`[https://x.example 中国 历史]` →
        // `中国 历史`), not just the last word. A bracketed span without
        // a scheme is not an external link; keeping its last
        // whitespace-separated run is the salvage for that case.
        let tail_start = if has_scheme {
            inner.iter().position(|ch| ch.is_whitespace())
        } else {
            inner.iter().rposition(|ch| ch.is_whitespace())
        };
        let tail = tail_start.map_or(inner, |pos| &inner[pos + 1..]);
        let mut trimmed = tail;
        while trimmed.first().is_some_and(|ch| ch.is_whitespace()) {
            trimmed = &trimmed[1..];
        }
        while trimmed.last().is_some_and(|ch| ch.is_whitespace()) {
            trimmed = &trimmed[..trimmed.len() - 1];
        }
        return Some((end + 1, Some(trimmed)));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        Memo, find_char, find_str, scan_tag, scan_template, scan_template_plain, strip_wikitext,
    };

    #[test]
    fn strips_plain_text_unchanged() {
        assert_eq!(strip_wikitext("这是普通文字。"), "这是普通文字。\n");
    }

    #[test]
    fn strips_refs_and_comments() {
        assert_eq!(
            strip_wikitext("甲<ref>注</ref>乙<!-- 注释 -->丙<ref name=\"a\"/>丁"),
            "甲乙丙丁\n"
        );
    }

    #[test]
    fn resolves_links_to_display_text() {
        assert_eq!(strip_wikitext("[[北京|北平]]很古老。"), "北平很古老。\n");
        assert_eq!(strip_wikitext("[[北京]]很古老。"), "北京很古老。\n");
        assert_eq!(strip_wikitext("[[File:photo.jpg|thumb|说明]]"), "");
        assert_eq!(strip_wikitext("[[Category:地理]]"), "");
    }

    #[test]
    fn drops_templates_except_lang_keep_list() {
        assert_eq!(strip_wikitext("{{cite web|title=x|url=y}}正文"), "正文\n");
        assert_eq!(strip_wikitext("{{lang|zh-hant|漢字}}文本"), "漢字文本\n");
        assert_eq!(strip_wikitext("{{lang-zh|漢字}}"), "漢字\n");
        assert_eq!(strip_wikitext("{{lang-zh-hant|漢字}}"), "漢字\n");
        assert_eq!(strip_wikitext("{{Infobox|x|{{nested|y}}|z}}尾"), "尾\n");
        assert_eq!(
            strip_wikitext("{{lang|la|'''''x'''''}}"),
            "x\n",
            "kept template arguments are re-stripped"
        );
    }

    #[test]
    fn drops_tables_with_nested_templates_and_links() {
        assert_eq!(
            strip_wikitext("前{| class=\"wikitable\"\n|-\n| {{a|b}} || [[c|d]]\n|}后"),
            "前后\n"
        );
        // `|}}` inside a template arg must not close the table early:
        // the `}}` belongs to the template, and the `|}` that follows
        // is the real close.
        assert_eq!(strip_wikitext("前{| {{t|x|}} |}后"), "前后\n");
    }

    #[test]
    fn handles_external_links_and_citations() {
        assert_eq!(
            strip_wikitext("[https://x.example/ 官方网站]"),
            "官方网站\n"
        );
        assert_eq!(strip_wikitext("[https://x.example/]"), "");
        assert_eq!(strip_wikitext("见文献[12]。"), "见文献。\n");
    }

    #[test]
    fn external_link_display_text_keeps_the_whole_tail() {
        // Display text is everything after the first space following the
        // URL, not just the last whitespace-separated word.
        assert_eq!(
            strip_wikitext("[https://example.com 中国 历史]"),
            "中国 历史\n"
        );
        // CJK + ASCII display text passes through unchanged.
        assert_eq!(
            strip_wikitext("[https://x.example/ 官方 site]"),
            "官方 site\n"
        );
    }

    #[test]
    fn keep_list_template_last_arg_may_be_a_piped_wikilink() {
        // `|` inside `[[target|display]]` is link syntax, not an argument
        // separator: the last argument is the whole link, re-stripped to
        // its display text.
        assert_eq!(strip_wikitext("{{lang|zh|[[中文|汉语]]}}"), "汉语\n");
    }

    #[test]
    fn strips_emphasis_headings_lists_and_magic_words() {
        assert_eq!(
            strip_wikitext("== 标题 ==\n'''粗体'''和''斜体''\n* 列表项\n# 编号项\n__NOTOC__"),
            "标题\n粗体和斜体\n列表项\n编号项\n"
        );
    }

    #[test]
    fn five_quote_runs_leave_no_stray_quotes() {
        assert_eq!(strip_wikitext("'''''x'''''y"), "xy\n");
        assert_eq!(strip_wikitext("''''''"), "");
        assert_eq!(strip_wikitext("a'b"), "a'b\n");
    }

    #[test]
    fn br_becomes_a_line_break() {
        assert_eq!(strip_wikitext("甲<br/>乙<br>丙"), "甲\n乙\n丙\n");
    }

    #[test]
    fn decodes_named_and_numeric_entities() {
        assert_eq!(strip_wikitext("a&nbsp;b&amp;c&#123;&#x22;"), "a b&c{\"\n");
    }

    #[test]
    fn keep_content_tags_drop_only_the_tags() {
        assert_eq!(strip_wikitext("<small>小字</small>正文"), "小字正文\n");
        assert_eq!(strip_wikitext("<code>x = 1</code>"), "");
        assert_eq!(strip_wikitext("<poem>诗句</poem>"), "诗句\n");
    }

    /// The open-tag scan as it was before the dead-path cache.
    fn scan_tag_uncached(chars: &[char], start: usize) -> Option<(String, usize, bool)> {
        let mut cursor = start + 1;
        let mut name = String::new();
        while cursor < chars.len()
            && (chars[cursor].is_ascii_alphanumeric() || chars[cursor] == '_')
        {
            name.push(chars[cursor].to_ascii_lowercase());
            cursor += 1;
        }
        if name.is_empty() {
            return None;
        }
        let mut quote = None;
        let mut prev_non_ws = ' ';
        while cursor < chars.len() {
            let ch = chars[cursor];
            match ch {
                '"' | '\'' if quote.is_none() => quote = Some(ch),
                quoted if quote == Some(quoted) => quote = None,
                '>' if quote.is_none() => {
                    return Some((name, cursor + 1, prev_non_ws == '/'));
                }
                ch if !ch.is_whitespace() => prev_non_ws = ch,
                _ => {}
            }
            cursor += 1;
        }
        None
    }

    /// Random strings over `alphabet`, from a fixed seed.
    fn delimiter_soup(alphabet: &str, cases: usize) -> Vec<Vec<char>> {
        let alphabet: Vec<char> = alphabet.chars().collect();
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        (0..cases)
            .map(|_| {
                let len = usize::try_from(next() % 120).unwrap_or(0);
                (0..len)
                    .map(|_| alphabet[usize::try_from(next()).unwrap_or(0) % alphabet.len()])
                    .collect()
            })
            .collect()
    }

    #[test]
    fn cached_scans_agree_with_uncached_scans() {
        // The full delimiter set, then narrow alphabets that make each
        // construct (templates, tags and quotes, links, comments) dense.
        let soups = [
            "<>/!-[]{}|'\" =_\nabre",
            "{}|a",
            "<>'\"/ a",
            "[]a|",
            "<!->a",
        ];
        for chars in soups
            .iter()
            .flat_map(|alphabet| delimiter_soup(alphabet, 20_000))
        {
            let mut memo = Memo::default();
            // Starts only move right within a pass, as in `strip_wikitext`;
            // the stride skips positions the way consumed spans do.
            for start in (0..chars.len()).step_by(1 + chars.len() % 3) {
                if chars[start] == '<' {
                    assert_eq!(
                        memo.gt.find(start, || find_char(&chars, start, '>')),
                        find_char(&chars, start, '>'),
                        "gt {chars:?} @{start}"
                    );
                    assert_eq!(
                        memo.comment.find(start, || find_str(&chars, start, "-->")),
                        find_str(&chars, start, "-->"),
                        "comment {chars:?} @{start}"
                    );
                    assert_eq!(
                        scan_tag(&chars, start, &mut memo.tag_dead),
                        scan_tag_uncached(&chars, start),
                        "tag {chars:?} @{start}"
                    );
                }
                if chars[start] == '[' && chars[start + 1..].starts_with(&['[']) {
                    assert_eq!(
                        memo.link
                            .find(start + 2, || find_str(&chars, start + 2, "]]")),
                        find_str(&chars, start + 2, "]]"),
                        "link {chars:?} @{start}"
                    );
                }
                if chars[start] == '{' && chars[start + 1..].starts_with(&['{']) {
                    assert_eq!(
                        scan_template(&chars, start, &mut memo.template),
                        scan_template_plain(&chars, start),
                        "template {chars:?} @{start}"
                    );
                }
            }
        }
    }

    #[test]
    fn unclosed_openers_scale_linearly() {
        // Each unit opens a construct that never closes. Rescanning to the
        // end of the page from every opener is quadratic: at this size it
        // takes minutes, the cached scans milliseconds.
        const UNITS: usize = 100_000;
        for unit in ["</pag", "<!--x", "<a x", "<a '", "[[a", "{{a", "{{{a"] {
            let text = unit.repeat(UNITS);
            assert_eq!(strip_wikitext(&text), format!("{text}\n"), "{unit}");
        }
    }
}
