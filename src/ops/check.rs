//! `splice check` (spec §8.7): owned drift, leftover conflict markers, an available update.
//! Reads only the lock and the disk (plus the tag list unless `--offline`); writes nothing.

use super::{check_readable, no_lock};
use crate::cli::{CheckArgs, GlobalArgs};
use crate::error::SpliceError;
use crate::fetch::Fetcher;
use crate::fsio::{read_opt, sha256_hex};
use crate::lock::Lock;
use crate::markers::find_markers;
use crate::output::{Action, Class, Report};
use crate::source::TemplateSource;
use std::path::Path;

type Result<T> = std::result::Result<T, SpliceError>;

pub fn run(g: &GlobalArgs, a: &CheckArgs) -> Result<Report> {
    let repo = g.dir.as_path();
    let (lock, warnings) = Lock::read_with_warnings(repo)?.ok_or_else(|| no_lock(repo))?;
    let mut report = Report::new("check");
    report.messages.extend(warnings);

    // BTreeMap order: changes come out sorted by path
    for (path, f) in &lock.files {
        check_readable(repo, path)?;
        let Some(bytes) = read_opt(repo, path)? else {
            // a deleted owned file is drift; a deleted shared file is the user's choice
            if f.class == Class::Owned {
                report.push(path, Action::Update, Class::Owned);
            }
            continue;
        };
        if f.class == Class::Owned && sha256_hex(&bytes) != f.sha256 {
            report.push(path, Action::Update, Class::Owned);
        } else if let Ok(text) = std::str::from_utf8(&bytes)
            && !find_markers(text).is_empty()
        {
            report.push(path, Action::Conflict, f.class);
        }
    }

    let mut update = false;
    if !a.offline {
        match latest_newer(&lock, repo) {
            Ok(Some(v)) => {
                update = true;
                report.msg(format!(
                    "update available: {} \u{2192} {v}",
                    lock.template.version
                ));
            }
            Ok(None) => {}
            Err(e) => report.msg(format!("could not look for updates: {e}")),
        }
    }

    if report.changes.is_empty() && !update {
        return Ok(report);
    }
    Err(SpliceError::Drift(report))
}

/// The newest stable tag when it is newer than the lock's version.
fn latest_newer(lock: &Lock, repo: &Path) -> Result<Option<semver::Version>> {
    let source = TemplateSource::parse_from(&lock.template.source, repo)?;
    let versions = Fetcher::from_env().available_versions(&source, false)?;
    Ok(versions
        .into_iter()
        .max()
        .filter(|v| *v > lock.template.version))
}
