//! Commands (spec §8) and the helpers they share.

use crate::error::SpliceError;
use crate::paths::validate_segment;
use crate::post::Undo;
use indexmap::IndexMap;
use std::path::{Path, PathBuf};

pub mod add;
pub mod check;
pub mod diff;
pub mod explain;
pub mod new;
pub mod update;

use crate::fetch::{Fetcher, ResolvedTemplate};
use crate::lock::Lock;
use crate::manifest::Template;
use crate::source::TemplateSource;

type Result<T> = std::result::Result<T, SpliceError>;

pub(crate) fn usage(msg: impl Into<String>) -> SpliceError {
    SpliceError::Usage(msg.into())
}

/// `K=V` flags into a map; the first `=` splits, later flags win.
pub(crate) fn parse_data(raw: &[String]) -> Result<IndexMap<String, String>> {
    let mut m = IndexMap::new();
    for item in raw {
        let (k, v) = item
            .split_once('=')
            .filter(|(k, _)| !k.is_empty())
            .ok_or_else(|| usage(format!("invalid --data `{item}`: expected K=V")))?;
        m.insert(k.to_string(), v.to_string());
    }
    Ok(m)
}

/// A name that becomes a path segment must be safe: a bad value is a usage error (exit 2).
pub(crate) fn safe_segment(what: &str, v: &str) -> Result<()> {
    validate_segment(v).map_err(|e| usage(format!("{what} {v:?} is not a safe name: {e}")))
}

/// Refuse to write through a symlink or over a directory below `dest` (checked before any write).
fn unsafe_path(msg: String) -> SpliceError {
    SpliceError::Template(format!("unsafe path: {msg}"))
}

pub(crate) fn check_target(dest: &Path, rel: &str) -> Result<PathBuf> {
    check_path(dest, rel, true)
}

/// Like `check_target` for read-only callers (`check`): the message does not talk about writing.
pub(crate) fn check_readable(dest: &Path, rel: &str) -> Result<PathBuf> {
    check_path(dest, rel, false)
}

fn check_path(dest: &Path, rel: &str, write: bool) -> Result<PathBuf> {
    let mut p = dest.to_path_buf();
    let segs: Vec<&str> = rel.split('/').collect();
    for (i, seg) in segs.iter().enumerate() {
        p.push(seg);
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.file_type().is_symlink() => {
                let what = if write {
                    "refusing to write through symlink"
                } else {
                    "symlink in the way:"
                };
                return Err(unsafe_path(format!("{what} {}", p.display())));
            }
            Ok(m) if i + 1 == segs.len() && m.is_dir() => {
                return Err(unsafe_path(format!("{rel} exists and is a directory")));
            }
            Ok(m) if i + 1 < segs.len() && !m.is_dir() => {
                return Err(unsafe_path(format!(
                    "{} exists and is not a directory",
                    p.display()
                )));
            }
            _ => {}
        }
    }
    Ok(p)
}

/// The lock of an existing repo with the template checked out at its commit.
pub(crate) struct Locked {
    pub lock: Lock,
    pub warnings: Vec<String>,
    pub resolved: ResolvedTemplate,
    pub template: Template,
}

/// Read the lock (and its warnings), check out the template at the locked commit, load it and
/// check `min_splice`. Shared by every command that works on an existing instance.
pub(crate) fn load_locked(repo: &Path) -> Result<Locked> {
    let (lock, warnings) = Lock::read_with_warnings(repo)?.ok_or_else(|| no_lock(repo))?;
    let source = TemplateSource::parse_from(&lock.template.source, repo)?;
    let resolved = Fetcher::from_env().checkout(
        &source,
        &lock.template.commit,
        lock.template.version.clone(),
    )?;
    let template = Template::load(&resolved.root)?;
    template.check_min_splice(
        &semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version is semver"),
    )?;
    Ok(Locked {
        lock,
        warnings,
        resolved,
        template,
    })
}

/// Checks on one piece instance about to be added (to a new repo or an existing one):
/// `present` holds the ids already chosen or locked.
pub(crate) fn validate_instance(
    template: &Template,
    id: &str,
    named: bool,
    present: &[&str],
) -> Result<()> {
    let piece = template
        .pieces
        .get(id)
        .ok_or_else(|| usage(format!("unknown piece `{id}`")))?;
    if id == template.manifest.base {
        return Err(usage(format!(
            "`{id}` is the base piece and is always included"
        )));
    }
    if named && !piece.multiple {
        return Err(usage(format!(
            "piece `{id}` is not `multiple`; it cannot be named"
        )));
    }
    if !piece.multiple && present.contains(&id) {
        return Err(usage(format!(
            "piece `{id}` is not `multiple` and is already present"
        )));
    }
    Ok(())
}

