//! Apply a [`Plan`] (spec §8.3, §8.6): files, removals, migration steps, `[[post]]`, then the lock.

use std::path::Path;

use semver::Version;

use crate::error::SpliceError;
use crate::fsio;
use crate::lock::{LOCK_PATH, Lock};
use crate::manifest::{PostCmd, When};
use crate::migrate::Step;
use crate::output::{Action, Report};
use crate::planner::Plan;
use crate::post::{run_post, run_steps};

pub struct ApplyCtx<'a> {
    pub repo: &'a Path,
    pub plan: &'a Plan,
    pub steps: Vec<(&'a Version, &'a Step)>,
    pub from: Version,
    pub to: Version,
    /// The lock to write: the state after this update.
    pub lock: Lock,
    pub post: Vec<PostCmd>,
    pub when: When,
    pub no_post: bool,
    pub json: bool,
}

/// Apply the plan in the fixed order: write files, removals, migration steps, `[[post]]`, and the
/// lock (atomic) last. Every target is checked before the first write. A failed step or post
/// exits 3 before the lock is written, so it still records the old state. With conflicts post is
/// skipped, the lock is still written (it records the target) and the result is `Err(Conflict)`.
/// A plan with drift is refused (`Err(Drift)`) before anything is written: the caller decides
/// about `--force` when planning.
pub fn apply(ctx: ApplyCtx) -> Result<Report, SpliceError> {
    let mut report = Report::new("update");
    let plan = ctx.plan;

    if !plan.drift.is_empty() {
        report.ok = false;
        for d in &plan.drift {
            report.msg(format!("`{d}` was edited locally"));
        }
        return Err(SpliceError::Drift(report));
    }

    fsio::check_target(ctx.repo, LOCK_PATH)?;
    for c in &plan.changes {
        fsio::check_target(ctx.repo, &c.path)?;
    }

    // From here on a failure may leave files written: say how to restore (only then).
    let mut wrote = false;
    for c in &plan.changes {
        if let (Some(bytes), true) = (&c.bytes, c.action != Action::Delete) {
            fsio::write_file(ctx.repo, &c.path, bytes)
                .map_err(|e| after_write(e, wrote, &ctx.from))?;
            wrote = true;
        }
    }
    for c in plan.changes.iter().filter(|c| c.action == Action::Delete) {
        match fsio::remove_file(ctx.repo, &c.path) {
            Err(SpliceError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(after_write(e, wrote, &ctx.from)),
            Ok(()) => wrote = true,
        }
    }
    for c in &plan.changes {
        report.push(c.path.clone(), c.action, c.class);
        if let Some(n) = &c.note {
            report.msg(n.clone());
        }
    }

    run_steps(ctx.repo, &ctx.steps, &ctx.from, &ctx.to, ctx.json)?;
    wrote |= !ctx.steps.is_empty();
    if plan.conflicts.is_empty() && !ctx.no_post {
        run_post(ctx.repo, &ctx.post, ctx.when, ctx.json).map_err(|e| match e {
            SpliceError::Env(m) => SpliceError::Env(format!(
                "{m}; the lock still records {}; `git checkout -- . && git clean -fd` restores the previous state",
                ctx.from
            )),
            other => other,
        })?;
        wrote |= ctx.post.iter().any(|p| p.when.contains(&ctx.when));
    }
    ctx.lock
        .write_atomic(ctx.repo)
        .map_err(|e| after_write(e, wrote, &ctx.from))?;

    if plan.conflicts.is_empty() {
        Ok(report)
    } else {
        report.ok = false;
        report.msg(format!(
            "conflicts in {}: resolve the diff3 markers",
            plan.conflicts.join(", ")
        ));
        Err(SpliceError::Conflict(report))
    }
}

/// An error after the write phase began: it says the lock still records `from`, and, when
/// something was written, how to restore the previous state.
fn after_write(e: SpliceError, wrote: bool, from: &Version) -> SpliceError {
    let hint = if wrote {
        "; `git checkout -- . && git clean -fd` restores the previous state"
    } else {
        ""
    };
    SpliceError::Env(format!("{e}; the lock still records {from}{hint}"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::lock::LockTemplate;
    use crate::output::Class;
    use crate::planner::Planned;
    use indexmap::IndexMap;
    use std::collections::BTreeMap;

    fn lock_at(version: &str) -> Lock {
        Lock {
            format: 1,
            splice: Version::new(0, 1, 0),
            template: LockTemplate {
                source: "gh:o/r//t".into(),
                version: Version::parse(version).unwrap(),
                commit: "3f2c9d1e5a7b4c6d8e0f1a2b3c4d5e6f708192a3".into(),
            },
            vars: IndexMap::new(),
            pieces: vec![],
            files: BTreeMap::new(),
        }
    }

    fn read(repo: &Path, rel: &str) -> String {
        std::fs::read_to_string(repo.join(rel)).unwrap()
    }

    fn read_lock(repo: &Path) -> Lock {
        Lock::read(repo).unwrap().unwrap()
    }

    fn planned(path: &str, action: Action, bytes: Option<&str>) -> Planned {
        Planned {
            path: path.into(),
            action,
            class: Class::Owned,
            bytes: bytes.map(|b| b.as_bytes().to_vec()),
            note: None,
        }
    }

    fn plan_of(changes: Vec<Planned>) -> Plan {
        let conflicts = changes
            .iter()
            .filter(|c| c.action == Action::Conflict)
            .map(|c| c.path.clone())
            .collect();
        Plan {
            changes,
            drift: vec![],
            conflicts,
        }
    }

    /// A repo at lock 1.0.0 holding `old.txt`.
    fn repo_v1() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        lock_at("1.0.0").write_atomic(dir.path()).unwrap();
        std::fs::write(dir.path().join("old.txt"), "old\n").unwrap();
        dir
    }

    fn step(run: &str) -> Step {
        Step {
            run: run.into(),
            description: None,
        }
    }

    fn post(run: &str) -> PostCmd {
        PostCmd {
            run: run.into(),
            when: vec![When::Update],
        }
    }

    struct Run<'a> {
        repo: &'a Path,
        plan: &'a Plan,
        steps: &'a [Step],
        post: Vec<PostCmd>,
        no_post: bool,
    }

    fn go(r: Run) -> Result<Report, SpliceError> {
        let v2 = Version::new(2, 0, 0);
        apply(ApplyCtx {
            repo: r.repo,
            plan: r.plan,
            steps: r.steps.iter().map(|s| (&v2, s)).collect(),
            from: Version::new(1, 0, 0),
            to: v2.clone(),
            lock: lock_at("2.0.0"),
            post: r.post,
            when: When::Update,
            no_post: r.no_post,
            json: true,
        })
    }

    #[test]
    fn order_is_files_removals_steps_post_lock() {
        let dir = repo_v1();
        let plan = plan_of(vec![
            planned("new.txt", Action::Create, Some("new\n")),
            planned("old.txt", Action::Delete, None),
        ]);
        let steps = [step(
            "test -f new.txt && test ! -f old.txt && grep -q 'version = \"1.0.0\"' .splice/lock.toml && echo step >> order.log",
        )];
        let report = go(Run {
            repo: dir.path(),
            plan: &plan,
            steps: &steps,
            post: vec![post(
                "grep -q 'version = \"1.0.0\"' .splice/lock.toml && echo post >> order.log",
            )],
            no_post: false,
        })
        .unwrap();
        assert_eq!(read(dir.path(), "order.log"), "step\npost\n");
        assert_eq!(
            read_lock(dir.path()).template.version,
            Version::new(2, 0, 0)
        );
        assert_eq!(read(dir.path(), "new.txt"), "new\n");
        assert!(!dir.path().join("old.txt").exists());
        assert_eq!(report.changes.len(), 2);
        assert!(report.ok);
    }

    #[test]
    fn step_sees_from_and_to_versions() {
        let dir = repo_v1();
        let plan = plan_of(vec![]);
        let steps = [step("echo $SPLICE_FROM_VERSION $SPLICE_TO_VERSION > v.log")];
        go(Run {
            repo: dir.path(),
            plan: &plan,
            steps: &steps,
            post: vec![],
            no_post: false,
        })
        .unwrap();
        assert_eq!(read(dir.path(), "v.log"), "1.0.0 2.0.0\n");
    }

    #[test]
    fn failing_step_leaves_old_lock() {
        let dir = repo_v1();
        let plan = plan_of(vec![planned("new.txt", Action::Create, Some("new\n"))]);
        let steps = [step("exit 7")];
        let e = go(Run {
            repo: dir.path(),
            plan: &plan,
            steps: &steps,
            post: vec![post("touch post.ran")],
            no_post: false,
        })
        .unwrap_err();
        assert_eq!(e.exit_code(), 3);
        let msg = e.to_string();
        assert!(msg.contains("git checkout -- . && git clean -fd"), "{msg}");
        assert!(msg.contains("exit 7") && msg.contains("2.0.0"), "{msg}");
        assert_eq!(
            read_lock(dir.path()).template.version,
            Version::parse("1.0.0").unwrap()
        );
        assert!(!dir.path().join("post.ran").exists());
    }

    #[test]
    fn failing_post_leaves_old_lock() {
        let dir = repo_v1();
        let plan = plan_of(vec![]);
        let e = go(Run {
            repo: dir.path(),
            plan: &plan,
            steps: &[],
            post: vec![post("exit 1")],
            no_post: false,
        })
        .unwrap_err();
        assert_eq!(e.exit_code(), 3);
        let msg = e.to_string();
        assert!(msg.contains("git checkout -- . && git clean -fd"), "{msg}");
        assert!(msg.contains("1.0.0") && msg.contains("`exit 1`"), "{msg}");
        assert_eq!(
            read_lock(dir.path()).template.version,
            Version::parse("1.0.0").unwrap()
        );
    }

    #[test]
    fn removal_of_an_already_gone_file_is_tolerated() {
        let dir = repo_v1();
        let plan = plan_of(vec![planned("gone.txt", Action::Delete, None)]);
        go(Run {
            repo: dir.path(),
            plan: &plan,
            steps: &[],
            post: vec![],
            no_post: false,
        })
        .unwrap();
    }

    #[test]
    fn drift_in_the_plan_is_refused_before_any_write() {
        let dir = repo_v1();
        let mut plan = plan_of(vec![planned("a.txt", Action::Create, Some("a\n"))]);
        plan.drift = vec!["old.txt".into()];
        let e = go(Run {
            repo: dir.path(),
            plan: &plan,
            steps: &[],
            post: vec![],
            no_post: false,
        })
        .unwrap_err();
        assert_eq!(e.exit_code(), 1);
        assert!(!dir.path().join("a.txt").exists());
    }

    #[test]
    fn conflicts_still_write_lock_and_exit_1_without_post() {
        let dir = repo_v1();
        let plan = plan_of(vec![planned(
            "old.txt",
            Action::Conflict,
            Some("<<<<<<< ours\nold\n=======\nnew\n>>>>>>> template\n"),
        )]);
        let e = go(Run {
            repo: dir.path(),
            plan: &plan,
            steps: &[],
            post: vec![post("touch post.ran")],
            no_post: false,
        })
        .unwrap_err();
        assert_eq!(e.exit_code(), 1);
        match e {
            SpliceError::Conflict(r) => {
                assert!(!r.ok);
                assert_eq!(r.changes.len(), 1);
            }
            other => panic!("{other:?}"),
        }
        assert!(read(dir.path(), "old.txt").contains("<<<<<<< ours"));
        assert_eq!(
            read_lock(dir.path()).template.version,
            Version::parse("2.0.0").unwrap()
        );
        assert!(!dir.path().join("post.ran").exists());
    }

    #[test]
    fn no_post_skips_post() {
        let dir = repo_v1();
        let plan = plan_of(vec![]);
        go(Run {
            repo: dir.path(),
            plan: &plan,
            steps: &[],
            post: vec![post("touch post.ran")],
            no_post: true,
        })
        .unwrap();
        assert!(!dir.path().join("post.ran").exists());
    }

    #[test]
    fn unsafe_target_refused_before_any_write() {
        let dir = repo_v1();
        let plan = plan_of(vec![
            planned("a.txt", Action::Create, Some("a\n")),
            planned("../escape.txt", Action::Create, Some("x\n")),
        ]);
        let e = go(Run {
            repo: dir.path(),
            plan: &plan,
            steps: &[],
            post: vec![],
            no_post: false,
        })
        .unwrap_err();
        assert_eq!(e.exit_code(), 3);
        // Nothing was written: no restore hint.
        assert!(!e.to_string().contains("git checkout"), "{e}");
        assert!(!dir.path().join("a.txt").exists());
        assert_eq!(
            read_lock(dir.path()).template.version,
            Version::parse("1.0.0").unwrap()
        );
    }

    #[test]
    fn skip_changes_touch_nothing_and_notes_become_messages() {
        let dir = repo_v1();
        let mut p = planned("old.txt", Action::Skip, None);
        p.note = Some("kept".into());
        let plan = plan_of(vec![p]);
        let report = go(Run {
            repo: dir.path(),
            plan: &plan,
            steps: &[],
            post: vec![],
            no_post: false,
        })
        .unwrap();
        assert_eq!(read(dir.path(), "old.txt"), "old\n");
        assert_eq!(report.messages, vec!["kept".to_string()]);
    }

    #[test]
    fn failure_after_a_write_carries_the_restore_hint() {
        let dir = repo_v1();
        std::fs::create_dir(dir.path().join("adir")).unwrap();
        let plan = plan_of(vec![
            planned("new.txt", Action::Create, Some("new\n")),
            planned("adir", Action::Delete, None),
        ]);
        let e = go(Run {
            repo: dir.path(),
            plan: &plan,
            steps: &[],
            post: vec![],
            no_post: false,
        })
        .unwrap_err();
        assert_eq!(e.exit_code(), 3);
        let msg = e.to_string();
        assert!(msg.contains("the lock still records 1.0.0"), "{msg}");
        assert!(msg.contains("git checkout -- . && git clean -fd"), "{msg}");
    }

    #[test]
    fn failed_lock_write_says_the_lock_still_records_from() {
        let dir = repo_v1();
        std::fs::remove_file(dir.path().join(LOCK_PATH)).unwrap();
        std::fs::create_dir(dir.path().join(LOCK_PATH)).unwrap();
        let plan = plan_of(vec![planned("new.txt", Action::Create, Some("new\n"))]);
        let e = go(Run {
            repo: dir.path(),
            plan: &plan,
            steps: &[],
            post: vec![],
            no_post: false,
        })
        .unwrap_err();
        assert_eq!(e.exit_code(), 3);
        let msg = e.to_string();
        assert!(msg.contains("the lock still records 1.0.0"), "{msg}");
        assert!(msg.contains("git checkout -- . && git clean -fd"), "{msg}");
    }
}
