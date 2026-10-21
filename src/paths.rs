//! Safe rendered paths (spec §6 "Paths", controller ruling C8).

use std::collections::HashMap;

use crate::error::SpliceError;
use crate::render::Renderer;

type Result<T> = std::result::Result<T, SpliceError>;

/// Path of `.splice/lock.toml`; never a rendered path.
const LOCK_PATH: &str = ".splice/lock.toml";

/// True for `.splice/channels` and everything below it (first two segments).
fn is_channels_path(path: &str) -> bool {
    let mut segs = path.split('/');
    segs.next() == Some(".splice") && segs.next() == Some("channels")
}

fn bad(origin: &str, why: &str) -> SpliceError {
    SpliceError::Template(format!("unsafe path {origin}: {why}"))
}

/// True for `CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9` in any case, with or without
/// an extension (the part before the first `.`, trailing spaces ignored, as Windows does).
fn is_windows_reserved(seg: &str) -> bool {
    let stem = seg.split('.').next().unwrap_or("").trim_end_matches(' ');
    let lower = stem.to_ascii_lowercase();
    match lower.as_str() {
        "con" | "prn" | "aux" | "nul" => true,
        _ => {
            let b = lower.as_bytes();
            b.len() == 4
                && (lower.starts_with("com") || lower.starts_with("lpt"))
                && (b'1'..=b'9').contains(&b[3])
        }
    }
}

/// Why `seg` cannot be a rendered path segment, piece id or instance name (contract §9.21), or
/// `None`. Mirrors hull's `segmentProblem` (`scripts/lib/segment.mjs`).
pub fn segment_problem(seg: &str) -> Option<String> {
    if seg.is_empty() {
        return Some("is empty".into());
    }
    if seg == "." || seg == ".." {
        return Some(format!("`{seg}` is not allowed"));
    }
    if seg
        .chars()
        .any(|c| matches!(c, '/' | '\\' | ':' | '\u{0}'..='\u{1f}' | '\u{7f}'))
    {
        return Some("contains a forbidden character (/ \\ : or control)".into());
    }
    if seg.ends_with('.') || seg.ends_with(' ') {
        return Some("ends with a dot or space".into());
    }
    if is_windows_reserved(seg) {
        return Some("is a Windows reserved name".into());
    }
    if seg.eq_ignore_ascii_case(".git") {
        return Some("is `.git`".into());
    }
    None
}

/// The first case-insensitive collision among the repo-relative `paths` (files), also as a
/// directory prefix (`A/x` and `a`), as a message naming both (contract §9.21).
pub fn case_collision<'a>(paths: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let paths: Vec<&str> = paths.into_iter().collect();
    // lowercase prefix -> (prefix as rendered, the file path it came from, is a directory)
    let mut folded: HashMap<String, (&str, &str, bool)> = HashMap::new();
    let mut note = |prefix: &'a str, from: &'a str, dir: bool| -> Option<String> {
        let key = prefix.to_lowercase();
        if let Some(&(prev, prev_from, prev_dir)) = folded.get(&key) {
            if prev != prefix {
                return Some(format!(
                    "case-insensitive path collision: `{prev_from}` and `{from}`"
                ));
            }
            if prev_dir != dir {
                return Some(format!(
                    "`{prefix}` is both a file and a directory (`{prev_from}` and `{from}`)"
                ));
            }
            return None;
        }
        folded.insert(key, (prefix, from, dir));
        None
    };
    for &path in &paths {
        for (i, _) in path.match_indices('/') {
            if let Some(e) = note(&path[..i], path, true) {
                return Some(e);
            }
        }
    }
    for &path in &paths {
        if let Some(e) = note(path, path, false) {
            return Some(e);
        }
    }
    None
}

/// Render `/`-separated `raw` segment by segment. A rendered value must not be empty, `.`, `..`
/// or contain `/` or `\\`, so a variable can neither escape the repo nor add a segment (C8).
pub(crate) fn render_segments(
    raw: &str,
    renderer: &Renderer,
    ctx: &minijinja::Value,
    origin: &str,
) -> Result<Vec<String>> {
    render_segments_with(raw, renderer, ctx, origin, true)
}

