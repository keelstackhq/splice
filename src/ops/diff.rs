//! `splice diff` (spec §8.8): the plan `update` would apply, printed, with nothing written.

use indexmap::IndexMap;

use crate::cli::{DiffArgs, GlobalArgs};
use crate::error::SpliceError;
use crate::fsio;
use crate::output::{Action, Report};
use crate::planner::Planned;

use super::update::{parse_to, prepare};

type Result<T> = std::result::Result<T, SpliceError>;

/// Runs `update` steps 2–6 (no dirty check: nothing is written). Human output: a unified diff
/// per written file (`a/`, `b/` labels; `/dev/null` for a new file) and one line per removal.
/// `--json`: the planned `changes`. Drift and conflicts are reported, never an error (exit 0).
pub fn run(g: &GlobalArgs, a: &DiffArgs) -> Result<Report> {
    if a.channel.is_some() {
        return Err(super::channel::not_implemented("diff --channel"));
    }
    let repo = g.dir.as_path();
    let to = parse_to(a.to.as_deref())?;
    let prep = prepare(g, to.as_ref(), &IndexMap::new(), false)?;
    let mut report = Report::new("diff");
    report.messages = prep.messages;
    for c in &prep.plan.changes {
        report.push(&c.path, c.action, c.class);
        if let Some(n) = &c.note {
            report.msg(n.clone());
        }
        if !g.json {
            report.lines.extend(patch_lines(repo, c)?);
        }
    }
    for path in &prep.plan.drift {
        report.msg(format!(
            "`{path}` is owned by the template but was edited locally; `splice update` refuses \
             it without --force"
        ));
    }
    Ok(report)
}

/// The human lines for one change: a removal line, a unified diff, or nothing (`skip`).
fn patch_lines(repo: &std::path::Path, c: &Planned) -> Result<Vec<String>> {
    if c.action == Action::Delete {
        return Ok(vec![format!("delete {}", c.path)]);
    }
    let Some(new) = &c.bytes else {
        return Ok(Vec::new());
    };
    fsio::check_target(repo, &c.path)?;
    let old = fsio::read_opt(repo, &c.path)?;
    let (Ok(old_text), Ok(new_text)) = (
        std::str::from_utf8(old.as_deref().unwrap_or_default()),
        std::str::from_utf8(new),
    ) else {
        return Ok(vec![format!("Binary file {} differs", c.path)]);
    };
    let mut opts = diffy::DiffOptions::new();
    opts.set_original_filename(match old {
        Some(_) => format!("a/{}", c.path),
        None => "/dev/null".to_string(),
    })
    .set_modified_filename(format!("b/{}", c.path));
    let patch = opts.create_patch(old_text, new_text).to_string();
    Ok(patch.lines().map(str::to_string).collect())
}
