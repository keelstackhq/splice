//! The rendering subset gate (contract §4, §9; spec §6).

use crate::error::SpliceError;

type Result<T> = std::result::Result<T, SpliceError>;

/// Names that are keywords or literals in one engine or the other, or that behave differently
/// there (spec §6, ruling C1; mirrors hull's `RESERVED_IDENTS`). Never usable as variables.
const KEYWORDS: [&str; 12] = [
    "true",
    "false",
    "none",
    "not",
    "and",
    "or",
    "in",
    "is",
    "loop",
    "self",
    "range",
    "namespace",
];

/// Context objects usable only through `"id" in pieces`, `for x in pieces` or dotted access.
const BARE_FORBIDDEN: [&str; 3] = ["pieces", "piece", "template"];

/// The only dotted names (contract §9.3).
const DOTTED: [&str; 4] = [
    "piece.id",
    "piece.name",
    "template.name",
    "template.version",
];

/// Reject any template syntax outside the portable subset. Errors name `origin:line`.
pub fn validate(text: &str, origin: &str) -> Result<()> {
    scan(text, origin, true).map(drop)
}

/// True when `text` is inside the subset and has at least one `{% raw %}` block (a raw tag in a
/// comment does not count). Invalid text is `false`: rendering reports it.
pub fn uses_raw(text: &str) -> bool {
    matches!(scan(text, "", true), Ok(n) if n > 0)
}

/// A rendered path segment may contain only `{{ expr }}` tags.
pub fn validate_path_segment(seg: &str, origin: &str) -> Result<()> {
    scan(seg, origin, false).map(drop)
}

enum Frame {
    If { line: usize, seen_else: bool },
    For { line: usize },
}

/// Validate `text`; returns the number of raw blocks.
fn scan(text: &str, origin: &str, blocks: bool) -> Result<usize> {
    let mut raws = 0;
    scan_counting(text, origin, blocks, &mut raws)?;
    Ok(raws)
}

fn scan_counting(text: &str, origin: &str, blocks: bool, raws: &mut usize) -> Result<()> {
    let line_of = |offset: usize| 1 + text[..offset].bytes().filter(|&b| b == b'\n').count();
    let fail = |offset: usize, msg: String| {
        Err(SpliceError::Template(format!(
            "{origin}:{}: {msg}",
            line_of(offset)
        )))
    };
    let mut stack: Vec<Frame> = Vec::new();
    let mut i = 0;
    // Start of the plain text since the last tag: a `#}` in it is stray (spec §10 item 17).
    let mut plain = 0;
    let stray = |from: usize, to: usize| match text[from..to].find("#}") {
        Some(off) => fail(from + off, "`#}` outside a comment".to_string()),
        None => Ok(()),
    };
    while let Some(found) = text[i..].find('{') {
        let start = i + found;
        let close = match text.as_bytes().get(start + 1) {
            Some(b'{') => "}}",
            Some(b'%') => "%}",
            Some(b'#') => "#}",
            _ => {
                i = start + 1;
                continue;
            }
        };
        stray(plain, start)?;
        if close == "%}"
            && blocks
            && let Some((name, strict, _)) = lax_tag(&text[start + 2..])
        {
            if name == "endraw" {
                return fail(
                    start,
                    "`{% endraw %}` without a matching `{% raw %}`".into(),
                );
            }
            if !strict {
                return fail(
                    start,
                    "`raw` allows no whitespace control (`-`, `+`) and only space, tab, CR or LF inside the tag".into(),
                );
            }
            match raw_end(text, start + 2) {
                Ok(after) => {
                    *raws += 1;
                    i = after;
                    plain = after;
                    continue;
                }
                Err((at, msg)) => return fail(at.unwrap_or(start), msg),
            }
        }
        let open = &text[start..start + 2];
        let Some(len) = text[start + 2..].find(close) else {
            return fail(start, format!("unterminated `{open}` tag"));
        };
        let inner = &text[start + 2..start + 2 + len];
        let end = start + 2 + len + 2;
        let tag = &text[start..end];
        i = end;
        plain = end;
        let res = match open {
            "{{" => check_output(inner),
            _ if !blocks => Err("only `{{ var }}` is allowed in a path segment".to_string()),
            "{#" if inner.starts_with(['-', '+']) || inner.ends_with(['-', '+']) => {
                Err("a comment allows no whitespace control (`-`, `+`)".to_string())
            }
            "{#" => Ok(()),
            _ => check_statement(inner, line_of(start), &mut stack),
        };
        if let Err(why) = res {
            return fail(
                start,
                format!("`{}` is outside the template subset: {why}", tag.trim()),
            );
        }
    }
    stray(plain, text.len())?;
    if let Some(frame) = stack.last() {
        let (line, what) = match frame {
            Frame::If { line, .. } => (*line, "if"),
            Frame::For { line } => (*line, "for"),
        };
        return Err(SpliceError::Template(format!(
            "{origin}:{line}: `{{% {what} %}}` is never closed"
        )));
    }
    Ok(())
}

