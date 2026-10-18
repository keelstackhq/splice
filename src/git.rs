//! The `git` CLI behind one wrapper. No network crates: all fetching goes through git.

use crate::error::SpliceError;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Namespace for git operations; `git_dir` is reserved for callers that bind one repository.
#[derive(Debug, Default, Clone)]
pub struct Git {
    pub git_dir: Option<PathBuf>,
}

fn command() -> Command {
    let mut cmd = Command::new("git");
    cmd.args(["-c", "core.autocrlf=false", "-c", "core.eol=lf"])
        .env("GIT_TERMINAL_PROMPT", "0");
    cmd
}

fn run(mut cmd: Command, what: &str) -> Result<Output, SpliceError> {
    let out = cmd.output().map_err(|e| {
        SpliceError::Env(format!(
            "cannot run git ({e}); install git and put it on PATH"
        ))
    })?;
    if out.status.success() {
        Ok(out)
    } else {
        Err(SpliceError::Env(format!(
            "git {what} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

impl Git {
    /// Tags of a remote as `(tag, sha)`, in git's order. The peeled sha (the commit) is
    /// preferred over an annotated tag object's sha.
    pub fn ls_remote_tags(url: &str) -> Result<Vec<(String, String)>, SpliceError> {
        let mut cmd = command();
        cmd.args(["ls-remote", "--tags", "--", url]);
        let out = run(cmd, "ls-remote")?;
        let mut tags: Vec<(String, String)> = Vec::new();
        for line in stdout(&out).lines() {
            let Some((sha, name)) = line.split_once('\t') else {
                continue;
            };
            let Some(tag) = name.strip_prefix("refs/tags/") else {
                continue;
            };
            if let Some(base) = tag.strip_suffix("^{}") {
                if let Some(entry) = tags.iter_mut().find(|t| t.0 == base) {
                    entry.1 = sha.to_string();
                } else {
                    tags.push((base.to_string(), sha.to_string()));
                }
            } else if !tags.iter().any(|t| t.0 == tag) {
                tags.push((tag.to_string(), sha.to_string()));
            }
        }
        Ok(tags)
    }

    /// Create a bare mirror of `url` at `mirror`, or bring an existing one up to date.
    pub fn ensure_mirror(url: &str, mirror: &Path) -> Result<(), SpliceError> {
        if mirror.join("HEAD").exists() {
            let mut cmd = command();
            cmd.arg("--git-dir")
                .arg(mirror)
                .args(["fetch", "--quiet", "--tags", "--prune", "origin"]);
            run(cmd, "fetch")?;
        } else {
            if let Some(parent) = mirror.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut cmd = command();
            cmd.args(["clone", "--quiet", "--mirror", "--", url])
                .arg(mirror);
            run(cmd, "clone --mirror")?;
        }
        Ok(())
    }

    /// Resolve `rev` to a full commit sha.
    pub fn rev_parse(git_dir: &Path, rev: &str) -> Result<String, SpliceError> {
        let mut cmd = command();
        cmd.arg("--git-dir")
            .arg(git_dir)
            .args(["rev-parse", "--verify", "--quiet"])
            .arg(format!("{rev}^{{commit}}"));
        let out = cmd.output().map_err(|e| {
            SpliceError::Env(format!(
                "cannot run git ({e}); install git and put it on PATH"
            ))
        })?;
        if !out.status.success() {
            return Err(SpliceError::Env(format!(
                "unknown revision `{rev}` in the template repository"
            )));
        }
        Ok(stdout(&out).trim().to_string())
    }

    /// Write the tree of `commit` to `dest` (no `.git`). Uses a throwaway index and work
    /// tree so the repository is untouched, then renames the work tree into place.
    pub fn extract_tree(git_dir: &Path, commit: &str, dest: &Path) -> Result<(), SpliceError> {
        let parent = dest.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)?;
        let work = tempfile::Builder::new()
            .prefix(".splice-tree-")
            .tempdir_in(parent)?;
        let scratch = tempfile::tempdir()?;
        let mut cmd = command();
        cmd.arg("--git-dir")
            .arg(git_dir)
            .arg("--work-tree")
            .arg(work.path())
            .env("GIT_INDEX_FILE", scratch.path().join("index"))
            .args(["checkout", "--quiet", commit, "--", "."]);
        run(cmd, "checkout")?;
        let tmp = work.keep();
        std::fs::rename(&tmp, dest).inspect_err(|_| {
            let _ = std::fs::remove_dir_all(&tmp);
        })?;
        Ok(())
    }

    /// `None` when `repo` is not inside a git work tree, else whether it has no changes
    /// (untracked files count as changes).
    pub fn is_clean(repo: &Path) -> Result<Option<bool>, SpliceError> {
        let mut cmd = command();
        cmd.arg("-C")
            .arg(repo)
            .args(["rev-parse", "--is-inside-work-tree"]);
        let Ok(out) = cmd.output() else {
            return Err(SpliceError::Env(
                "cannot run git; install git and put it on PATH".into(),
            ));
        };
        if !out.status.success() || stdout(&out).trim() != "true" {
            return Ok(None);
        }
        let mut cmd = command();
        cmd.arg("-C").arg(repo).args(["status", "--porcelain"]);
        let out = run(cmd, "status")?;
        Ok(Some(out.stdout.is_empty()))
    }

    /// `git init` in `dir` (created if missing).
    pub fn init(dir: &Path) -> Result<(), SpliceError> {
        std::fs::create_dir_all(dir)?;
        let mut cmd = command();
        cmd.args(["init", "--quiet"]).arg(dir);
        run(cmd, "init")?;
        Ok(())
    }
}
