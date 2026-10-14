//! Execute a `Plan` in the fixed order of spec §8.3: files, removals, migration steps, lock
//! (atomic, last), post commands.

use crate::error::SpliceError;
use crate::fsio::write_file;
use crate::lock::Lock;
use crate::manifest::{PostCmd, When};
use crate::migrate::Step;
use crate::output::{Action, Report};
use crate::planner::Plan;
use crate::post::{RESTORE, post_runs, run_post, run_steps};
use semver::Version;
use std::path::Path;

type Result<T> = std::result::Result<T, SpliceError>;

pub struct ApplyCtx<'a> {
    pub repo: &'a Path,
    pub plan: &'a Plan,
    pub steps: Vec<(&'a Version, &'a Step)>,
    pub from: Version,
    pub to: Version,
    /// The lock to write: the target state.
    pub lock: Lock,
    pub post: Vec<PostCmd>,
    pub when: When,
    pub no_post: bool,
    pub json: bool,
}

/// A failure after something was written: keep the report, name the restore command.
fn partial(e: SpliceError, report: &Report, restore: bool) -> SpliceError {
    let message = if restore {
        format!("{e}; `{RESTORE}` restores the previous state")
    } else {
        e.to_string()
    };
    match e {
        SpliceError::Conflict(_) | SpliceError::Drift(_) => e,
        _ => SpliceError::Partial {
            message,
            report: Box::new(report.clone()),
        },
    }
}

