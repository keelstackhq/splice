//! `[[post]]` commands (spec §8.1).

use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::SpliceError;
use crate::manifest::{PostCmd, When};

/// Run every command whose `when` contains `when`, in order, skipping repeats of an identical
/// `run` string. The shell is `sh -c` (`cmd /C` on Windows) with `repo` as working directory.
/// The command's stdout and stderr both go to our stderr (our stdout is the report only); with
/// `json` its stdin is closed. A non-zero exit is an environment error (exit 3) naming the command.
pub fn run_post(repo: &Path, cmds: &[PostCmd], when: When, json: bool) -> Result<(), SpliceError> {
    let mut seen: Vec<&str> = Vec::new();
    for cmd in cmds.iter().filter(|c| c.when.contains(&when)) {
        if seen.contains(&cmd.run.as_str()) {
            continue;
        }
        seen.push(&cmd.run);
        let mut shell = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/C", &cmd.run]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", &cmd.run]);
            c
        };
        shell.current_dir(repo);
        // Our stdout is for the report only: the child's stdout joins its stderr on ours, and
        // both stream as they are produced.
        shell
            .stdout(stderr_stdio().map_err(|e| spawn_error(&cmd.run, &e))?)
            .stderr(Stdio::inherit());
        if json {
            shell.stdin(Stdio::null());
        }
        let status = shell.status().map_err(|e| spawn_error(&cmd.run, &e))?;
        if !status.success() {
            let how = match status.code() {
                Some(c) => format!("exit {c}"),
                None => "killed by a signal".to_string(),
            };
            return Err(SpliceError::Env(format!(
                "post command `{}` failed ({how})",
                cmd.run
            )));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn stderr_stdio() -> std::io::Result<Stdio> {
    use std::os::fd::AsFd;
    Ok(Stdio::from(std::io::stderr().as_fd().try_clone_to_owned()?))
}

#[cfg(windows)]
fn stderr_stdio() -> std::io::Result<Stdio> {
    use std::os::windows::io::AsHandle;
    Ok(Stdio::from(
        std::io::stderr().as_handle().try_clone_to_owned()?,
    ))
}

fn spawn_error(run: &str, e: &std::io::Error) -> SpliceError {
    SpliceError::Env(format!("cannot run post command `{run}`: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(run: &str, when: &[When]) -> PostCmd {
        PostCmd {
            run: run.into(),
            when: when.to_vec(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn filters_by_when_and_dedupes_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let cmds = [
            cmd("echo a >> log", &[When::New]),
            cmd("echo b >> log", &[When::Update]),
            cmd("echo a >> log", &[When::New, When::Add]),
            cmd("echo c >> log", &[When::New]),
        ];
        run_post(dir.path(), &cmds, When::New, true).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("log")).unwrap(),
            "a\nc\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn failure_names_the_command_and_stops() {
        let dir = tempfile::tempdir().unwrap();
        let cmds = [
            cmd("exit 4", &[When::New]),
            cmd("touch after", &[When::New]),
        ];
        let e = run_post(dir.path(), &cmds, When::New, true).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("`exit 4`") && e.to_string().contains("exit 4"));
        assert!(!dir.path().join("after").exists());
    }
}
