//! Safe rendered paths (spec §6 Paths).

use crate::error::SpliceError;
use crate::render::Renderer;
use crate::subset;
use minijinja::Value;

const LOCK_PATH: &str = ".splice/lock.toml";

fn bad(origin: &str, msg: impl std::fmt::Display) -> SpliceError {
    SpliceError::Template(format!("unsafe path `{origin}`: {msg}"))
}

/// Render one `/`-free segment and check the result is still exactly one safe segment.
fn render_segment(
    seg: &str,
    full: &str,
    renderer: &Renderer,
    ctx: &Value,
) -> Result<String, SpliceError> {
    subset::validate_path_segment(seg, full)?;
    let out = renderer.render_str(seg, ctx, full)?;
    if out.is_empty() || out == "." || out == ".." {
        return Err(bad(full, format!("segment `{seg}` renders to `{out}`")));
    }
    if out.contains('/') || out.contains('\\') || out.contains('\0') || out.contains('\n') {
        return Err(bad(
            full,
            format!("segment `{seg}` renders to {out:?}, which is not one path segment"),
        ));
    }
    Ok(out)
}

fn render_path(raw: &str, renderer: &Renderer, ctx: &Value) -> Result<Vec<String>, SpliceError> {
    if raw.contains('\\') {
        return Err(bad(raw, "backslash in path"));
    }
    raw.split('/')
        .map(|seg| render_segment(seg, raw, renderer, ctx))
        .collect()
}

/// Render `src_rel` (relative to `files/`, `/`-separated) and the optional `target`.
/// Returns `(final path, class key)`; the class key is rendered, without `.jinja` and without
/// the target prefix.
pub fn render_rel_path(
    src_rel: &str,
    target: Option<&str>,
    renderer: &Renderer,
    ctx: &Value,
) -> Result<(String, String), SpliceError> {
    let stripped = src_rel.strip_suffix(".jinja").unwrap_or(src_rel);
    let key = render_path(stripped, renderer, ctx)?.join("/");
    let fin = match target {
        Some(t) => {
            let mut segs = render_path(t, renderer, ctx)?;
            segs.push(key.clone());
            segs.join("/")
        }
        None => key.clone(),
    };
    if fin == LOCK_PATH {
        return Err(bad(
            &fin,
            "`.splice/lock.toml` is reserved for the lock file",
        ));
    }
    Ok((fin, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(name: &str) -> Value {
        Value::from_serialize(serde_json::json!({ "name": name }))
    }

    fn render_with_name_var(v: &str) -> Result<(String, String), SpliceError> {
        render_rel_path("{{ name }}/f.txt", None, &Renderer::new(), &ctx(v))
    }

    #[test]
    fn unsafe_paths_rejected() {
        for v in ["../../etc", "a/b", "", ".", "..", "a\\b", "/abs"] {
            assert_eq!(render_with_name_var(v).unwrap_err().exit_code(), 3, "{v}");
        }
    }

    #[test]
    fn safe_paths_render() {
        let r = Renderer::new();
        let (fin, key) = render_rel_path(
            "src/{{ name }}.ts.jinja",
            Some("apps/{{ name }}"),
            &r,
            &ctx("web"),
        )
        .unwrap();
        assert_eq!(key, "src/web.ts");
        assert_eq!(fin, "apps/web/src/web.ts");
    }

    #[test]
    fn target_is_checked_too() {
        let r = Renderer::new();
        let bad = |t: &str, n: &str| render_rel_path("f", Some(t), &r, &ctx(n)).is_err();
        assert!(bad("{{ name }}", ".."));
        assert!(bad("/abs", "x"));
        assert!(bad("a//b", "x"));
        assert!(bad("{{ name }}", "a/b"));
    }

    #[test]
    fn lock_path_is_reserved() {
        let r = Renderer::new();
        let e = render_rel_path(".splice/lock.toml", None, &r, &ctx("x")).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(render_rel_path("lock.toml", Some(".splice"), &r, &ctx("x")).is_err());
        assert!(render_rel_path("lock.toml.jinja", Some(".splice"), &r, &ctx("x")).is_err());
        assert!(render_rel_path(".splice/other", None, &r, &ctx("x")).is_ok());
    }

    #[test]
    fn syntax_outside_path_subset_rejected() {
        let r = Renderer::new();
        assert!(render_rel_path("{% if a %}x{% endif %}", None, &r, &ctx("x")).is_err());
    }
}