/// [`render_segments`]; `portable = false` (a merge base) skips the contract §9.21 rules and
/// keeps only the escape checks (empty, `.`, `..`, separators).
fn render_segments_with(
    raw: &str,
    renderer: &Renderer,
    ctx: &minijinja::Value,
    origin: &str,
    portable: bool,
) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for seg in raw.split('/') {
        let value = renderer.render_path_segment(seg, ctx, origin)?;
        if value.is_empty() {
            return Err(bad(origin, &format!("segment `{seg}` renders empty")));
        }
        if value == "." || value == ".." {
            return Err(bad(
                origin,
                &format!("segment `{seg}` renders to `{value}`"),
            ));
        }
        if value.contains('/') || value.contains('\\') {
            return Err(bad(
                origin,
                &format!("segment `{seg}` renders to `{value}`, which contains a path separator"),
            ));
        }
        if portable && let Some(why) = segment_problem(&value) {
            return Err(bad(origin, &format!("segment `{value}` {why}")));
        }
        out.push(value);
    }
    Ok(out)
}

/// Render a path below `files/` (and its piece `target`). Returns the final repo-relative path
/// and the class key: the rendered path without `.jinja` and without the target (contract §9.1).
pub fn render_rel_path(
    src_rel: &str,
    target: Option<&str>,
    renderer: &Renderer,
    ctx: &minijinja::Value,
) -> Result<(String, String)> {
    render_rel_path_with(src_rel, target, renderer, ctx, true)
}

