use crate::error::SpliceError;
use std::path::{Path, PathBuf};
use std::process::Command;

type Result<T> = std::result::Result<T, SpliceError>;

/// Thin wrapper over the `git` CLI. All calls disable autocrlf/eol conversion and never prompt.
#[derive(Debug, Default, Clone)]
pub struct Git {
    pub git_dir: Option<PathBuf>,
}

fn base(git_dir: Option<&Path>) -> Command {
    let mut c = Command::new("git");
    c.args(["-c", "core.autocrlf=false", "-c", "core.eol=lf"]);
    if let Some(d) = git_dir {
        c.arg("--git-dir").arg(d);
    }
    c.env("GIT_TERMINAL_PROMPT", "0");
    c
}

fn run(mut c: Command) -> Result<String> {
    let out = c
        .output()
        .map_err(|e| SpliceError::Env(format!("could not run git: {e}")))?;
    if !out.status.success() {
        return Err(SpliceError::Env(format!(
            "git failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

impl Git {
    /// Tags as `(name, sha)`, in ls-remote order; a peeled `^{}` sha wins over the tag object sha.
    pub fn ls_remote_tags(url: &str) -> Result<Vec<(String, String)>> {
        let mut c = base(None);
        c.args(["ls-remote", "--tags", "--", url]);
        let out = run(c)?;
        let mut tags: Vec<(String, String)> = Vec::new();
        for line in out.lines() {
            let Some((sha, name)) = line.split_once('\t') else {
                continue;
            };
            let Some(name) = name.strip_prefix("refs/tags/") else {
                continue;
            };
            let (name, peeled) = match name.strip_suffix("^{}") {
                Some(n) => (n, true),
                None => (name, false),
            };
            match tags.iter_mut().find(|t| t.0 == name) {
                Some(t) if peeled => t.1 = sha.to_string(),
                Some(_) => {}
                None => tags.push((name.to_string(), sha.to_string())),
            }
        }
        Ok(tags)
    }

    /// Clone `url` as a bare mirror at `mirror`, or update an existing one.
    pub fn ensure_mirror(url: &str, mirror: &Path) -> Result<()> {
        if mirror.join("HEAD").exists() {
            let mut c = base(Some(mirror));
            c.args([
                "fetch",
                "--tags",
                "--prune",
                "--force",
                "--",
                url,
                "+refs/heads/*:refs/heads/*",
            ]);
            return run(c).map(|_| ());
        }
        if let Some(p) = mirror.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut c = base(None);
        c.args(["clone", "--mirror", "-q", "--", url]).arg(mirror);
        run(c).map(|_| ())
    }

    pub fn rev_parse(git_dir: &Path, rev: &str) -> Result<String> {
        let mut c = base(Some(git_dir));
        if rev.starts_with('-') {
            return Err(SpliceError::Usage(format!("invalid revision `{rev}`")));
        }
        c.args(["rev-parse", "--verify", "--quiet"])
            .arg(format!("{rev}^{{commit}}"));
        let out = c
            .output()
            .map_err(|e| SpliceError::Env(format!("could not run git: {e}")))?;
        if !out.status.success() {
            return Err(SpliceError::Template(format!("revision `{rev}` not found")));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Export the tree of `commit` into `dest` (created atomically via rename).
    pub fn extract_tree(git_dir: &Path, commit: &str, dest: &Path) -> Result<()> {
        let parent = dest.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)?;
        match Self::rev_parse(git_dir, commit) {
            Ok(_) => {}
            Err(SpliceError::Template(_)) => {
                return Err(SpliceError::Template(format!(
                    "template commit {commit} is missing from the repository (force-pushed?)"
                )));
            }
            Err(e) => return Err(e),
        }
        let work = tempfile::tempdir_in(parent)?;
        let idx = tempfile::tempdir()?;
        let mut c = base(Some(git_dir));
        c.env("GIT_INDEX_FILE", idx.path().join("index"))
            .arg("--work-tree")
            .arg(work.path())
            .args(["checkout", commit, "--", "."]);
        run(c)?;
        let work = work.keep();
        if let Err(e) = std::fs::rename(&work, dest) {
            let _ = std::fs::remove_dir_all(&work);
            if dest.exists() {
                return Ok(()); // lost a race: an immutable tree is already in place
            }
            return Err(e.into());
        }
        Ok(())
    }

    /// `Some(true)` when the work tree has no changes, `None` when `repo` is not a work tree.
    pub fn is_clean(repo: &Path) -> Result<Option<bool>> {
        let mut c = base(None);
        c.arg("-C")
            .arg(repo)
            .args(["rev-parse", "--is-inside-work-tree"]);
        match c.output() {
            Ok(o) if o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true" => {}
            Ok(_) => return Ok(None),
            Err(e) => return Err(SpliceError::Env(format!("could not run git: {e}"))),
        }
        let mut c = base(None);
        c.arg("-C").arg(repo).args(["status", "--porcelain"]);
        Ok(Some(run(c)?.trim().is_empty()))
    }

    /// True when `dir` is inside any git work tree (its own or a parent's).
    pub fn is_inside_work_tree(dir: &Path) -> Result<bool> {
        let mut c = base(None);
        c.arg("-C")
            .arg(dir)
            .args(["rev-parse", "--is-inside-work-tree"]);
        match c.output() {
            Ok(o) => Ok(o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true"),
            Err(e) => Err(SpliceError::Env(format!("could not run git: {e}"))),
        }
    }

    pub fn init(dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir)?;
        let mut c = base(None);
        c.arg("-C").arg(dir).args(["init", "-q"]);
        run(c).map(|_| ())
    }
}
