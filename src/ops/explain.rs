//! `splice explain` (spec §8.9): which piece, class, glob and merge strategy own a path.

use super::usage;
use crate::cli::{ExplainArgs, GlobalArgs};
use crate::error::SpliceError;
use crate::fetch::Fetcher;
use crate::lock::Lock;
use crate::manifest::{MergeStrategy, Template};
use crate::output::{Action, Report};
use crate::render::{RenderInput, render_tree};
use crate::source::TemplateSource;

type Result<T> = std::result::Result<T, SpliceError>;

/// `\` to `/`, then drop `.` segments and empty segments. `None` when the path can never name a
/// rendered file: absolute, a drive prefix, or containing `..`.
fn normalise(raw: &str) -> Option<String> {
    let p = raw.replace('\\', "/");
    if p.starts_with('/') || p.split('/').next().is_some_and(|s| s.contains(':')) {
        return None;
    }
    let mut segs = Vec::new();
    for s in p.split('/') {
        match s {
            "" | "." => {}
            ".." => return None,
            s => segs.push(s),
        }
    }
    (!segs.is_empty()).then(|| segs.join("/"))
}

fn strategy_name(s: MergeStrategy) -> &'static str {
    match s {
        MergeStrategy::Json => "json",
        MergeStrategy::Yaml => "yaml",
        MergeStrategy::Toml => "toml",
        MergeStrategy::Text => "text",
    }
}

pub fn run(g: &GlobalArgs, a: &ExplainArgs) -> Result<Report> {
    let repo = g.dir.as_path();
    let (lock, lock_warnings) = Lock::read_with_warnings(repo)?.ok_or_else(|| {
        usage(format!(
            "{} has no .splice/lock.toml; use `splice new` first",
            repo.display()
        ))
    })?;
    let source = TemplateSource::parse(&lock.template.source)?;
    let resolved = Fetcher::from_env().checkout(
        &source,
        &lock.template.commit,
        lock.template.version.clone(),
    )?;
    let template = Template::load(&resolved.root)?;
    let tree = render_tree(
        &template,
        &RenderInput {
            template_vars: lock.vars.clone(),
            instances: lock.instances(),
            version: resolved.version.clone(),
        },
    )?;

    let mut report = Report::new("explain");
    for w in lock_warnings.iter().chain(&template.warnings) {
        report.msg(w.clone());
    }
    let norm = normalise(&a.path);
    let found = norm.as_deref().and_then(|p| tree.get(p));
    match found {
        Some(f) => {
            let glob = f.glob.as_deref().unwrap_or("default");
            let merge = strategy_name(f.strategy);
            let piece = format!("{}/{}", f.piece_id, f.piece_name);
            report.push(f.path.clone(), Action::Skip, f.class);
            report.msg(format!("piece={piece}"));
            report.msg(format!("glob={glob}"));
            report.msg(format!("merge={merge}"));
            let class = serde_json::to_value(f.class)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
            report.lines.push(format!(
                "{}  {class}  {piece}  glob={glob}  merge={merge}",
                f.path
            ));
        }
        None => match &norm {
            Some(p) => {
                report.msg("unmanaged");
                report.lines.push(format!("{p}  unmanaged"));
            }
            None => {
                report.msg("unmanaged (not a repo-relative path)");
                report.lines.push(format!("{}  unmanaged", a.path));
            }
        },
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::normalise;

    #[test]
    fn normalises_paths() {
        assert_eq!(normalise("./a/b").as_deref(), Some("a/b"));
        assert_eq!(normalise("a\\b").as_deref(), Some("a/b"));
        assert_eq!(normalise("a//b/./c").as_deref(), Some("a/b/c"));
        assert_eq!(normalise("../a"), None);
        assert_eq!(normalise("a/../b"), None);
        assert_eq!(normalise("/etc/x"), None);
        assert_eq!(normalise("C:\\x"), None);
        assert_eq!(normalise("."), None);
    }
}
