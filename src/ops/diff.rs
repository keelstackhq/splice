//! `splice diff` (spec §8.8): the plan `update` would apply, printed, with nothing written.

use indexmap::IndexMap;

use crate::cli::{DiffArgs, GlobalArgs};
use crate::error::SpliceError;
use crate::fsio;
use crate::output::{Action, Class, Report};
use crate::sources::Sources;

use super::update::{Written, parse_to, prepare, prepare_channel};

type Result<T> = std::result::Result<T, SpliceError>;

/// Runs `update` steps 2–6 (no dirty check: nothing is written). Human output: a unified diff
/// per written file (`a/`, `b/` labels; `/dev/null` for a new file) and one line per removal.
/// `--json`: the planned `changes`. Drift and conflicts are reported, never an error (exit 0).
pub fn run(g: &GlobalArgs, a: &DiffArgs) -> Result<Report> {
    let repo = g.dir.as_path();
    let to = parse_to(a.to.as_deref())?;
    if let Some(name) = &a.channel {
        return run_channel(g, name, to.as_ref());
    }
    let prep = prepare(g, to.as_ref(), &IndexMap::new(), false)?;
    let mut report = Report::new("diff");
    report.messages = prep.messages;
    for c in &prep.plan.changes {
        report.push(&c.path, c.action, c.class);
        if let Some(n) = &c.note {
            report.msg(n.clone());
        }
        if !g.json {
            report
                .lines
                .extend(patch_lines(repo, &c.path, c.action, c.bytes.as_deref())?);
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

/// `diff --channel NAME`: the plan of `update --channel NAME` (files and sections), printed and
/// not written. `--json` changes equal those of `update --channel NAME --json`; drift and broken
/// markers are messages, never an error (exit 0).
fn run_channel(g: &GlobalArgs, name: &str, to: Option<&semver::Version>) -> Result<Report> {
    let repo = g.dir.as_path();
    let sources = Sources::load(repo)?;
    let cl = sources.channel(name)?;
    let prep = prepare_channel(g, cl, &sources.claims(), to, None, false, &Written::new())?;
    let mut report = Report::new("diff");
    report.messages = prep.messages;
    let ch = prep.channel.as_ref().expect("a channel update");
    for c in &prep.plan.changes {
        report.push(&c.path, c.action, c.class);
        if let Some(n) = &c.note {
            report.msg(n.clone());
        }
        if !g.json {
            report
                .lines
                .extend(patch_lines(repo, &c.path, c.action, c.bytes.as_deref())?);
        }
    }
    for s in &ch.sections.changes {
        report.push(s.report_path(name), s.action, Class::Owned);
        if let Some(n) = &s.note {
            report.msg(n.clone());
        }
        if !g.json {
            report
                .lines
                .extend(patch_lines(repo, &s.host, s.action, s.bytes.as_deref())?);
        }
    }
    for path in prep.plan.drift.iter().chain(&ch.sections.drift) {
        report.msg(format!(
            "`{path}` is owned by channel {name} but was edited locally; `splice update \
             --channel {name}` refuses it without --force"
        ));
    }
    report.messages.extend(ch.sections.broken.iter().cloned());
    Ok(report)
}

/// The human lines for one change: a removal line, a unified diff, or nothing (`skip`).
fn patch_lines(
    repo: &std::path::Path,
    path: &str,
    action: Action,
    new: Option<&[u8]>,
) -> Result<Vec<String>> {
    if action == Action::Delete && new.is_none() {
        return Ok(vec![format!("delete {path}")]);
    }
    let Some(new) = new else {
        return Ok(Vec::new());
    };
    fsio::check_target(repo, path)?;
    let old = fsio::read_opt(repo, path)?;
    let (Ok(old_text), Ok(new_text)) = (
        std::str::from_utf8(old.as_deref().unwrap_or_default()),
        std::str::from_utf8(new),
    ) else {
        return Ok(vec![format!("Binary file {path} differs")]);
    };
    let mut opts = diffy::DiffOptions::new();
    opts.set_original_filename(match old {
        Some(_) => format!("a/{path}"),
        None => "/dev/null".to_string(),
    })
    .set_modified_filename(format!("b/{path}"));
    let patch = opts.create_patch(old_text, new_text).to_string();
    Ok(patch.lines().map(str::to_string).collect())
}