/// [`render_rel_path`] with the contract §9.21 segment rules on or off (off for a merge base).
pub fn render_rel_path_with(
    src_rel: &str,
    target: Option<&str>,
    renderer: &Renderer,
    ctx: &minijinja::Value,
    portable: bool,
) -> Result<(String, String)> {
    let src = src_rel.strip_suffix(".jinja").unwrap_or(src_rel);
    let key = render_segments_with(src, renderer, ctx, src_rel, portable)?.join("/");
    let path = match target {
        Some(t) => {
            let mut segs =
                render_segments_with(t, renderer, ctx, &format!("target `{t}`"), portable)?;
            segs.push(key.clone());
            segs.join("/")
        }
        None => key.clone(),
    };
    if path == LOCK_PATH || key == LOCK_PATH {
        return Err(bad(src_rel, "`.splice/lock.toml` is reserved"));
    }
    if is_channels_path(&path) || is_channels_path(&key) {
        return Err(bad(src_rel, "`.splice/channels/` is reserved"));
    }
    Ok((path, key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use minijinja::context;

    fn render_with_name_var(src: &str, name: &str) -> Result<(String, String)> {
        let ctx = context! { name => name };
        render_rel_path(src, None, &Renderer::new(), &ctx)
    }

    #[test]
    fn unsafe_segment_values_rejected() {
        for v in ["../../etc", "a/b", "a\\b", "", ".", "..", "/abs", "x/"] {
            let e = render_with_name_var("{{ name }}", v).unwrap_err();
            assert_eq!(e.exit_code(), 3, "{v:?}");
        }
    }

    #[test]
    fn non_portable_segment_values_rejected() {
        for v in [
            "a:b",
            "c:",
            "a\u{1}b",
            "tab\t",
            "a\u{7f}",
            "x.",
            "x ",
            "CON",
            "con",
            "Nul",
            "con.txt",
            "aux.tar.gz",
            "com1",
            "LPT9",
            "COM9.x",
            ".git",
            ".GIT",
            ".Git",
        ] {
            let e = render_with_name_var("{{ name }}", v).unwrap_err();
            assert_eq!(e.exit_code(), 3, "{v:?}");
            assert!(e.to_string().contains("unsafe path"), "{v:?}: {e}");
        }
        for v in [
            "com0",
            "com10",
            "lpt",
            "conx",
            "x.con",
            ".gitignore",
            "git",
            "a.b",
            "a b",
        ] {
            assert!(render_with_name_var("{{ name }}", v).is_ok(), "{v:?}");
        }
    }

    #[test]
    fn non_portable_literal_names_rejected() {
        let ctx = context! {};
        let r = Renderer::new();
        for (src, target) in [
            ("a:b.txt", None),
            ("dir./x", None),
            ("x ", None),
            ("nul.jinja", None),
            ("CON.txt", None),
            ("x", Some("aux")),
            (".git/config", None),
            ("x/.Git/y", None),
        ] {
            let e = render_rel_path(src, target, &r, &ctx).unwrap_err();
            assert_eq!(e.exit_code(), 3, "{src:?} {target:?}");
        }
    }

    #[test]
    fn segment_problem_names_the_reason() {
        assert_eq!(segment_problem("ok.txt"), None);
        assert!(
            segment_problem("a:b")
                .unwrap()
                .contains("forbidden character")
        );
        assert!(segment_problem("a.").unwrap().contains("ends with"));
        assert!(
            segment_problem("con.txt")
                .unwrap()
                .contains("Windows reserved")
        );
        assert!(segment_problem(".GIT").unwrap().contains(".git"));
        assert!(segment_problem("").unwrap().contains("empty"));
    }

    #[test]
    fn case_collisions_found_also_as_directory_prefixes() {
        assert!(case_collision(["a/x", "b/x", "A.txt"]).is_none());
        let e = case_collision(["README.md", "readme.md"]).unwrap();
        assert!(e.contains("README.md") && e.contains("readme.md"), "{e}");
        let e = case_collision(["A/x", "a"]).unwrap();
        assert!(e.contains("A/x") && e.contains("`a`"), "{e}");
        let e = case_collision(["Dir/x", "dir/y"]).unwrap();
        assert!(e.contains("Dir/x") && e.contains("dir/y"), "{e}");
        let e = case_collision(["a", "a/x"]).unwrap();
        assert!(e.contains("`a`") && e.contains("a/x"), "{e}");
    }

    #[test]
    fn jinja_suffix_stripped_and_key_excludes_target() {
        let ctx = context! { name => "web" };
        let r = Renderer::new();
        let (path, key) =
            render_rel_path("src/{{ name }}.ts.jinja", Some("apps/{{ name }}"), &r, &ctx).unwrap();
        assert_eq!(path, "apps/web/src/web.ts");
        assert_eq!(key, "src/web.ts");
        let (path, key) = render_rel_path("a.txt", None, &r, &ctx).unwrap();
        assert_eq!((path.as_str(), key.as_str()), ("a.txt", "a.txt"));
    }

    #[test]
    fn bad_target_and_source_shapes_rejected() {
        let ctx = context! { name => "web" };
        let r = Renderer::new();
        for (src, target) in [
            ("a/../b", None),
            ("./a", None),
            ("a//b", None),
            ("a\\b", None),
            ("/a", None),
            ("a", Some("../x")),
            ("a", Some("/x")),
            ("a", Some("")),
            (".splice/lock.toml", None),
            ("lock.toml", Some(".splice")),
        ] {
            let e = render_rel_path(src, target, &r, &ctx).unwrap_err();
            assert_eq!(e.exit_code(), 3, "{src:?} {target:?}");
        }
    }

    #[test]
    fn reserved_splice_paths() {
        let ctx = context! {};
        let r = Renderer::new();
        for (src, target) in [
            (".splice/channels/a.toml", None),
            ("channels/a.toml", Some(".splice")),
            (".splice/lock.toml", None),
        ] {
            let e = render_rel_path(src, target, &r, &ctx).unwrap_err();
            assert_eq!(e.exit_code(), 3, "{src:?} {target:?}");
        }
        for ok in [
            ".splice/migrations/x.mjs",
            ".splicex/a",
            ".splice/channelsx/a",
        ] {
            assert!(render_rel_path(ok, None, &r, &ctx).is_ok(), "{ok}");
        }
    }
}
