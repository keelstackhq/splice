//! `splice diff` (spec §8.8): steps 2-6 of `update` without writing anything.

use super::usage;
use crate::cli::{DiffArgs, GlobalArgs};
use crate::error::SpliceError;
use crate::fsio::read_opt;
use crate::ops::update::prepare;
use crate::output::{Action, Report};
use crate::planner::Planned;
use indexmap::IndexMap;
use semver::Version;
use std::path::Path;

type Result<T> = std::result::Result<T, SpliceError>;

/// The unified patch of one planned write, `a/` and `b/` paths (`/dev/null` for a side that does
/// not exist). Non-UTF-8 content is reported as binary.
fn patch_lines(repo: &Path, c: &Planned, new: &[u8]) -> Result<Vec<String>> {
    let old = read_opt(repo, &c.path)?;
    let (Ok(o), Ok(n)) = (
        std::str::from_utf8(old.as_deref().unwrap_or_default()),
        std::str::from_utf8(new),
    ) else {
        return Ok(vec![format!("Binary files a/{0} and b/{0} differ", c.path)]);
    };
    let name = |side: &str, exists: bool| {
        if exists {
            format!("{side}/{}", c.path)
        } else {
            "/dev/null".to_string()
        }
    };
    let patch = diffy::DiffOptions::new()
        .set_original_filename(name("a", old.is_some()))
        .set_modified_filename(name("b", true))
        .create_patch(o, n);
    Ok(patch.to_string().lines().map(str::to_string).collect())
}

pub fn run(g: &GlobalArgs, a: &DiffArgs) -> Result<Report> {
    let repo = g.dir.as_path();
    let to =
        a.to.as_deref()
            .map(|v| {
                Version::parse(v.strip_prefix('v').unwrap_or(v))
                    .map_err(|e| usage(format!("invalid --to version {v:?}: {e}")))
            })
            .transpose()?;

    // never prompts and never forces: new vars take their default, and owned drift is reported
    // rather than planned as an update
    let p = prepare(g, to.as_ref(), &IndexMap::new(), false, false).map_err(|e| match e {
        SpliceError::Usage(m) if m.starts_with("missing value for var") => usage(format!(
            "{m} (a var that is new in the target and has no default; `splice diff` does not ask: \
             run `splice update --data NAME=VALUE`)"
        )),
        e => e,
    })?;

    let mut report = Report::new("diff");
    report.messages.extend(p.messages.iter().cloned());
    for d in &p.plan.drift {
        report.msg(format!(
            "{d}: owned file was edited or deleted locally; update would refuse (--force replaces it)"
        ));
    }
    for c in &p.plan.changes {
        match (&c.bytes, c.action) {
            (Some(bytes), _) => report.lines.extend(patch_lines(repo, c, bytes)?),
            (None, Action::Delete) => report.lines.push(format!("delete   {}", c.path)),
            _ => {}
        }
        if let Some(n) = &c.note {
            report.msg(format!("{}: {n}", c.path));
        }
        report.push(c.path.clone(), c.action, c.class);
    }
    Ok(report)
}
