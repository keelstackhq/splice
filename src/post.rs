//! `[[post]]` commands (spec §8): run in the repo root, output on stderr.

use crate::error::SpliceError;
use crate::manifest::{PostCmd, When};
use crate::migrate::Step;
use semver::Version;
use std::path::Path;
use std::process::{Command, Stdio};

/// The git part of the restore command (spec §8.3 step 1).
pub const RESTORE: &str = "git checkout -- . && git clean -fd";

/// What git can do to undo a partial update (spec §8.3 step 1, §10 item 25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Undo {
    /// The tree was verified clean before the update: the restore command is safe.
    Git,
    /// The tree had uncommitted changes (`--allow-dirty`) or could not be checked.
    Unverified,
    /// Not inside a git work tree.
    NoGit,
}

impl Undo {
    /// The advice that ends an error message after files were written.
    pub fn advice(self, repo: &Path) -> String {
        match self {
            Undo::Git => format!("`{}` restores the previous state", restore_cmd(repo)),
            Undo::Unverified => "the tree had uncommitted changes before the update, so git \
                cannot separate them from the update's changes; inspect `git status` and \
                `git diff` and undo by hand"
                .to_string(),
            Undo::NoGit => "this is not a git work tree, so there is no git undo; inspect the \
                files by hand"
                .to_string(),
        }
    }
}

/// What to run to undo a partial update: `cd <dir> && git checkout -- . && git clean -fd`. The
/// `cd` matters in a monorepo: run inside the project directory, git only touches that subtree.
pub fn restore_cmd(repo: &Path) -> String {
    let abs = std::path::absolute(repo).unwrap_or_else(|_| repo.to_path_buf());
    let dir = abs.display().to_string();
    let plain = dir
        .chars()
        .all(|c| c.is_alphanumeric() || "/\\._-:+@,=~".contains(c));
    if cfg!(windows) {
        // `/d` also switches drive; quote when anything beyond plain path characters appears.
        let dir = if plain { dir } else { format!("\"{dir}\"") };
        format!("cd /d {dir} && {RESTORE}")
    } else {
        let dir = if plain && !dir.contains('%') {
            dir
        } else {
            format!("'{}'", dir.replace('\'', "'\\''"))
        };
        format!("cd {dir} && {RESTORE}")
    }
}

/// Where Git for Windows keeps its `sh.exe`, derived from `git --exec-path` (spec §10 item 31).
/// Walks up from the exec path (`<prefix>\mingw64\libexec\git-core`, also `mingw32` and
/// `clangarm64`) to the first directory with `bin\sh.exe`, else `usr\bin\sh.exe`. Pure: `exists`
/// answers whether a candidate file is there. Accepts `\` and `/`.
pub fn sh_from_exec_path(exec_path: &str, exists: impl Fn(&str) -> bool) -> Option<String> {
    let sep = if exec_path.contains('\\') { '\\' } else { '/' };
    let parts: Vec<&str> = exec_path
        .trim()
        .split(['\\', '/'])
        .filter(|p| !p.is_empty())
        .collect();
    // Leading separator (`/c/Git/...`, `\\server\...`) is kept only for the first piece.
    let lead = if exec_path.trim().starts_with(['\\', '/']) {
        sep.to_string()
    } else {
        String::new()
    };
    for n in (1..parts.len()).rev() {
        let prefix = format!("{lead}{}", parts[..n].join(&sep.to_string()));
        for tail in [format!("bin{sep}sh.exe"), format!("usr{sep}bin{sep}sh.exe")] {
            let cand = format!("{prefix}{sep}{tail}");
            if exists(&cand) {
                return Some(cand);
            }
        }
    }
    None
}

#[cfg(windows)]
fn find_sh() -> Result<std::path::PathBuf, SpliceError> {
    let exists = |p: &str| Path::new(p).is_file();
    let on_path = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .flat_map(|d| [d.join("sh.exe"), d.join("sh")])
            .find(|c| c.is_file())
    });
    if let Some(p) = on_path {
        return Ok(p);
    }
    let derived = Command::new("git")
        .args(["--exec-path"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| sh_from_exec_path(String::from_utf8_lossy(&o.stdout).trim(), exists));
    derived.map(std::path::PathBuf::from).ok_or_else(|| {
        SpliceError::Env(
            "template commands need a POSIX sh; install Git for Windows or put sh on PATH".into(),
        )
    })
}

#[cfg(windows)]
fn shell(run: &str) -> Result<Command, SpliceError> {
    let mut c = Command::new(find_sh()?);
    c.args(["-c", run]);
    Ok(c)
}

