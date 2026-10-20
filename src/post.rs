//! `[[post]]` commands and migration steps (spec §8.1). Template commands are POSIX `sh -c` on
//! every OS; there is no `cmd /C`.

use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

use semver::Version;

use crate::error::SpliceError;
use crate::manifest::{PostCmd, When};
use crate::migrate::Step;

/// Where Git for Windows keeps its `sh.exe`, derived from `git --exec-path` (spec §10 item 19).
/// Walks up from the exec path (`<prefix>\mingw64\libexec\git-core`, also `mingw32` and
/// `clangarm64`) to the first directory with `bin\sh.exe`, else `usr\bin\sh.exe`. Pure: `exists`
/// answers whether a candidate file is there. Accepts `\` and `/`.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn sh_from_exec_path(exec_path: &str, exists: impl Fn(&str) -> bool) -> Option<String> {
    let sep = if exec_path.contains('\\') { '\\' } else { '/' };
    let trimmed = exec_path.trim();
    let parts: Vec<&str> = trimmed
        .split(['\\', '/'])
        .filter(|p| !p.is_empty())
        .collect();
    // A leading separator (`/c/Git/...`, `\\server\...`) belongs to the first piece.
    let lead = if trimmed.starts_with(['\\', '/']) {
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

/// The POSIX `sh` on Windows: first on `PATH`, else Git for Windows' own, found from
/// `git --exec-path`. Missing is an environment error (exit 3).
#[cfg(windows)]
fn find_sh() -> Result<std::path::PathBuf, SpliceError> {
    let on_path = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join("sh.exe"))
            .find(|c| c.is_file())
    });
    if let Some(p) = on_path {
        return Ok(p);
    }
    let derived = Command::new("git")
        .arg("--exec-path")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            sh_from_exec_path(String::from_utf8_lossy(&o.stdout).trim(), |p| {
                Path::new(p).is_file()
            })
        });
    derived.map(std::path::PathBuf::from).ok_or_else(|| {
        SpliceError::Env(
            "template commands need a POSIX sh (`sh`), and none was found on PATH or next to git; \
             install Git for Windows or put sh on PATH"
                .into(),
        )
    })
}

#[cfg(windows)]
fn sh_command() -> Result<Command, SpliceError> {
    Ok(Command::new(find_sh()?))
}

#[cfg(not(windows))]
fn sh_command() -> Result<Command, SpliceError> {
    Ok(Command::new("sh"))
}

/// Run `run` through the shell with `repo` as working directory and `envs` added. Our stdout is
/// for the report only: the child's stdout joins its stderr on ours, and both stream as they are
/// produced. With `json` its stdin is closed.
fn run_shell(
    repo: &Path,
    run: &str,
    json: bool,
    envs: &[(&str, String)],
) -> Result<std::io::Result<ExitStatus>, SpliceError> {
    let mut shell = sh_command()?;
    shell.args(["-c", run]);
    shell.current_dir(repo);
    shell.envs(envs.iter().map(|(k, v)| (*k, v.as_str())));
    let out = match stderr_stdio() {
        Ok(o) => o,
        Err(e) => return Ok(Err(e)),
    };
    shell.stdout(out).stderr(Stdio::inherit());
    if json {
        shell.stdin(Stdio::null());
    }
    Ok(shell.status())
}

fn how(status: ExitStatus) -> String {
    match status.code() {
        Some(c) => format!("exit {c}"),
        None => "killed by a signal".to_string(),
    }
}

/// Run every command whose `when` contains `when`, in order, skipping repeats of an identical
/// `run` string. The shell is POSIX `sh -c` on every OS (on Windows Git for Windows' `sh.exe`, spec §10 item 19) with `repo` as working directory.
/// The command's stdout and stderr both go to our stderr (our stdout is the report only); with
/// `json` its stdin is closed. A non-zero exit is an environment error (exit 3) naming the command.
pub fn run_post(repo: &Path, cmds: &[PostCmd], when: When, json: bool) -> Result<(), SpliceError> {
    let mut seen: Vec<&str> = Vec::new();
    for cmd in cmds.iter().filter(|c| c.when.contains(&when)) {
        if seen.contains(&cmd.run.as_str()) {
            continue;
        }
        seen.push(&cmd.run);
        let status =
            run_shell(repo, &cmd.run, json, &[])?.map_err(|e| spawn_error(&cmd.run, &e))?;
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
        let status = run_shell(repo, &step.run, json, &envs)?.map_err(|e| {
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
