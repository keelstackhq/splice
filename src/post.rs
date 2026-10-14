//! `[[post]]` commands (spec §8): run in the repo root, output on stderr.

use crate::error::SpliceError;
use crate::manifest::{PostCmd, When};
use crate::migrate::Step;
use semver::Version;
use std::path::Path;
use std::process::{Command, Stdio};

/// The git part of the restore command (spec §8.3 step 1).
pub const RESTORE: &str = "git checkout -- . && git clean -fd";

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

#[cfg(windows)]
fn shell(run: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut c = Command::new("cmd");
    // raw_arg: no re-quoting, so quotes inside the command reach cmd.exe as written.
    c.arg("/C").raw_arg(run);
    c
}

#[cfg(not(windows))]
fn shell(run: &str) -> Command {
    let mut c = Command::new("sh");
    c.args(["-c", run]);
    c
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
pub fn run_post(repo: &Path, cmds: &[PostCmd], when: When, _json: bool) -> Result<(), SpliceError> {
    for run in post_runs(cmds, when) {
        let mut cmd = shell(run);
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
    _json: bool,
) -> Result<(), SpliceError> {
    let restore = restore_cmd(repo);
    for (version, step) in steps {
        let status = shell(&step.run)
            .current_dir(repo)
            .env("SPLICE_FROM_VERSION", from.to_string())
            .env("SPLICE_TO_VERSION", to.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::from(std::io::stderr()))
            .stderr(Stdio::from(std::io::stderr()))
            .status()
            .map_err(|e| {
                SpliceError::Env(format!(
                    "migration step `{}` ({version}) could not start: {e}; the lock still records {from}; `{restore}` restores the previous state",
                    step.run
                ))
            })?;
        if !status.success() {
            return Err(SpliceError::Env(format!(
                "migration step `{}` ({version}) failed ({status}); the lock still records {from}; `{restore}` restores the previous state",
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

    #[cfg(unix)]
    #[test]
    fn restore_quotes_odd_paths() {
        assert_eq!(
            restore_cmd(Path::new("/a b/it's")),
            format!("cd '/a b/it'\\''s' && {RESTORE}")
        );
    }
}