#[cfg(not(windows))]
fn shell(run: &str) -> Result<Command, SpliceError> {
    let mut c = Command::new("sh");
    c.args(["-c", run]);
    Ok(c)
}

/// The `run` strings that would execute for `when`, in order, without repeats.
pub fn post_runs(cmds: &[PostCmd], when: When) -> Vec<&str> {
    let mut seen: Vec<&str> = Vec::new();
    for c in cmds.iter().filter(|c| c.when.contains(&when)) {
        if !seen.contains(&c.run.as_str()) {
            seen.push(&c.run);
        }
    }
    seen
}

/// Run every command whose `when` contains `when`, in order, skipping repeated `run` strings.
/// The child's stdout and stderr are both forwarded to our stderr, so with `--json` stdout stays
/// exactly one object (spec §3).
pub fn run_post(repo: &Path, cmds: &[PostCmd], when: When) -> Result<(), SpliceError> {
    for run in post_runs(cmds, when) {
        let mut cmd = shell(run)?;
        let out = cmd
            .current_dir(repo)
            .stdin(Stdio::null())
            .stdout(Stdio::from(std::io::stderr()))
            .stderr(Stdio::from(std::io::stderr()))
            .status()
            .map_err(|e| {
                SpliceError::Env(format!("post command `{}` could not start: {e}", run))
            })?;
        if !out.success() {
            return Err(SpliceError::Env(format!(
                "post command `{}` failed ({out}); the files and the lock were already updated; fix the command and rerun it by hand",
                run
            )));
        }
    }
    Ok(())
}

/// Run migration steps in order, in the repo root, output on stderr, with
/// `SPLICE_FROM_VERSION` and `SPLICE_TO_VERSION` set (spec §8.3 step 7). A failure is an Env
/// error naming the step, its version and how to restore the previous state; the lock has not
/// been written yet.
pub fn run_steps(
    repo: &Path,
    steps: &[(&Version, &Step)],
    from: &Version,
    to: &Version,
    undo: Undo,
) -> Result<(), SpliceError> {
    let advice = undo.advice(repo);
    for (version, step) in steps {
        let status = shell(&step.run)?
            .current_dir(repo)
            .env("SPLICE_FROM_VERSION", from.to_string())
            .env("SPLICE_TO_VERSION", to.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::from(std::io::stderr()))
            .stderr(Stdio::from(std::io::stderr()))
            .status()
            .map_err(|e| {
                SpliceError::Env(format!(
                    "migration step `{}` ({version}) could not start: {e}; the lock still records {from}; {advice}",
                    step.run
                ))
            })?;
        if !status.success() {
            return Err(SpliceError::Env(format!(
                "migration step `{}` ({version}) failed ({status}); the lock still records {from}; {advice}",
                step.run
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_names_the_directory() {
        let c = restore_cmd(Path::new("/work/proj"));
        assert!(c.ends_with(" && git checkout -- . && git clean -fd"));
        assert!(c.starts_with("cd "));
        assert!(c.contains("proj"));
    }

    fn has<'a>(files: &'a [&'a str]) -> impl Fn(&str) -> bool + 'a {
        move |p| files.contains(&p)
    }

    #[test]
    fn sh_found_next_to_git_for_windows() {
        let ep = r"C:\Program Files\Git\mingw64\libexec\git-core";
        let f = [r"C:\Program Files\Git\bin\sh.exe"];
        assert_eq!(sh_from_exec_path(ep, has(&f)).as_deref(), Some(f[0]));
    }

    #[test]
    fn sh_falls_back_to_usr_bin_and_forward_slashes() {
        let ep = "C:/Program Files/Git/mingw64/libexec/git-core";
        let f = ["C:/Program Files/Git/usr/bin/sh.exe"];
        assert_eq!(sh_from_exec_path(ep, has(&f)).as_deref(), Some(f[0]));
    }

    #[test]
    fn sh_prefers_bin_and_handles_variants() {
        for v in ["mingw32", "clangarm64"] {
            let ep = format!(r"D:\G\{v}\libexec\git-core");
            let f = [r"D:\G\usr\bin\sh.exe", r"D:\G\bin\sh.exe"];
            assert_eq!(sh_from_exec_path(&ep, has(&f)).as_deref(), Some(f[1]));
        }
    }

    #[test]
    fn sh_none_when_absent() {
        assert_eq!(
            sh_from_exec_path(r"C:\Git\mingw64\libexec\git-core", |_| false),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn restore_quotes_odd_paths() {
        assert_eq!(
            restore_cmd(Path::new("/a b/it's")),
            format!("cd '/a b/it'\\''s' && {RESTORE}")
        );
    }
}