/// Every piece `id` requires must be in `present`; `fix` completes the hint (`{r}` is the missing id).
pub(crate) fn check_requires(
    template: &Template,
    id: &str,
    present: &[&str],
    fix: &str,
) -> Result<()> {
    for r in &template.pieces[id].requires {
        if !present.contains(&r.as_str()) {
            return Err(usage(format!(
                "piece `{id}` requires piece `{r}`; {}",
                fix.replace("{r}", r)
            )));
        }
    }
    Ok(())
}

/// Spec §8.3 step 1: refuse a dirty tree, or no git work tree, with exit 2 unless `allow_dirty`.
/// `verb` completes the hint (`pass --allow-dirty to {verb} anyway`). The result says what git
/// can undo afterwards: only a tree verified clean may be told to `git checkout`/`git clean`
/// (spec §10 item 25).
pub(crate) fn ensure_clean(repo: &Path, allow_dirty: bool, verb: &str) -> Result<Undo> {
    let state = crate::git::Git::is_clean(repo);
    if allow_dirty {
        return Ok(match state {
            Ok(Some(true)) => Undo::Git,
            Ok(None) => Undo::NoGit,
            _ => Undo::Unverified,
        });
    }
    match state? {
        None => Err(usage(format!(
            "{} is not inside a git work tree; pass --allow-dirty to {verb} anyway",
            repo.display()
        ))),
        Some(false) => Err(usage(
            "the working tree has uncommitted changes; commit or stash them, or pass \
             --allow-dirty",
        )),
        Some(true) => Ok(Undo::Git),
    }
}

/// `--to` as a version: `1.2.3` or `v1.2.3`.
pub(crate) fn parse_to(raw: Option<&str>) -> Result<Option<semver::Version>> {
    raw.map(|v| {
        semver::Version::parse(v.strip_prefix('v').unwrap_or(v))
            .map_err(|e| usage(format!("invalid --to version {v:?}: {e}")))
    })
    .transpose()
}

/// A repo without a lock is a usage error (checked before the dirty check can mislead).
pub(crate) fn no_lock(repo: &Path) -> SpliceError {
    usage(format!(
        "{} has no .splice/lock.toml; use `splice new` first",
        repo.display()
    ))
}

/// Exit 1 when the plan was refused (owned drift, or unresolved markers in a file that would
/// be merged): a report with no changes, one message per file, then `nothing was written`.
/// `None` when the plan is fine. `verb` is `update` or `add`.
pub(crate) fn refusal(
    verb: &str,
    plan: &crate::planner::Plan,
    messages: &[String],
) -> Option<SpliceError> {
    if plan.drift.is_empty() && plan.marked.is_empty() {
        return None;
    }
    let mut report = crate::output::Report::new(verb);
    report.messages.extend(messages.iter().cloned());
    for d in &plan.drift {
        report.msg(format!(
            "{d}: owned file was edited or deleted locally; {verb} refused (--force replaces it)"
        ));
    }
    for m in &plan.marked {
        report.msg(format!(
            "resolve conflict markers in {m} first (splice check)"
        ));
    }
    report.msg("nothing was written");
    Some(SpliceError::Drift(report))
}

/// Put `msgs` (lock and template warnings, notes) in front of the messages of an apply result.
pub(crate) fn with_messages(
    result: Result<crate::output::Report>,
    msgs: &[String],
) -> Result<crate::output::Report> {
    let prepend = |r: &mut crate::output::Report| {
        r.messages.splice(0..0, msgs.iter().cloned());
    };
    match result {
        Ok(mut r) => {
            prepend(&mut r);
            Ok(r)
        }
        Err(SpliceError::Conflict(mut r)) => {
            prepend(&mut r);
            Err(SpliceError::Conflict(r))
        }
        Err(SpliceError::Partial {
            message,
            mut report,
        }) => {
            prepend(&mut report);
            Err(SpliceError::Partial { message, report })
        }
        Err(e) => Err(e),
    }
}
