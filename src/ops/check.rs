//! `splice check` (spec §8.7): read-only drift, conflict-marker and update-available gate.

use crate::cli::{CheckArgs, GlobalArgs};
use crate::error::SpliceError;
use crate::fetch::Fetcher;
use crate::fsio::{read_opt, sha256_hex};
use crate::lock::{LOCK_PATH, Lock};
use crate::markers::find_markers;
use crate::output::{Action, Class, Report};
use crate::source::TemplateSource;

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

/// Writes nothing: the lock is only read, and the only cache touched is the fetcher's own.
pub fn run(g: &GlobalArgs, a: &CheckArgs) -> Result<Report> {
    let repo = g.dir.as_path();
    let Some(lock) = Lock::read(repo)? else {
        return Err(SpliceError::Usage(format!(
            "{} has no {LOCK_PATH}; run `splice new` first",
            repo.display()
        )));
    };
    let mut report = Report::new("check");

    for (path, file) in &lock.files {
        let bytes = read_opt(repo, path)?;
        if file.class == Class::Owned
            && bytes.as_deref().map(sha256_hex).as_deref() != Some(file.sha256.as_str())
        {
            report.push(path, Action::Update, Class::Owned);
        }
        if let Some(bytes) = bytes {
            let lines = find_markers(&String::from_utf8_lossy(&bytes));
            if !lines.is_empty() {
                report.push(path, Action::Conflict, Class::Shared);
                report.msg(format!(
                    "`{path}` has unresolved conflict markers (line {})",
                    lines[0]
                ));
            }
        }
    }

    let mut update_available = false;
    if !a.offline {
        match newer_stable(&lock) {
            Ok(Some(v)) => {
                update_available = true;
                report.msg(format!("update available: {} → {v}", lock.template.version));
            }
            Ok(None) => {}
            Err(e) => report.msg(format!("could not look for updates: {e}")),
        }
    }

    if !report.changes.is_empty() || update_available {
        report.ok = false;
        return Err(SpliceError::Drift(report));
    }
    Ok(report)
}
