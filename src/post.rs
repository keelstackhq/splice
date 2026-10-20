//! `[[post]]` commands (spec §8.1).

use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

use semver::Version;

use crate::error::SpliceError;
use crate::manifest::{PostCmd, When};
use crate::migrate::Step;

/// Run `run` through the shell with `repo` as working directory and `envs` added. Our stdout is
/// for the report only: the child's stdout joins its stderr on ours, and both stream as they are
/// produced. With `json` its stdin is closed.
fn run_shell(
    repo: &Path,
    run: &str,
    json: bool,
    envs: &[(&str, String)],
) -> std::io::Result<ExitStatus> {
    let mut shell = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", run]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", run]);
        c
    };
    shell.current_dir(repo);
    shell.envs(envs.iter().map(|(k, v)| (*k, v.as_str())));
    shell.stdout(stderr_stdio()?).stderr(Stdio::inherit());
    if json {
        shell.stdin(Stdio::null());
    }
    shell.status()
}

fn how(status: ExitStatus) -> String {
    match status.code() {
        Some(c) => format!("exit {c}"),
        None => "killed by a signal".to_string(),
    }
}

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
        let status = run_shell(repo, &cmd.run, json, &[]).map_err(|e| spawn_error(&cmd.run, &e))?;
        if !status.success() {
            return Err(SpliceError::Env(format!(
                "post command `{}` failed ({})",
                cmd.run,
                how(status)
            )));
        }
    }
    Ok(())
}

/// Run the migration steps in order (each paired with its migration's version), with
/// `SPLICE_FROM_VERSION` and `SPLICE_TO_VERSION` set. Output goes to our stderr like `run_post`.
/// A failure is an environment error (exit 3) naming the step, its version, `still` (which lock
/// still records what, e.g. `the lock still records 1.0.0`) and the command that restores the
/// previous state; the lock has not been written yet.
pub fn run_steps(
    repo: &Path,
    steps: &[(&Version, &Step)],
    from: &Version,
    to: &Version,
    still: &str,
    json: bool,
) -> Result<(), SpliceError> {
    let envs = [
        ("SPLICE_FROM_VERSION", from.to_string()),
        ("SPLICE_TO_VERSION", to.to_string()),
    ];
    for (version, step) in steps {
        if let Some(d) = &step.description {
            eprintln!("migration {version}: {d}");
        }
        let restore =
            format!("{still}; `git checkout -- . && git clean -fd` restores the previous state");
        let status = run_shell(repo, &step.run, json, &envs).map_err(|e| {
            SpliceError::Env(format!(
                "cannot run migration step `{}` (migration {version}): {e}; {restore}",
                step.run
            ))
        })?;
        if !status.success() {
            return Err(SpliceError::Env(format!(
                "migration step `{}` (migration {version}) failed ({}); {restore}",
                step.run,
                how(status)
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn cmd(run: &str, when: &[When]) -> PostCmd {
        PostCmd {
            run: run.into(),
            when: when.to_vec(),
        }
    }

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