/// Whitespace as either engine's lexer may read it inside a tag (MiniJinja: ASCII whitespace,
/// Nunjucks: JS `\s`). Only space, tab, CR and LF are allowed; the rest is detected to reject it.
fn lax_ws(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// Recognise `raw` / `endraw` as either engine would, on the text right after `{%`: optional
/// whitespace control and any whitespace around the name, then `%}`. Returns the name, whether
/// the spelling is the strict one (only space/tab/CR/LF, no `-`/`+`), and the length consumed.
fn lax_tag(after: &str) -> Option<(&'static str, bool, usize)> {
    let skip = |c: char| lax_ws(c) || c == '-' || c == '+';
    let a = after.trim_start_matches(skip);
    let (name, rest) = if let Some(r) = a.strip_prefix("endraw") {
        ("endraw", r)
    } else {
        ("raw", a.strip_prefix("raw")?)
    };
    let b = rest.trim_start_matches(skip);
    let tail = b.strip_prefix("%}")?;
    let consumed = after.len() - tail.len();
    let head = &after[..consumed - 2];
    let strict = head.trim_matches([' ', '\t', '\n', '\r']) == name;
    Some((name, strict, consumed))
}

type RawErr = (Option<usize>, String);

/// Find the `{% endraw %}` closing a raw block whose body starts at `from`; returns the offset
/// just after it. Mirrors MiniJinja (every `{%` is tried in turn). Spellings on which MiniJinja
/// and Nunjucks disagree are rejected: dashes, odd whitespace, and an inner `{% raw %}` (Nunjucks
/// nests raw blocks, MiniJinja does not). `None` as the offset means "the opening tag".
fn raw_end(text: &str, from: usize) -> std::result::Result<usize, RawErr> {
    let mut p = from;
    while let Some(off) = text[p..].find("{%") {
        let q = p + off + 2;
        if let Some((name, strict, len)) = lax_tag(&text[q..]) {
            if name == "raw" {
                return Err((
                    Some(q - 2),
                    "a `raw` tag inside a raw block is not allowed (engines disagree on nesting)"
                        .into(),
                ));
            }
            if !strict {
                return Err((
                    Some(q - 2),
                    "this `endraw` spelling is ambiguous inside a raw block (no `-`/`+`, only space, tab, CR or LF)"
                        .into(),
                ));
            }
            return Ok(q + len);
        }
        p = q;
    }
    Err((None, "`{% raw %}` is never closed".into()))
}

/// Whitespace allowed inside a tag (contract §9.19): ASCII space, tab, CR and LF only. Other
/// whitespace (NBSP, form feed, vertical tab) stays part of a token and fails the checks.
fn tag_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some('a'..='z' | '_'))
        && chars.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_'))
}

fn check_path(p: &str) -> std::result::Result<(), String> {
    if !p.split('.').all(is_ident) {
        return Err(format!(
            "`{p}` is not a variable path like `name` or `piece.id`"
        ));
    }
    let root = p.split('.').next().unwrap_or_default();
    if KEYWORDS.contains(&root) {
        return Err(format!("`{root}` is a keyword or literal, not a variable"));
    }
    if p.contains('.') {
        if !DOTTED.contains(&p) {
            return Err(format!(
                "dotted name `{p}` is not allowed (only piece.id, piece.name, template.name, template.version)"
            ));
        }
    } else if BARE_FORBIDDEN.contains(&p) {
        return Err(format!(
            "bare `{p}` is not allowed (use `\"id\" in pieces`, piece.id, piece.name, template.name or template.version)"
        ));
    }
    Ok(())
}

fn check_output(inner: &str) -> std::result::Result<(), String> {
    let mut parts = inner.split('|');
    check_path(parts.next().unwrap_or_default().trim_matches(tag_ws))?;
    if let Some(filter) = parts.next() {
        if !matches!(filter.trim_matches(tag_ws), "lower" | "upper") {
            return Err("only the filters `lower` and `upper` are allowed".into());
        }
        if parts.next().is_some() {
            return Err("at most one filter is allowed".into());
        }
    }
    Ok(())
}

