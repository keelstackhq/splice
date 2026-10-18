//! `splice check` (spec §8.7): read-only drift, conflict-marker and update-available gate.

use crate::cli::{CheckArgs, GlobalArgs};
use std::path::Path;

use crate::error::SpliceError;
use crate::fetch::Fetcher;
use crate::fsio::{check_target, read_opt, sha256_hex};
use crate::lock::{ChannelLock, LOCK_PATH, Lock};
use crate::markers::find_markers;
use crate::output::{Action, Class, Report};
use crate::sections::{body_sha, find_block};
use crate::source::TemplateSource;
use crate::sources::Sources;

type Result<T> = std::result::Result<T, SpliceError>;

/// The newest stable version of the locked template, if it is newer than the lock.
fn newer_stable(lock: &Lock) -> Result<Option<semver::Version>> {
    let src = TemplateSource::parse(&lock.template.source)?;
    let versions = Fetcher::from_env().available_versions(&src, false)?;
    Ok(versions
        .into_iter()
        .max()
        .filter(|v| *v > lock.template.version))
}

/// Owned drift and conflict markers of one lock's files. `prefix` is `""` for the main lock and
/// `channel NAME: ` for a channel, so main messages stay exactly as in M2. Returns the number of
/// drifted owned files.
fn check_files(repo: &Path, lock: &Lock, prefix: &str, report: &mut Report) -> Result<usize> {
    // Every lock path is checked before any is read: no `..`, no absolute path, no symlink.
    for path in lock.files.keys() {
        check_target(repo, path)
            .map_err(|e| SpliceError::Template(format!("{LOCK_PATH} lists an unsafe path: {e}")))?;
    }
    let mut drifted = 0;
    for (path, file) in &lock.files {
        let bytes = read_opt(repo, path)?;
        if file.class == Class::Owned
            && bytes.as_deref().map(sha256_hex).as_deref() != Some(file.sha256.as_str())
        {
            report.push(path, Action::Update, Class::Owned);
            drifted += 1;
        }
        if let Some(bytes) = bytes {
            let lines = find_markers(&String::from_utf8_lossy(&bytes));
            if !lines.is_empty() {
                report.push(path, Action::Conflict, file.class);
                report.msg(format!(
                    "{prefix}`{path}` has unresolved conflict markers (line {})",
                    lines[0]
                ));
            }
        }
    }
    Ok(drifted)
}

/// Section drift of one channel: a locked block that was edited, deleted, lost its host or has
/// broken markers is reported as `HOST#CHANNEL`, `update`, `owned` (the same shape as owned
/// drift; broken markers also get a message that says how to fix them).
fn check_sections(repo: &Path, cl: &ChannelLock, report: &mut Report) -> Result<usize> {
    let name = cl.meta.name.as_str();
    let mut drifted = 0;
    for (host, locked) in &cl.sections {
        check_target(repo, host).map_err(|e| {
            SpliceError::Template(format!("{} lists an unsafe path: {e}", cl.path()))
        })?;
        let path = format!("{host}#{name}");
        let drift = match read_opt(repo, host)? {
            None => true,
            Some(bytes) => match String::from_utf8(bytes) {
                Err(_) => {
                    report.msg(format!(
                        "channel {name}: {host}: not valid UTF-8; fix the file by hand"
                    ));
                    true
                }
                Ok(text) => match find_block(&text, name) {
                    Err(e) => {
                        report.msg(format!("channel {name}: {}", e.message(host, name)));
                        true
                    }
                    Ok(None) => true,
                    Ok(Some(b)) => body_sha(&b.body) != locked.sha256,
                },
            },
        };
        if drift {
            report.push(path, Action::Update, Class::Owned);
            drifted += 1;
        }
    }
    Ok(drifted)
}

/// `Some("A → B (tag)")` when the channel's source has something newer whose template subtree
/// differs from the locked commit; a tracked channel compares branch heads (short shas).
fn channel_update(cl: &ChannelLock) -> Result<Option<String>> {
    let lock = &cl.lock;
    let src = TemplateSource::parse(&lock.template.source)?;
    let fetcher = Fetcher::from_env();
    let (target, label) = match &cl.meta.track {
        Some(branch) => {
            let head = fetcher.branch_head(&src, branch)?;
            (head, branch.clone())
        }
        None => {
            let Some(v) = fetcher
                .available_versions(&src, false)?
                .into_iter()
                .max()
                .filter(|v| *v > lock.template.version)
            else {
                return Ok(None);
            };
            (fetcher.resolve_version(&src, &v)?, "tag".to_string())
        }
    };
    if target.commit == lock.template.commit {
        return Ok(None);
    }
    let tree = |c: &str| fetcher.subtree_id(&src, c).ok();
    if tree(&target.commit).is_some_and(|t| Some(t) == tree(&lock.template.commit)) {
        return Ok(None);
    }
    let (from, to) = if cl.meta.track.is_some() {
        (
            lock.template.commit[..7.min(lock.template.commit.len())].to_string(),
            target.commit[..7].to_string(),
        )
    } else {
        (
            lock.template.version.to_string(),
            target.version.to_string(),
        )
    };
    Ok(Some(format!("{from} → {to} ({label})")))
}

/// Writes nothing: the locks are only read, and the only cache touched is the fetcher's own.
/// Without `--channel`: the main lock (if any) and every channel; with it, that channel only.
pub fn run(g: &GlobalArgs, a: &CheckArgs) -> Result<Report> {
    let repo = g.dir.as_path();
    let sources = Sources::load(repo)?;
    let channels: Vec<&ChannelLock> = match &a.channel {
        Some(name) => vec![sources.channel(name)?],
        None => sources.channels.iter().collect(),
    };
    let main = if a.channel.is_some() {
        None
    } else {
        sources.main.as_ref()
    };
    if main.is_none() && channels.is_empty() && a.channel.is_none() {
        return Err(SpliceError::Usage(format!(
            "{} has no {LOCK_PATH}; run `splice new` first",
            repo.display()
        )));
    }
    let mut report = Report::new("check");
    let mut update_available = false;

    if let Some(lock) = main {
        check_files(repo, lock, "", &mut report)?;
        if !a.offline {
            match newer_stable(lock) {
                Ok(Some(v)) => {
                    update_available = true;
                    report.msg(format!("update available: {} → {v}", lock.template.version));
                }
                Ok(None) => {}
                Err(e) => report.msg(format!("could not look for updates: {e}")),
            }
        }
    }

    for cl in channels {
        let name = cl.meta.name.as_str();
        let prefix = format!("channel {name}: ");
        let mut drifted = check_files(repo, &cl.lock, &prefix, &mut report)?;
        drifted += check_sections(repo, cl, &mut report)?;
        if drifted > 0 {
            report.msg(format!(
                "{prefix}{drifted} managed path(s) differ from {}",
                cl.path()
            ));
        }
        if !a.offline {
            match channel_update(cl) {
                Ok(Some(what)) => {
                    update_available = true;
                    report.msg(format!("{prefix}update available {what}"));
                }
                Ok(None) => {}
                Err(e) => report.msg(format!("{prefix}could not look for updates: {e}")),
            }
        }
    }

    if !report.changes.is_empty() || update_available {
        report.ok = false;
        return Err(SpliceError::Drift(report));
    }
    Ok(report)
}
