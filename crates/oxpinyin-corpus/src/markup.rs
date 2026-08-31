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

    while i < chars.len() {
        let at_line_start = i == 0 || chars[i - 1] == '\n';
        match chars[i] {
            '<' => i = on_tag(&chars, i, &mut out, &mut line),
            '{' => i = on_template(&chars, i, &mut line),
            '[' => i = on_link(&chars, i, &mut line),
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
fn on_tag(chars: &[char], i: usize, out: &mut String, line: &mut Line) -> usize {
    if chars[i + 1..].starts_with(&['!', '-', '-']) {
        find_str(chars, i, "-->").map_or_else(
            || {
                line.push('<');
                i + 1
            },
            |end| end + 3,
        )
    } else if chars[i + 1..].starts_with(&['/']) {
        find_char(chars, i, '>').map_or_else(
            || {
                line.push('<');
                i + 1
            },
            |end| end + 1,
        )
    } else if let Some((name, open_end, self_closing)) = scan_tag(chars, i) {
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
fn on_template(chars: &[char], i: usize, line: &mut Line) -> usize {
    if chars[i + 1..].starts_with(&['{']) {
        if let Some((end, template)) = scan_template(chars, i) {
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
fn on_link(chars: &[char], i: usize, line: &mut Line) -> usize {
    if chars[i + 1..].starts_with(&['[']) {
        if let Some((end, text)) = scan_link(chars, i) {
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

/// Scans an open tag `<name …>` starting at `<`. Returns the lowercased
/// name, the index just past the closing `>`, and whether the tag is
/// self-closing (`/>`).
fn scan_tag(chars: &[char], start: usize) -> Option<(String, usize, bool)> {
    let mut cursor = start + 1;
    let mut name = String::new();
    while cursor < chars.len() && (chars[cursor].is_ascii_alphanumeric() || chars[cursor] == '_') {
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

/// Scans a `{{…}}` template starting at `{{`, tracking nesting. Returns
/// the index past `}}` and the raw inner content.
fn scan_template(chars: &[char], start: usize) -> Option<(usize, &[char])> {
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
fn scan_link(chars: &[char], start: usize) -> Option<(usize, Option<&[char]>)> {
    let end = find_str(chars, start + 2, "]]")?;
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
fn scan_external(chars: &[char], start: usize) -> Option<(usize, Option<&[char]>)> {
    // Bound the lookahead so a stray `[` far from any `]` stays literal.
    let limit = (start + 1 + 2048).min(chars.len());
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
    use super::strip_wikitext;

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
}
