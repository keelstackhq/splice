//! Apply a [`Plan`] (spec §8.3, §8.6): files, section hosts, removals, migration steps, `[[post]]`,
//! then the lock (main or a channel's).

use std::path::Path;

use semver::Version;

use crate::error::SpliceError;
use crate::fsio;
use crate::lock::{ChannelLock, LOCK_PATH, Lock};
use crate::manifest::{PostCmd, When};
use crate::migrate::Step;
use crate::output::{Action, Class, Report};
use crate::planner::Plan;
use crate::post::{run_post, run_steps};
use crate::sections::SectionChange;

/// The lock an operation writes last: the main lock or one channel's.
#[derive(Debug, Clone)]
pub enum LockOut {
    Main(Lock),
    Channel(ChannelLock),
}

impl LockOut {
    pub fn write_atomic(&self, repo: &Path) -> Result<(), SpliceError> {
        match self {
            LockOut::Main(l) => l.write_atomic(repo),
            LockOut::Channel(c) => c.write_atomic(repo),
        }
    }

    pub fn path(&self) -> String {
        match self {
            LockOut::Main(_) => LOCK_PATH.to_string(),
            LockOut::Channel(c) => c.path(),
        }
    }

    /// The channel of `HOST#CHANNEL` report paths (main never has sections).
    fn channel_name(&self) -> &str {
        match self {
            LockOut::Main(_) => "main",
            LockOut::Channel(c) => &c.meta.name,
        }
    }

    /// The template version this lock records.
    pub fn version(&self) -> &Version {
        match self {
            LockOut::Main(l) => &l.template.version,
            LockOut::Channel(c) => &c.lock.template.version,
        }
    }

    /// `the lock still records 1.0.0` (main, M2 text), or with the lock's path for a channel:
    /// `the lock still records 1.0.0 (.splice/channels/rig.toml)`.
    pub fn still_records(&self, from: &Version) -> String {
        match self {
            LockOut::Main(_) => format!("the lock still records {from}"),
            LockOut::Channel(c) => format!("the lock still records {from} ({})", c.path()),
        }
    }
}

pub struct ApplyCtx<'a> {
    pub repo: &'a Path,
    pub plan: &'a Plan,
    pub steps: Vec<(&'a Version, &'a Step)>,
    pub from: Version,
    pub to: Version,
    /// The lock to write: the state after this operation.
    pub lock: LockOut,
    pub post: Vec<PostCmd>,
    pub when: When,
    pub no_post: bool,
    pub json: bool,
    /// The report's `command` (`add`, `update`, `channel add`, …).
    pub command: &'static str,
    /// Section host writes and deletes (a channel's only; empty for main).
    pub sections: &'a [SectionChange],
}

/// The write phase alone: drift refusal, every target (and the lock path) checked before the
/// first write, then files, section hosts, removals and migration steps. No post, no lock: the
/// caller (`apply`, or `update --all`) runs those. Errors say the lock still records `from` and,
/// once something was written, how to restore.
pub fn apply_files(ctx: &ApplyCtx) -> Result<Report, SpliceError> {
    write_phase(ctx).map(|(report, _)| report)
}

