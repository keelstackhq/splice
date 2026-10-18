//! Safe rendered paths (spec §6 "Paths", controller ruling C8).

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

/// Render `/`-separated `raw` segment by segment. A rendered value must not be empty, `.`, `..`
/// or contain `/` or `\\`, so a variable can neither escape the repo nor add a segment (C8).
pub(crate) fn render_segments(
    raw: &str,
    renderer: &Renderer,
    ctx: &minijinja::Value,
    origin: &str,
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
    let src = src_rel.strip_suffix(".jinja").unwrap_or(src_rel);
    let key = render_segments(src, renderer, ctx, src_rel)?.join("/");
    let path = match target {
        Some(t) => {
            let mut segs = render_segments(t, renderer, ctx, &format!("target `{t}`"))?;
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
