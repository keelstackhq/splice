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
    while let Some(off) = text[pos..].find('{') {
        let start = pos + off;
        let line = text[..start].matches('\n').count() + 1;
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
        match kind {
            'c' if blocks => {}
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

fn ws(s: &str) -> &str {
    s.trim_matches([' ', '\t', '\n', '\r'])
}

fn is_ident(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(ch) if ch.is_ascii_alphabetic() || ch == '_')
        && c.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
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
    if p == "pieces" {
        return Err("`pieces` cannot be printed directly".into());
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
            "{% raw %}{% endraw %}",
            "{{ \"lit\" }}",
            "{% if a %}",
            "{% endif %}",
            "{% if a %}{% endfor %}",
            "{% for p in pieces %}{% else %}{% endfor %}",
            "{% if a %}{% else %}{% elif b %}{% endif %}",
            "{{ a -}}",
            "{{ name",
            "{# open",
        ] {
            let e = validate(t, "f.jinja").unwrap_err();
            assert_eq!(e.exit_code(), 3, "{t}");
            assert!(e.to_string().contains("f.jinja:1"), "{t}: {e}");
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
    }
}
