//! `[[post]]` commands (spec §8): run in the repo root, output on stderr.

use crate::error::SpliceError;
use crate::manifest::{PostCmd, When};
use std::path::Path;
use std::process::{Command, Stdio};

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
        let mut cmd = if cfg!(windows) {
            let mut c2 = Command::new("cmd");
            c2.args(["/C", run]);
            c2
        } else {
            let mut c2 = Command::new("sh");
            c2.args(["-c", run]);
            c2
        };
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
                "post command `{}` failed ({out}); the files and the lock were already written",
                run
            )));
        }
    }
    Ok(())
}