pub fn apply(ctx: ApplyCtx) -> Result<Report> {
    let repo = ctx.repo;
    // Every target is checked before anything is written (spec §10 item 19).
    for c in &ctx.plan.changes {
        if c.bytes.is_some() || c.action == Action::Delete {
            crate::ops::check_target(repo, &c.path)?;
        }
    }

    let mut report = Report::new("update");
    let mut wrote = false;
    // Files first, then removals.
    for deleting in [false, true] {
        for c in ctx
            .plan
            .changes
            .iter()
            .filter(|c| (c.action == Action::Delete) == deleting)
        {
            let res = match (&c.bytes, c.action) {
                (Some(b), _) => write_file(repo, &c.path, b),
                (None, Action::Delete) => match std::fs::remove_file(repo.join(&c.path)) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    r => r,
                },
                _ => Ok(()),
            };
            if let Err(e) = res {
                let e = SpliceError::Io(std::io::Error::new(e.kind(), format!("{}: {e}", c.path)));
                return Err(partial(e, &report, wrote));
            }
            if c.bytes.is_some() || c.action == Action::Delete {
                wrote = true;
            }
            if let Some(n) = &c.note {
                report.msg(format!("{}: {n}", c.path));
            }
            report.push(c.path.clone(), c.action, c.class);
        }
    }

    run_steps(repo, &ctx.steps, &ctx.from, &ctx.to, ctx.json)
        .map_err(|e| partial(e, &report, false))?;

    // The lock is the last write; a failure leaves the old lock in place.
    ctx.lock
        .write_atomic(repo)
        .map_err(|e| partial(e, &report, true))?;

    if !ctx.plan.conflicts.is_empty() {
        let cmds = post_runs(&ctx.post, ctx.when);
        if !ctx.no_post && !cmds.is_empty() {
            let list = cmds
                .iter()
                .map(|c| format!("`{c}`"))
                .collect::<Vec<_>>()
                .join(", ");
            report.msg(format!(
                "post commands skipped because of conflicts; run them by hand after resolving: {list}"
            ));
        }
        return Err(SpliceError::Conflict(report));
    }
    if !ctx.no_post {
        run_post(repo, &ctx.post, ctx.when, ctx.json).map_err(|e| partial(e, &report, false))?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::{LockFile, LockTemplate};
    use crate::output::Class;
    use crate::planner::Planned;
    use std::collections::BTreeMap;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn lock(version: &str) -> Lock {
        Lock {
            format: 1,
            splice: v("0.1.0"),
            template: LockTemplate {
                source: "gh:o/r".into(),
                version: v(version),
                commit: "a".repeat(40),
            },
            vars: Default::default(),
            pieces: vec![],
            files: BTreeMap::from([(
                "new.txt".to_string(),
                LockFile {
                    class: Class::Owned,
                    sha256: "0".repeat(64),
                },
            )]),
        }
    }

    fn write(a: Action, path: &str, bytes: Option<&str>) -> Planned {
        Planned {
            path: path.into(),
            action: a,
            class: Class::Owned,
            bytes: bytes.map(|b| b.as_bytes().to_vec()),
            note: None,
        }
    }

    fn repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        lock("1.0.0").write_atomic(d.path()).unwrap();
        std::fs::write(d.path().join("old.txt"), "old").unwrap();
        d
    }

    #[cfg(unix)]
    fn pc(run: &str) -> PostCmd {
        PostCmd {
            run: run.into(),
            when: vec![When::Update],
        }
    }

    #[cfg(unix)]
    fn step(run: &str) -> Step {
        Step {
            run: run.into(),
            description: None,
        }
    }

    fn read(d: &tempfile::TempDir, rel: &str) -> String {
        std::fs::read_to_string(d.path().join(rel)).unwrap_or_default()
    }

    fn lock_version(d: &tempfile::TempDir) -> Version {
        Lock::read(d.path()).unwrap().unwrap().template.version
    }

    fn plan(changes: Vec<Planned>, conflicts: &[&str]) -> Plan {
        Plan {
            changes,
            drift: vec![],
            conflicts: conflicts.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn ctx<'a>(
        d: &'a tempfile::TempDir,
        plan: &'a Plan,
        steps: Vec<(&'a Version, &'a Step)>,
        post: Vec<PostCmd>,
    ) -> ApplyCtx<'a> {
        ApplyCtx {
            repo: d.path(),
            plan,
            steps,
            from: v("1.0.0"),
            to: v("2.0.0"),
            lock: lock("2.0.0"),
            post,
            when: When::Update,
            no_post: false,
            json: false,
        }
    }

    fn basic_plan() -> Plan {
        plan(
            vec![
                write(Action::Delete, "old.txt", None),
                write(Action::Create, "new.txt", Some("n")),
            ],
            &[],
        )
    }

    #[test]
    #[cfg(unix)]
    fn order_is_files_removals_steps_lock_post() {
        let d = repo();
        let p = basic_plan();
        let ver = v("2.0.0");
        let s = step(
            r#"test -f new.txt && test ! -f old.txt && grep -q 'version = "1.0.0"' .splice/lock.toml && echo "$SPLICE_FROM_VERSION $SPLICE_TO_VERSION" >> order.log"#,
        );
        let post = pc(r#"grep -q 'version = "2.0.0"' .splice/lock.toml && echo post >> order.log"#);
        let r = apply(ctx(&d, &p, vec![(&ver, &s)], vec![post])).unwrap();
        assert_eq!(read(&d, "order.log"), "1.0.0 2.0.0\npost\n");
        assert_eq!(r.changes.len(), 2);
    }

    #[test]
    #[cfg(unix)]
    fn failing_step_leaves_old_lock() {
        let d = repo();
        let p = basic_plan();
        let ver = v("2.0.0");
        let s = step("exit 7");
        let e = apply(ctx(&d, &p, vec![(&ver, &s)], vec![])).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("git checkout -- . && git clean -fd"));
        assert!(e.to_string().contains("exit 7") || e.to_string().contains("`exit 7`"));
        assert_eq!(lock_version(&d), v("1.0.0"));
        let SpliceError::Partial { report, .. } = e else {
            panic!("expected Partial")
        };
        assert_eq!(report.changes.len(), 2);
    }

    #[test]
    #[cfg(unix)]
    fn failing_post_keeps_new_lock() {
        let d = repo();
        let p = basic_plan();
        let e = apply(ctx(&d, &p, vec![], vec![pc("exit 1")])).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert_eq!(lock_version(&d), v("2.0.0"));
        let SpliceError::Partial { report, .. } = e else {
            panic!("expected Partial")
        };
        assert_eq!(report.changes.len(), 2);
    }

    #[test]
    #[cfg(unix)]
    fn conflicts_still_write_lock_and_exit_1() {
        let d = repo();
        let p = plan(
            vec![Planned {
                action: Action::Conflict,
                ..write(Action::Conflict, "new.txt", Some("<<<<<<< ours\n"))
            }],
            &["new.txt"],
        );
        let e = apply(ctx(&d, &p, vec![], vec![pc("echo post >> post.log")])).unwrap_err();
        assert_eq!(e.exit_code(), 1);
        let SpliceError::Conflict(r) = e else {
            panic!("expected Conflict")
        };
        assert_eq!(r.changes.len(), 1);
        assert_eq!(lock_version(&d), v("2.0.0"));
        assert_eq!(read(&d, "new.txt"), "<<<<<<< ours\n");
        assert_eq!(read(&d, "post.log"), "", "post is skipped on conflict");
        assert!(
            r.messages
                .iter()
                .any(|m| m.contains("post commands skipped"))
        );
    }

    #[test]
    fn unsafe_target_writes_nothing() {
        let d = repo();
        std::fs::create_dir(d.path().join("dir")).unwrap();
        let p = plan(
            vec![
                write(Action::Create, "new.txt", Some("n")),
                write(Action::Update, "dir", Some("x")),
            ],
            &[],
        );
        let e = apply(ctx(&d, &p, vec![], vec![])).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("unsafe path"));
        assert!(!d.path().join("new.txt").exists());
        assert_eq!(lock_version(&d), v("1.0.0"));
    }

    #[test]
    fn lock_failure_after_writes_keeps_changes_and_names_restore() {
        let d = repo();
        let p = basic_plan();
        // A broken lock path: `.splice` replaced by a plain file after the checks pass.
        let mut c = ctx(&d, &p, vec![], vec![]);
        let steps_dir = d.path().join(".splice");
        std::fs::remove_dir_all(&steps_dir).unwrap();
        std::fs::write(&steps_dir, "x").unwrap();
        c.json = true;
        let e = apply(c).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("git checkout -- . && git clean -fd"));
        let SpliceError::Partial { report, .. } = e else {
            panic!("expected Partial")
        };
        assert_eq!(report.changes.len(), 2);
    }

    #[test]
    fn deleting_a_directory_is_refused_before_any_write() {
        let d = repo();
        std::fs::create_dir(d.path().join("dir")).unwrap();
        let p = plan(
            vec![
                write(Action::Create, "new.txt", Some("n")),
                write(Action::Delete, "dir", None),
            ],
            &[],
        );
        let e = apply(ctx(&d, &p, vec![], vec![])).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("unsafe path"));
        assert!(!d.path().join("new.txt").exists());
        assert_eq!(lock_version(&d), v("1.0.0"));
    }
}