fn is_piece_id_literal(s: &str) -> bool {
    s.len() > 2
        && s.starts_with('"')
        && s.ends_with('"')
        && s[1..s.len() - 1]
            .chars()
            .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '-'))
}

fn check_statement(
    inner: &str,
    line: usize,
    stack: &mut Vec<Frame>,
) -> std::result::Result<(), String> {
    let words: Vec<&str> = inner.split(tag_ws).filter(|w| !w.is_empty()).collect();
    match words.as_slice() {
        ["if" | "elif", cond @ ..] => {
            match cond {
                [path] => check_path(path)?,
                [lit, "in", "pieces"] if is_piece_id_literal(lit) => {}
                _ => {
                    return Err("a condition is `var` or `\"piece-id\" in pieces`".to_string());
                }
            }
            if words[0] == "if" {
                stack.push(Frame::If {
                    line,
                    seen_else: false,
                });
            } else {
                match stack.last() {
                    Some(Frame::If {
                        seen_else: false, ..
                    }) => {}
                    _ => return Err("`elif` without a matching `if` (or after `else`)".into()),
                }
            }
            Ok(())
        }
        ["else"] => match stack.last_mut() {
            Some(Frame::If { seen_else, .. }) if !*seen_else => {
                *seen_else = true;
                Ok(())
            }
            _ => Err("`else` is only allowed once, inside `if`".into()),
        },
        ["endif"] => match stack.pop() {
            Some(Frame::If { .. }) => Ok(()),
            _ => Err("`endif` without a matching `if`".into()),
        },
        ["endfor"] => match stack.pop() {
            Some(Frame::For { .. }) => Ok(()),
            _ => Err("`endfor` without a matching `for`".into()),
        },
        ["for", var, "in", "pieces"] => {
            if !is_ident(var) || (KEYWORDS.contains(var) || BARE_FORBIDDEN.contains(var)) {
                return Err(format!("`{var}` is not allowed as a loop variable"));
            }
            stack.push(Frame::For { line });
            Ok(())
        }
        _ => Err("allowed tags are if/elif/else/endif and `for x in pieces`/endfor".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_subset() {
        for t in [
            "{{ name }}",
            "{{ name | upper }}",
            "{{piece.name|lower}}",
            "{% if a %}x{% elif \"app\" in pieces %}y{% else %}z{% endif %}",
            "{% for p in pieces %}{{ p }}{% endfor %}",
            "{# any } thing #}",
            "{% raw %}a #} b{% endraw %}",
            "{{ a }}{% raw %}#}{% endraw %}",
            "{% raw %}{% endraw %}",
            "{%raw%}{{ x }}{%endraw%}",
            "{%\n\traw\r\n%}x{% \tendraw\n%}",
            "{% raw %}{{ x }}{% endraw %}",
            "{% raw %}{% if %}{% endraw %}",
            "{% raw %}{# #}{% endraw %}",
            "{% raw %}{{ ~ }} {% set %} {# open {{ open{% endraw %}",
            "{% raw %}{% endraw x %}{% endrawx %}{%- endraw{% endraw %}",
            "{% if a %}{% raw %}{% endif %}{% endraw %}{% endif %}",
            "{% for p in pieces %}{% raw %}{% for %}{% endraw %}{% endfor %}",
            "{% raw %}a{% endraw %}{% raw %}b{% endraw %}",
            "{# {% raw %} #}",
        ] {
            assert!(validate(t, "t").is_ok(), "{t}");
        }
    }

    #[test]
    fn rejects_raw_and_stray_comment_end() {
        for t in [
            "foo #} bar",
            "{{ a }} #}",
            "#}{{ a }}",
            "{# c #} #}",
            "{% raw %}x{% endraw %} #}",
            "{% raw %}x",
            "{% raw %}{% endraw",
            "{% endraw %}",
            "{%- raw %}x{% endraw %}",
            "{% raw -%}x{% endraw %}",
            "{%+ raw %}x{% endraw %}",
            "{% raw +%}x{% endraw %}",
            "{% raw %}x{%- endraw %}",
            "{% raw %}x{% endraw -%}",
            "{% raw %}x{%+ endraw %}",
            "{% raw x %}x{% endraw %}",
            "{% raw %}x{% endraw %}{% endraw %}",
            "{%\u{a0}raw %}x{% endraw %}",
            "{% raw %}x{%\u{a0}endraw %}",
            "{% raw %}x{%\u{c}endraw %}",
            "{% raw %}{% raw %}{% endraw %}{% endraw %}",
            "{% raw %}{%- raw %}{% endraw %}",
            "{% RAW %}x{% ENDRAW %}",
            "{% if a %}{% raw %}{% endraw %}",
        ] {
            let e = validate(t, "f.jinja").unwrap_err();
            assert_eq!(e.exit_code(), 3, "{t}");
            assert!(e.to_string().contains("f.jinja:1"), "{t}: {e}");
        }
    }

    #[test]
    fn raw_errors_name_the_line() {
        let e = validate("a\nb\n{% raw %}\nx\ny", "f.jinja").unwrap_err();
        assert!(e.to_string().contains("f.jinja:3"), "{e}");
        let e = validate("a\n{% raw %}\nx\n{%- endraw %}", "f.jinja").unwrap_err();
        assert!(e.to_string().contains("f.jinja:4"), "{e}");
        let e = validate("{% raw %}a\n{% endraw %}\n{{ x ~ y }}", "f.jinja").unwrap_err();
        assert!(e.to_string().contains("f.jinja:3"), "{e}");
        let e = validate("{% raw %}\n\n{% endraw %}{% set %}", "f.jinja").unwrap_err();
        assert!(e.to_string().contains("f.jinja:3"), "{e}");
        let e = validate("a\nb\nc #} d", "f.jinja").unwrap_err();
        assert!(e.to_string().contains("f.jinja:3"), "{e}");
    }

    #[test]
    fn rejects_everything_else() {
        for t in [
            "{% set x = 1 %}",
            "{{ a ~ b }}",
            "{%- if a %}{% endif %}",
            "{{ loop.index }}x",
            "{% include 'x' %}",
            "{% macro m() %}{% endmacro %}",
            "{{ name | title }}",
            "{% if not a %}{% endif %}",
            "{% if pieces %}{% endif %}",
            "{% for x in items %}{% endfor %}",
            "{{ \"lit\" }}",
        ] {
            let e = validate(t, "f.jinja").unwrap_err();
            assert_eq!(e.exit_code(), 3, "{t}");
            assert!(e.to_string().contains("f.jinja:1"), "{t}: {e}");
        }
    }

    #[test]
    fn rejects_more_syntax() {
        for t in [
            "{{ a -}}",
            "{{- a }}",
            "{% if a -%}{% endif %}",
            "{{ self.x }}",
            "{{ range }}",
            "{{ namespace.x }}",
            "{{ pieces }}",
            "{{ Name }}",
            "{{ a | lower | upper }}",
            "{{ a.b. }}",
            "{{ }}",
            "{% if a and b %}{% endif %}",
            "{% if a == b %}{% endif %}",
            "{% if \"A\" in pieces %}{% endif %}",
            "{% if \"a\" in other %}{% endif %}",
            "{% if \"a\" not in pieces %}{% endif %}",
            "{% for p in pieces %}{% else %}{% endfor %}",
            "{% for loop in pieces %}{% endfor %}",
            "{% for p, q in pieces %}{% endfor %}",
            "{% else %}",
            "{% endif %}",
            "{% endfor %}",
            "{% if a %}",
            "{% if a %}{% endfor %}",
            "{% for p in pieces %}{% endif %}",
            "{% if a %}{% else %}{% elif b %}{% endif %}",
            "{% if a %}{% else %}{% else %}{% endif %}",
            "{% if a %}{% for p in pieces %}{% endif %}{% endfor %}",
            "{% if a %}x{% endif extra %}",
            "{% endif",
            "{{ a",
            "{# open",
            "{% block b %}{% endblock %}",
            "{% filter upper %}{% endfilter %}",
            "{% call x %}{% endcall %}",
            "{% extends 'x' %}",
            "{% import 'x' as y %}",
            "{% ifa %}{% endif %}",
            "{{ true }}",
            "{{ false }}",
            "{{ none }}",
            "{% if false %}{% endif %}",
            "{% if true %}{% endif %}",
            "{% if none %}{% endif %}",
            "{{ not }}",
            "{{ and }}",
            "{{ or }}",
            "{{ in }}",
            "{{ is }}",
            "{% if not %}{% endif %}",
            "{{ true.x }}",
            "{{ piece }}",
            "{{ template }}",
            "{% if piece %}{% endif %}",
            "{% if template %}{% endif %}",
            "{{ piece.version }}",
            "{{ template.id }}",
            "{{ piece.name.x }}",
            "{{ a.b }}",
            "{% if a.b %}{% endif %}",
            "{% for true in pieces %}{% endfor %}",
            "{% for piece in pieces %}{% endfor %}",
        ] {
            let e = validate(t, "f.jinja").unwrap_err();
            assert_eq!(e.exit_code(), 3, "{t}");
            assert!(e.to_string().contains("f.jinja:1"), "{t}: {e}");
        }
    }

    #[test]
    fn rejects_comment_whitespace_control() {
        for t in [
            "{#- c #}", "{# c -#}", "{#+ c #}", "{# c +#}", "{#-#}", "{#+#}",
        ] {
            let e = validate(t, "f.jinja").unwrap_err();
            assert_eq!(e.exit_code(), 3, "{t}");
            assert!(e.to_string().contains("f.jinja:1"), "{t}: {e}");
        }
        for t in ["{# - c - #}", "{# a-b #}", "{# + #}", "{##}"] {
            assert!(validate(t, "t").is_ok(), "{t}");
        }
    }

    #[test]
    fn rejects_non_ascii_tag_whitespace() {
        for t in [
            "{{\u{a0}name }}",
            "{{ name\u{a0}}}",
            "{{ name |\u{a0}lower }}",
            "{{\u{c}name }}",
            "{{ name\u{b}}}",
            "{%\u{a0}if a %}{% endif %}",
            "{% if\u{c}a %}{% endif %}",
            "{% if a %}{%\u{b}endif %}",
            "{% for\u{a0}p in pieces %}{% endfor %}",
            "{% if \"a\"\u{2003}in pieces %}{% endif %}",
        ] {
            let e = validate(t, "f.jinja").unwrap_err();
            assert_eq!(e.exit_code(), 3, "{t:?}");
        }
        assert!(validate("{{\r\n\tname\t|\r\nlower }}", "t").is_ok());
        assert!(validate("{%\tif\r\na\n%}{%\tendif\t%}", "t").is_ok());
    }

    #[test]
    fn accepts_nested_and_spacing_variants() {
        for t in [
            "{%if a%}x{%endif%}",
            "{%  if  \"a-1\"  in  pieces  %}x{%   endif   %}",
            "{% for p in pieces %}{% if p %}{{ p }}{% else %}-{% endif %}{% endfor %}",
            "{% if a %}{% if b %}x{% elif c %}y{% endif %}{% elif d %}z{% endif %}",
            "{{ piece.id | lower }}{{template.version|upper}}",
            "{{ _a1_b2 }}",
            "{{ piece.id }}{{ piece.name }}{{ template.name }}{{ template.version }}",
            "{% if piece.name %}{% endif %}",
            "plain { text } and {x} and } {",
            "{# {{ not parsed #}",
            "{{\n name\n}}",
        ] {
            assert!(validate(t, "t").is_ok(), "{t}: {:?}", validate(t, "t"));
        }
    }

    #[test]
    fn error_names_line_and_tag() {
        let e = validate("a\nb\n  {% set x = 1 %}\n", "pieces/p/files/f.jinja").unwrap_err();
        let m = e.to_string();
        assert!(m.contains("pieces/p/files/f.jinja:3"), "{m}");
        assert!(m.contains("set"), "{m}");
    }

    #[test]
    fn unbalanced_error_names_opening_line() {
        let e = validate("a\n{% if x %}\nb\n", "f").unwrap_err();
        assert!(e.to_string().contains("f:2"), "{e}");
    }

    #[test]
    fn path_segments_allow_only_output_tags() {
        for ok in [
            "{{ name }}",
            "app-{{ name | lower }}",
            "plain",
            "{{piece.id}}.x",
        ] {
            assert!(validate_path_segment(ok, "t").is_ok(), "{ok}");
        }
        for bad in [
            "{% if a %}x{% endif %}",
            "{# c #}",
            "{{ loop.x }}",
            "{{ \"x\" }}",
            "{{ a ~ b }}",
            "{{ name",
            "{% raw %}x{% endraw %}",
            "a #} b",
        ] {
            let e = validate_path_segment(bad, "target").unwrap_err();
            assert_eq!(e.exit_code(), 3, "{bad}");
            assert!(e.to_string().contains("target"), "{bad}: {e}");
        }
    }
}
