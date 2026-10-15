//! Subset gate (spec section 6): rejects any template syntax outside the portable grammar
//! before MiniJinja sees the file.

use crate::error::SpliceError;

type Result<T> = std::result::Result<T, SpliceError>;

const BAD_ROOTS: [&str; 4] = ["loop", "self", "range", "namespace"];
const KEYWORDS: [&str; 12] = [
    "true", "false", "none", "null", "True", "False", "None", "not", "and", "or", "in", "is",
];

#[derive(PartialEq)]
enum Frame {
    If { seen_else: bool },
    For,
}

fn err(origin: &str, line: usize, msg: impl std::fmt::Display) -> SpliceError {
    SpliceError::Template(format!("{origin}:{line}: {msg}"))
}

/// Validate a whole `.jinja` file.
pub fn validate(text: &str, origin: &str) -> Result<()> {
    scan(text, origin, true)
}

/// Validate one path segment: literal text and `{{ expr }}` only.
pub fn validate_path_segment(seg: &str, origin: &str) -> Result<()> {
    scan(seg, origin, false)
}

fn scan(text: &str, origin: &str, blocks: bool) -> Result<()> {
    let mut stack: Vec<(Frame, usize)> = Vec::new();
    let mut pos = 0;
    let (mut line, mut counted) = (1usize, 0usize);
    while let Some(off) = text[pos..].find('{') {
        let start = pos + off;
        line += text[counted..start].matches('\n').count();
        counted = start;
        let rest = &text[start..];
        let (close, kind) = match rest.as_bytes().get(1) {
            Some(b'{') => ("}}", 'o'),
            Some(b'%') => ("%}", 'b'),
            Some(b'#') => ("#}", 'c'),
            _ => {
                pos = start + 1;
                continue;
            }
        };
        let Some(end) = rest[2..].find(close) else {
            return Err(err(
                origin,
                line,
                format!("unterminated `{}` tag", &rest[..2]),
            ));
        };
        let inner = &rest[2..2 + end];
        pos = start + 2 + end + 2;
        if kind == 'b'
            && blocks
            && let Some((name, strict, _)) = lax_tag(&rest[2..])
        {
            {
                if name == "endraw" {
                    return Err(err(
                        origin,
                        line,
                        "`{% endraw %}` without a matching `{% raw %}`",
                    ));
                }
                if !strict {
                    return Err(err(
                        origin,
                        line,
                        "`raw` allows no whitespace control (`-`, `+`) and only space, tab, CR or LF inside the tag",
                    ));
                }
                match raw_end(text, pos) {
                    Ok(after) => {
                        pos = after;
                        continue;
                    }
                    Err((at, m)) => {
                        let l = if let Some(at) = at {
                            1 + text[..at].matches('\n').count()
                        } else {
                            line
                        };
                        return Err(err(origin, l, m));
                    }
                }
            }
        }
        match kind {
            'c' if blocks => {
                if inner.starts_with(['-', '+']) || inner.ends_with(['-', '+']) {
                    return Err(err(
                        origin,
                        line,
                        "whitespace control in a comment is not allowed",
                    ));
                }
            }
            'c' | 'b' if !blocks => {
                return Err(err(
                    origin,
                    line,
                    format!(
                        "`{}` is not allowed in a path segment (only `{{{{ var }}}}`)",
                        &rest[..2]
                    ),
                ));
            }
            'o' => {
                output(inner).map_err(|m| err(origin, line, format!("`{{{{{inner}}}}}`: {m}")))?
            }
            _ => block(inner, &mut stack, line)
                .map_err(|m| err(origin, line, format!("`{{%{inner}%}}`: {m}")))?,
        }
    }
    if let Some((f, line)) = stack.last() {
        let name = if *f == Frame::For { "for" } else { "if" };
        return Err(err(
            origin,
            *line,
            format!("`{{% {name} %}}` is never closed"),
        ));
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
/// nests raw blocks, MiniJinja does not).
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

fn ws(s: &str) -> &str {
    s.trim_matches([' ', '\t', '\n', '\r'])
}

fn is_ident(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(ch) if ch.is_ascii_lowercase() || ch == '_')
        && c.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
}

/// `path := ident ("." ident)*`
fn path(s: &str) -> std::result::Result<(), String> {
    if s.is_empty() {
        return Err("expected a variable name".into());
    }
    for (i, part) in s.split('.').enumerate() {
        if !is_ident(part) {
            return Err(format!("`{s}` is not a plain variable path"));
        }
        if i == 0 && BAD_ROOTS.contains(&part) {
            return Err(format!("`{part}` is not available in the portable subset"));
        }
        if KEYWORDS.contains(&part) {
            return Err(format!("`{part}` is not allowed here"));
        }
    }
    Ok(())
}

fn output(inner: &str) -> std::result::Result<(), String> {
    let inner = ws(inner);
    let (p, filter) = match inner.split_once('|') {
        Some((p, f)) => (ws(p), Some(ws(f))),
        None => (inner, None),
    };
    path(p)?;
    if ["pieces", "piece", "template"].contains(&p) {
        return Err(format!("`{p}` cannot be printed directly"));
    }
    match filter {
        None | Some("lower") | Some("upper") => Ok(()),
        Some(f) => Err(format!("filter `{f}` is not allowed (only lower, upper)")),
    }
}

fn piece_id(s: &str) -> bool {
    s.len() >= 3
        && s.starts_with('"')
        && s.ends_with('"')
        && s.len() > 2
        && s[1..s.len() - 1]
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn cond(c: &str) -> std::result::Result<(), String> {
    let c = ws(c);
    if c.starts_with('"') {
        let Some((lit, rhs)) = c.rsplit_once('"').map(|(a, b)| (&c[..a.len() + 1], ws(b))) else {
            return Err("bad condition".into());
        };
        if !piece_id(lit) {
            return Err("only a lowercase piece id string is allowed".into());
        }
        let Some(rhs) = rhs.strip_prefix("in") else {
            return Err("expected `\"id\" in pieces`".into());
        };
        if rhs.starts_with(|ch: char| !ch.is_whitespace()) || ws(rhs) != "pieces" {
            return Err("expected `\"id\" in pieces`".into());
        }
        return Ok(());
    }
    path(c)?;
    if c == "pieces" {
        return Err("`{% if pieces %}` is not allowed; use `\"id\" in pieces`".into());
    }
    Ok(())
}

fn block(
    inner: &str,
    stack: &mut Vec<(Frame, usize)>,
    line: usize,
) -> std::result::Result<(), String> {
    let inner = ws(inner);
    let (kw, rest) = match inner.find(|c: char| c.is_whitespace()) {
        Some(i) => (&inner[..i], ws(&inner[i..])),
        None => (inner, ""),
    };
    match kw {
        "if" => {
            cond(rest)?;
            stack.push((Frame::If { seen_else: false }, line));
        }
        "elif" => {
            cond(rest)?;
            match stack.last() {
                Some((Frame::If { seen_else: false }, _)) => {}
                _ => return Err("`elif` without a matching open `if` (or after `else`)".into()),
            }
        }
        "else" | "endif" | "endfor" if !rest.is_empty() => {
            return Err(format!("`{kw}` takes no arguments"));
        }
        "else" => match stack.last_mut() {
            Some((Frame::If { seen_else }, _)) if !*seen_else => *seen_else = true,
            _ => return Err("`else` without a matching open `if`".into()),
        },
        "endif" => match stack.pop() {
            Some((Frame::If { .. }, _)) => {}
            _ => return Err("`endif` without a matching open `if`".into()),
        },
        "endfor" => match stack.pop() {
            Some((Frame::For, _)) => {}
            _ => return Err("`endfor` without a matching open `for`".into()),
        },
        "for" => {
            let Some((var, src)) = rest.split_once(char::is_whitespace) else {
                return Err("expected `for x in pieces`".into());
            };
            let src = ws(src);
            let Some(src) = src.strip_prefix("in") else {
                return Err("expected `for x in pieces`".into());
            };
            if !src.starts_with(char::is_whitespace) || ws(src) != "pieces" {
                return Err("only `for x in pieces` is allowed".into());
            }
            if !is_ident(var)
                || BAD_ROOTS.contains(&var)
                || KEYWORDS.contains(&var)
                || var == "pieces"
            {
                return Err(format!("`{var}` is not a valid loop variable"));
            }
            stack.push((Frame::For, line));
        }
        "raw" | "endraw" => return Err(format!("`{kw}` takes no arguments")),
        "" => return Err("empty tag (whitespace control `-` is not allowed)".into()),
        other => return Err(format!("tag `{other}` is not in the portable subset")),
    }
    Ok(())
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
            "{% if a %}",
            "{% endif %}",
            "{% if a %}{% endfor %}",
            "{% for p in pieces %}{% else %}{% endfor %}",
            "{% if a %}{% else %}{% elif b %}{% endif %}",
            "{{ a -}}",
            "{{ name",
            "{# open",
            "{#- c #}",
            "{# c -#}",
            "{#+ c #}",
            "{# c +#}",
            "{#-c-#}",
            "{{ piece }}",
            "{{ template }}",
            "{{ pieces }}",
            "{% if \"a%}b\" in pieces %}{% endif %}",
            "{% if 'x' in pieces %}{% endif %}",
            "{{ a | lower | upper }}",
            "{{{ x }}}",
            "{% if \"a\" inpieces %}{% endif %}",
            "{% for loop in pieces %}{% endfor %}",
            "{% if a %}{% endif %}{% endif %}",
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
    fn adversarial_accepts() {
        for t in [
            "{# {{ x ~ y }} {% set %} #}",
            "{% for p in pieces %}{% for q in pieces %}{{ p }}{{ q }}{% endfor %}{% endfor %}",
            "{{\n name\t|\r\nupper }}",
            "{ not a tag } {x}",
        ] {
            assert!(validate(t, "t").is_ok(), "{t}");
        }
    }

    #[test]
    fn reports_line() {
        let e = validate("a\n\nb {{ x ~ y }}", "f.jinja").unwrap_err();
        assert!(e.to_string().contains("f.jinja:3"), "{e}");
    }

    #[test]
    fn path_segments_allow_only_output() {
        assert!(validate_path_segment("{{ name }}-x", "p").is_ok());
        assert!(validate_path_segment("plain", "p").is_ok());
        assert!(validate_path_segment("{% if a %}x{% endif %}", "p").is_err());
        assert!(validate_path_segment("{# c #}", "p").is_err());
        assert!(validate_path_segment("{% raw %}x{% endraw %}", "p").is_err());
        assert!(validate_path_segment("{% raw %}", "p").is_err());
    }

    #[test]
    fn raw_errors_name_the_line() {
        let e = validate("a\nb\n{% raw %}\nx\ny", "f.jinja").unwrap_err();
        assert!(e.to_string().contains("f.jinja:3"), "{e}");
        let e = validate("a\n{% raw %}\nx\n{%- endraw %}", "f.jinja").unwrap_err();
        assert!(e.to_string().contains("f.jinja:4"), "{e}");
        let e = validate("{% raw %}a\n{% endraw %}\n{{ x ~ y }}", "f.jinja").unwrap_err();
        assert!(e.to_string().contains("f.jinja:3"), "{e}");
        // lines inside a raw body are counted
        let e = validate("{% raw %}\n\n{% endraw %}{% set %}", "f.jinja").unwrap_err();
        assert!(e.to_string().contains("f.jinja:3"), "{e}");
    }
}