/// Apply the plan in the fixed order: write files, section hosts, removals, migration steps,
/// `[[post]]`, and the lock (atomic) last. Every target is checked before the first write. A
/// failed step or post exits 3 before the lock is written, so it still records the old state.
/// With conflicts post is skipped, the lock is still written (it records the target) and the
/// result is `Err(Conflict)`. A plan with drift is refused (`Err(Drift)`) before anything is
/// written: the caller decides about `--force` when planning.
pub fn apply(ctx: ApplyCtx) -> Result<Report, SpliceError> {
    let (mut report, mut wrote) = write_phase(&ctx)?;
    let plan = ctx.plan;
    let still = ctx.lock.still_records(&ctx.from);
    if plan.conflicts.is_empty() && !ctx.no_post {
        if let Err(e) = run_post(ctx.repo, &ctx.post, ctx.when, ctx.json) {
            let e = match e {
                SpliceError::Env(m) => SpliceError::Env(format!(
                    "{m}; {still}; `git checkout -- . && git clean -fd` restores the previous state"
                )),
                other => other,
            };
            return Err(applied(report, e));
        }
        wrote |= ctx.post.iter().any(|p| p.when.contains(&ctx.when));
    }
    if let Err(e) = ctx.lock.write_atomic(ctx.repo) {
        return Err(applied(report, after_write(e, wrote, &still)));
    }

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

/// [`apply_files`] plus whether anything may have been written (for the restore hint).
fn write_phase(ctx: &ApplyCtx) -> Result<(Report, bool), SpliceError> {
    let mut report = Report::new(ctx.command);
    let plan = ctx.plan;
    let still = ctx.lock.still_records(&ctx.from);

    if !plan.drift.is_empty() {
        report.ok = false;
        for d in &plan.drift {
            report.msg(format!("`{d}` was edited locally"));
        }
        return Err(SpliceError::Drift(report));
    }

    fsio::check_target(ctx.repo, &ctx.lock.path())?;
    for c in &plan.changes {
        fsio::check_target(ctx.repo, &c.path)?;
    }
    for s in ctx.sections {
        fsio::check_target(ctx.repo, &s.host)?;
    }

    // From here on a failure may leave files written: say how to restore (only then), and list
    // the changes already applied (spec §9). `wrote` is set before each write attempt: a write
    // that fails midway may still have changed the file.
    let mut wrote = false;
    for c in plan.changes.iter().filter(|c| c.action != Action::Delete) {
        if let Some(bytes) = &c.bytes {
            wrote = true;
            if let Err(e) = fsio::write_file(ctx.repo, &c.path, bytes) {
                return Err(applied(report, after_write(e, wrote, &still)));
            }
        }
        push(&mut report, c);
    }
    // Section hosts: new bytes are written; `Delete` without bytes deletes the host.
    let channel = ctx.lock.channel_name();
    for s in ctx.sections {
        let done = match (&s.bytes, s.action) {
            (Some(bytes), _) => {
                wrote = true;
                fsio::write_file(ctx.repo, &s.host, bytes)
            }
            (None, Action::Delete) => match fsio::remove_file(ctx.repo, &s.host) {
                Err(SpliceError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                r => {
                    wrote |= r.is_ok();
                    r
                }
            },
            (None, _) => Ok(()),
        };
        if let Err(e) = done {
            return Err(applied(report, after_write(e, wrote, &still)));
        }
        report.push(s.report_path(channel), s.action, Class::Owned);
        if let Some(n) = &s.note {
            report.msg(n.clone());
        }
    }
    for c in plan.changes.iter().filter(|c| c.action == Action::Delete) {
        match fsio::remove_file(ctx.repo, &c.path) {
            Err(SpliceError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(applied(report, after_write(e, wrote, &still))),
            Ok(()) => wrote = true,
        }
        push(&mut report, c);
    }

    if let Err(e) = run_steps(ctx.repo, &ctx.steps, &ctx.from, &ctx.to, &still, ctx.json) {
        return Err(applied(report, e));
    }
    wrote |= !ctx.steps.is_empty();
    Ok((report, wrote))
}

/// Write every lock atomically, in the given order (`update --all`: main, then channels by
/// name), after all files and post commands. Each file is atomic on its own; a failure at one
/// lock names the locks already written and those not written (a re-run converges).
pub fn write_locks(repo: &Path, locks: &[&LockOut]) -> Result<(), SpliceError> {
    for (j, lock) in locks.iter().enumerate() {
        if let Err(e) = lock.write_atomic(repo) {
            let names = |ls: &[&LockOut]| {
                if ls.is_empty() {
                    "none".to_string()
                } else {
                    ls.iter().map(|l| l.path()).collect::<Vec<_>>().join(", ")
                }
            };
            return Err(SpliceError::Env(format!(
                "cannot write `{}`: {e}; locks written: {}; locks not written: {}; re-run the \
                 update to converge, or `git checkout -- . && git clean -fd` restores the \
                 previous state",
                lock.path(),
                names(&locks[..j]),
                names(&locks[j..]),
            )));
        }
    }
    Ok(())
}

fn push(report: &mut Report, c: &crate::planner::Planned) {
    report.push(c.path.clone(), c.action, c.class);
    if let Some(n) = &c.note {
        report.msg(n.clone());
    }
}

/// `e` with the changes applied so far: `Applied` (exit 3, `--json` lists them) once there are
/// any, else `e` itself.
fn applied(mut report: Report, e: SpliceError) -> SpliceError {
    if report.changes.is_empty() {
        return e;
    }
    report.ok = false;
    report.msg(e.to_string());
    SpliceError::Applied(report)
}

/// An error after the write phase began: it says which lock still records the old version
/// (`still`), and, when
/// something was written, how to restore the previous state.
fn after_write(e: SpliceError, wrote: bool, still: &str) -> SpliceError {
    let hint = if wrote {
        "; `git checkout -- . && git clean -fd` restores the previous state"
    } else {
        ""
    };
    SpliceError::Env(format!("{e}; {still}{hint}"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::lock::{ChannelMeta, LockTemplate};
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
        go_as(r, LockOut::Main(lock_at("2.0.0")), &[], "update")
    }

    const V2: Version = Version::new(2, 0, 0);

    fn ctx<'a>(
        r: Run<'a>,
        lock: LockOut,
        sections: &'a [SectionChange],
        command: &'static str,
    ) -> ApplyCtx<'a> {
        static TO: Version = V2;
        ApplyCtx {
            repo: r.repo,
            plan: r.plan,
            steps: r.steps.iter().map(|s| (&TO, s)).collect(),
            from: Version::new(1, 0, 0),
            to: V2,
            lock,
            post: r.post,
            when: When::Update,
            no_post: r.no_post,
            json: true,
            command,
            sections,
        }
    }

    fn go_as(
        r: Run,
        lock: LockOut,
        sections: &[SectionChange],
        command: &'static str,
    ) -> Result<Report, SpliceError> {
        apply(ctx(r, lock, sections, command))
    }

    fn chan_at(version: &str) -> ChannelLock {
        ChannelLock {
            meta: ChannelMeta {
                name: "rig".into(),
                ..Default::default()
            },
            lock: lock_at(version),
            sections: BTreeMap::new(),
        }
    }

    fn rig() -> LockOut {
        LockOut::Channel(chan_at("2.0.0"))
    }

    fn sec(host: &str, action: Action, bytes: Option<&str>) -> SectionChange {
        SectionChange {
            host: host.into(),
            action,
            bytes: bytes.map(|b| b.as_bytes().to_vec()),
            note: None,
        }
    }

    fn chan_version(repo: &Path) -> Version {
        ChannelLock::read(repo, "rig")
            .unwrap()
            .unwrap()
            .lock
            .template
            .version
    }

    fn quiet<'a>(repo: &'a Path, plan: &'a Plan, steps: &'a [Step]) -> Run<'a> {
        Run {
            repo,
            plan,
            steps,
            post: vec![],
            no_post: false,
        }
    }

    const CHAN: &str = ".splice/channels/rig.toml";

    #[test]
    fn section_written_before_removals_and_lock() {
        // A repo with no lock at all (`channel add` into a repo without the main template).
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("old.txt"), "old\n").unwrap();
        let plan = plan_of(vec![planned("old.txt", Action::Delete, None)]);
        let secs = [sec("AGENTS.md", Action::Create, Some("x\n"))];
        let steps = [step(&format!(
            "test -f AGENTS.md && test ! -f old.txt && test ! -f {CHAN} && touch step.ran"
        ))];
        let r = go_as(
            quiet(dir.path(), &plan, &steps),
            rig(),
            &secs,
            "channel add",
        )
        .unwrap();
        assert!(dir.path().join("step.ran").is_file());
        assert!(
            r.changes
                .iter()
                .any(|c| c.path == "AGENTS.md#rig" && c.class == Class::Owned),
            "{:?}",
            r.changes
        );
        assert_eq!(read(dir.path(), "AGENTS.md"), "x\n");
        assert!(dir.path().join(CHAN).is_file());
        assert!(!dir.path().join(LOCK_PATH).exists());
        assert_eq!(chan_version(dir.path()), V2);
    }

    #[test]
    fn section_written_before_a_failing_removal() {
        let dir = repo_v1();
        std::fs::create_dir(dir.path().join("adir")).unwrap();
        let plan = plan_of(vec![planned("adir", Action::Delete, None)]);
        let secs = [sec("AGENTS.md", Action::Create, Some("x\n"))];
        let e = go_as(quiet(dir.path(), &plan, &[]), rig(), &secs, "update").unwrap_err();
        assert_eq!(e.exit_code(), 3);
        let msg = e.to_string();
        assert!(
            msg.contains("the lock still records 1.0.0 (.splice/channels/rig.toml)"),
            "{msg}"
        );
        assert!(msg.contains("git checkout -- . && git clean -fd"), "{msg}");
        assert_eq!(read(dir.path(), "AGENTS.md"), "x\n");
        assert!(!dir.path().join(CHAN).exists());
    }

    #[test]
    fn section_delete_host_and_rewrite() {
        let dir = repo_v1();
        std::fs::write(dir.path().join("AGENTS.md"), "block\n").unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "# C\n\nblock\n").unwrap();
        let plan = plan_of(vec![]);
        let secs = [
            sec("AGENTS.md", Action::Delete, None),
            sec("CLAUDE.md", Action::Delete, Some("# C\n")),
        ];
        let r = go_as(quiet(dir.path(), &plan, &[]), rig(), &secs, "update").unwrap();
        assert!(!dir.path().join("AGENTS.md").exists());
        assert_eq!(read(dir.path(), "CLAUDE.md"), "# C\n");
        let got: Vec<_> = r
            .changes
            .iter()
            .map(|c| (c.path.as_str(), c.action, c.class))
            .collect();
        assert_eq!(
            got,
            [
                ("AGENTS.md#rig", Action::Delete, Class::Owned),
                ("CLAUDE.md#rig", Action::Delete, Class::Owned)
            ]
        );
    }

    #[test]
    fn section_skip_touches_nothing_and_its_note_is_a_message() {
        let dir = repo_v1();
        std::fs::write(dir.path().join("AGENTS.md"), "mine\n").unwrap();
        let plan = plan_of(vec![]);
        let mut s = sec("AGENTS.md", Action::Skip, None);
        s.note = Some("AGENTS.md#rig: block edited locally; kept".into());
        let r = go_as(quiet(dir.path(), &plan, &[]), rig(), &[s], "update").unwrap();
        assert_eq!(read(dir.path(), "AGENTS.md"), "mine\n");
        assert_eq!(r.messages, ["AGENTS.md#rig: block edited locally; kept"]);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_section_host_is_refused_with_nothing_written() {
        let dir = repo_v1();
        let outside = tempfile::tempdir().unwrap();
        let victim = outside.path().join("victim.md");
        std::fs::write(&victim, "keep\n").unwrap();
        std::os::unix::fs::symlink(&victim, dir.path().join("AGENTS.md")).unwrap();
        let plan = plan_of(vec![planned("new.txt", Action::Create, Some("new\n"))]);
        let secs = [sec("AGENTS.md", Action::Update, Some("x\n"))];
        let e = go_as(quiet(dir.path(), &plan, &[]), rig(), &secs, "update").unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("symlink"), "{e}");
        assert!(!e.to_string().contains("git checkout"), "{e}");
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep\n");
        assert!(!dir.path().join("new.txt").exists());
        assert!(!dir.path().join(CHAN).exists());
    }

    #[test]
    fn failing_step_leaves_old_channel_lock() {
        let dir = repo_v1();
        chan_at("1.0.0").write_atomic(dir.path()).unwrap();
        let plan = plan_of(vec![]);
        let e = go_as(
            quiet(dir.path(), &plan, &[step("exit 7")]),
            rig(),
            &[],
            "update",
        )
        .unwrap_err();
        assert_eq!(
            e.to_string(),
            "migration step `exit 7` (migration 2.0.0) failed (exit 7); the lock still records \
             1.0.0 (.splice/channels/rig.toml); `git checkout -- . && git clean -fd` restores the \
             previous state"
        );
        assert_eq!(chan_version(dir.path()), Version::new(1, 0, 0));
    }

    #[test]
    fn failing_post_leaves_old_channel_lock() {
        let dir = repo_v1();
        chan_at("1.0.0").write_atomic(dir.path()).unwrap();
        let plan = plan_of(vec![]);
        let mut r = quiet(dir.path(), &plan, &[]);
        r.post = vec![post("exit 1")];
        let e = go_as(r, rig(), &[], "update").unwrap_err();
        assert_eq!(
            e.to_string(),
            "post command `exit 1` failed (exit 1); the lock still records 1.0.0 \
             (.splice/channels/rig.toml); `git checkout -- . && git clean -fd` restores the \
             previous state"
        );
        assert_eq!(chan_version(dir.path()), Version::new(1, 0, 0));
    }

    #[test]
    fn main_messages_unchanged() {
        // Guard: M2's exact texts for the main lock.
        let dir = repo_v1();
        let plan = plan_of(vec![]);
        let e = go(quiet(dir.path(), &plan, &[step("exit 7")])).unwrap_err();
        assert_eq!(
            e.to_string(),
            "migration step `exit 7` (migration 2.0.0) failed (exit 7); the lock still records \
             1.0.0; `git checkout -- . && git clean -fd` restores the previous state"
        );
        let mut r = quiet(dir.path(), &plan, &[]);
        r.post = vec![post("exit 1")];
        assert_eq!(
            go(r).unwrap_err().to_string(),
            "post command `exit 1` failed (exit 1); the lock still records 1.0.0; \
             `git checkout -- . && git clean -fd` restores the previous state"
        );
    }

    #[test]
    fn main_lock_byte_identical_when_only_a_channel_changes() {
        let dir = repo_v1();
        let before = std::fs::read(dir.path().join(LOCK_PATH)).unwrap();
        let plan = plan_of(vec![planned("new.txt", Action::Create, Some("new\n"))]);
        let secs = [sec("AGENTS.md", Action::Create, Some("x\n"))];
        go_as(quiet(dir.path(), &plan, &[]), rig(), &secs, "update").unwrap();
        assert_eq!(std::fs::read(dir.path().join(LOCK_PATH)).unwrap(), before);
        assert_eq!(chan_version(dir.path()), V2);
    }

    #[test]
    fn apply_files_writes_no_lock_and_runs_no_post() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("old.txt"), "old\n").unwrap();
        let plan = plan_of(vec![
            planned("new.txt", Action::Create, Some("new\n")),
            planned("old.txt", Action::Delete, None),
        ]);
        let secs = [sec("AGENTS.md", Action::Create, Some("x\n"))];
        let steps = [step("touch step.ran")];
        let mut r = quiet(dir.path(), &plan, &steps);
        r.post = vec![post("touch post.ran")];
        let report = apply_files(&ctx(r, rig(), &secs, "update")).unwrap();
        assert_eq!(report.changes.len(), 3);
        assert_eq!(read(dir.path(), "new.txt"), "new\n");
        assert_eq!(read(dir.path(), "AGENTS.md"), "x\n");
        assert!(!dir.path().join("old.txt").exists());
        assert!(dir.path().join("step.ran").exists());
        assert!(!dir.path().join("post.ran").exists());
        assert!(!dir.path().join(".splice").exists());
    }

    #[test]
    fn apply_files_refuses_drift_and_checks_the_lock_path() {
        let dir = tempfile::tempdir().unwrap();
        let mut plan = plan_of(vec![planned("a.txt", Action::Create, Some("a\n"))]);
        plan.drift = vec!["x.txt".into()];
        let e = apply_files(&ctx(quiet(dir.path(), &plan, &[]), rig(), &[], "update"));
        assert_eq!(e.unwrap_err().exit_code(), 1);
        // A symlinked `.splice/channels` is refused before the first write.
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            std::fs::create_dir(dir.path().join(".splice")).unwrap();
            std::os::unix::fs::symlink(outside.path(), dir.path().join(".splice/channels"))
                .unwrap();
            let plan = plan_of(vec![planned("a.txt", Action::Create, Some("a\n"))]);
            let e = apply_files(&ctx(quiet(dir.path(), &plan, &[]), rig(), &[], "update"));
            assert_eq!(e.unwrap_err().exit_code(), 3);
        }
        assert!(!dir.path().join("a.txt").exists());
    }

    #[test]
    fn write_locks_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let main = LockOut::Main(lock_at("2.0.0"));
        let chan = chan_at("3.0.0");
        let rig = LockOut::Channel(chan.clone());
        write_locks(dir.path(), &[&main, &rig]).unwrap();
        let alone = tempfile::tempdir().unwrap();
        main.write_atomic(alone.path()).unwrap();
        assert_eq!(read(dir.path(), LOCK_PATH), read(alone.path(), LOCK_PATH));
        assert_eq!(read(dir.path(), CHAN), chan.to_toml());
        assert_eq!(rig.version(), &Version::new(3, 0, 0));
        assert_eq!(rig.path(), CHAN);
        assert_eq!(main.path(), LOCK_PATH);
    }

    #[test]
    fn write_locks_failure_names_unwritten() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".splice")).unwrap();
        std::fs::write(dir.path().join(".splice/channels"), "not a dir").unwrap();
        let main = LockOut::Main(lock_at("2.0.0"));
        let rig = LockOut::Channel(chan_at("2.0.0"));
        let e = write_locks(dir.path(), &[&main, &rig]).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        let msg = e.to_string();
        assert!(
            msg.contains("locks written: .splice/lock.toml;")
                && msg.contains("locks not written: .splice/channels/rig.toml;"),
            "{msg}"
        );
        assert!(msg.contains("git checkout -- . && git clean -fd"), "{msg}");
        assert!(dir.path().join(LOCK_PATH).is_file());
    }

    #[test]
    fn command_name_in_report() {
        let dir = tempfile::tempdir().unwrap();
        let plan = plan_of(vec![]);
        let r = go_as(quiet(dir.path(), &plan, &[]), rig(), &[], "channel add").unwrap();
        assert_eq!(r.command, "channel add");
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
    fn failed_first_write_has_the_restore_hint() {
        // `adir` is a directory, so writing it fails after the target check passed.
        let dir = repo_v1();
        std::fs::create_dir(dir.path().join("adir")).unwrap();
        let plan = plan_of(vec![planned("adir", Action::Create, Some("x\n"))]);
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
        assert!(msg.contains("git checkout -- . && git clean -fd"), "{msg}");
        assert!(msg.contains("the lock still records 1.0.0"), "{msg}");
    }

    #[test]
    fn failed_step_lists_the_applied_changes() {
        let dir = repo_v1();
        let plan = plan_of(vec![planned("new.txt", Action::Create, Some("new\n"))]);
        let e = go(Run {
            repo: dir.path(),
            plan: &plan,
            steps: &[step("exit 7")],
            post: vec![],
            no_post: false,
        })
        .unwrap_err();
        let SpliceError::Applied(r) = &e else {
            panic!("{e:?}");
        };
        assert_eq!(e.exit_code(), 3);
        assert!(!r.ok);
        assert_eq!(r.changes.len(), 1);
        assert_eq!(r.changes[0].path, "new.txt");
        assert!(r.messages.last().unwrap().contains("exit 7"));
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
        let lock_before = std::fs::read(dir.path().join(LOCK_PATH)).unwrap();
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
        assert_eq!(
            std::fs::read(dir.path().join(LOCK_PATH)).unwrap(),
            lock_before
        );
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
