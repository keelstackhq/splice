//! `splice adopt` (spec §8.1a): the main template applied to an existing repo with no lock.

use std::collections::BTreeSet;

use crate::apply::{ApplyCtx, LockOut, apply, check_targets};
use crate::cli::{AdoptArgs, GlobalArgs};
use crate::error::SpliceError;
use crate::fetch::Fetcher;
use crate::lock::{LOCK_PATH, Lock};
use crate::manifest::{PostCmd, Template, When};
use crate::output::{Action, Class, Report};
use crate::planner::{Disk, PlanInput, plan};
use crate::render::{RenderInput, RenderedTree, render_tree};
use crate::source::TemplateSource;

type Result<T> = std::result::Result<T, SpliceError>;

pub fn run(g: &GlobalArgs, a: &AdoptArgs) -> Result<Report> {
    let repo = g.dir.as_path();
    if repo.join(LOCK_PATH).exists() {
        return Err(SpliceError::Usage(format!(
            "{} already has a {LOCK_PATH}; use `splice update` or `splice add`",
            repo.display()
        )));
    }
    if !a.allow_dirty {
        super::update::require_clean(repo, "adopt")?;
    }

    // Resolve and render exactly like `new`.
    let src = TemplateSource::parse(&a.source)?;
    let resolved = Fetcher::from_env().resolve(&src, a.git_ref.as_deref(), a.pre)?;
    let template = Template::load(&resolved.root)?;
    let running = semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version");
    template.check_min_splice(&running)?;
    let mut report = Report::new("adopt");
    report.messages.extend(template.warnings.iter().cloned());
    let picked = super::update::select_pieces(&template, &template.manifest.base, &a.pieces)?;
    template.ensure_no_sections(&picked.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>())?;
    let data = super::new::parse_data(&a.data)?;
    let (vars, instances) =
        super::update::instances_for(&template, &template.manifest.base, &a.pieces, &data, g)?;
    let theirs = render_tree(
        &template,
        &RenderInput {
            template_vars: vars.clone(),
            instances: instances.clone(),
            version: resolved.version.clone(),
        },
    )?;

    // Plan against an empty base: nothing is locked yet.
    let empty = RenderedTree::new();
    let disk = Disk::snapshot(repo, theirs.keys().map(String::as_str))?;
    let none_locked = Lock::from_tree(&resolved, &vars, &instances, &empty);
    let fplan = plan(&PlanInput {
        base: &empty,
        theirs: &theirs,
        disk: &disk,
        lock: &none_locked,
        removes: Vec::new(),
        force: a.force,
    });
    for c in fplan.changes.iter().filter(|c| c.action == Action::Update) {
        // Only a forced owned drift is an `update` against an empty base.
        report.msg(format!(
            "replace: `{}` exists and differs from the template's owned file; {}",
            c.path,
            if a.dry_run {
                "would be replaced"
            } else {
                "replaced"
            }
        ));
    }
    let mut changes = Vec::new();
    // Paths the planner left alone (identical owned files, kept seeds) are reported as `skip`.
    let planned: BTreeSet<&str> = fplan
        .changes
        .iter()
        .map(|c| c.path.as_str())
        .chain(fplan.drift.iter().map(String::as_str))
        .collect();
    for (path, f) in &theirs {
        if !planned.contains(path.as_str()) {
            changes.push((path.clone(), Action::Skip, f.class));
        }
    }
    let with_skips = |mut r: Report| {
        for (p, act, class) in &changes {
            r.push(p, *act, *class);
        }
        let mut all = report.messages.clone();
        all.append(&mut r.messages);
        r.messages = all;
        r
    };

    // Files, post, then the lock last (atomically).
    let mut post: Vec<PostCmd> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for inst in &instances {
        if !seen.contains(&inst.id.as_str()) {
            seen.push(&inst.id);
            post.extend(template.pieces[&inst.id].post.iter().cloned());
        }
    }
    let lock_out = LockOut::Main(Lock::from_tree(&resolved, &vars, &instances, &theirs));
    let still = lock_out.still_records(&resolved.version);
    let none_yet = format!("no {LOCK_PATH} was written");
    // The restore hint would wipe the user's own uncommitted work under --allow-dirty.
    let hint = "; `git checkout -- . && git clean -fd` restores the previous state";
    let dirty_note = "; the working tree had uncommitted changes, so review `git status`";
    let swap = |m: &str| {
        let m = m.replace(&still, &none_yet);
        if a.allow_dirty {
            m.replace(hint, dirty_note)
        } else {
            m
        }
    };
    let fix = |mut r: Report| {
        for m in &mut r.messages {
            *m = swap(m);
        }
        with_skips(r)
    };
    let ctx = ApplyCtx {
        repo,
        plan: &fplan,
        steps: Vec::new(),
        from: resolved.version.clone(),
        to: resolved.version.clone(),
        lock: lock_out,
        post,
        when: When::New,
        no_post: a.no_post,
        json: g.json,
        command: "adopt",
        sections: &[],
    };

    let refuse = !a.force && !fplan.drift.is_empty();
    if a.dry_run {
        check_targets(&ctx)?;
        let mut r = Report::new("adopt");
        for c in &fplan.changes {
            r.push(&c.path, c.action, c.class);
        }
        for path in &fplan.drift {
            r.push(path, Action::Conflict, Class::Owned);
            r.msg(drift_msg(path));
        }
        r.msg("dry run: nothing written");
        r.ok = !refuse && fplan.conflicts.is_empty();
        let r = with_skips(r);
        return if r.ok {
            Ok(r)
        } else {
            Err(SpliceError::Drift(r))
        };
    }
    if refuse {
        let mut r = Report::new("adopt");
        for path in &fplan.drift {
            r.push(path, Action::Conflict, Class::Owned);
            r.msg(drift_msg(path));
        }
        r.msg("nothing was written");
        r.ok = false;
        return Err(SpliceError::Drift(with_skips(r)));
    }

    match apply(ctx) {
        Ok(r) => Ok(fix(r)),
        Err(SpliceError::Conflict(r)) => Err(SpliceError::Conflict(fix(r))),
        Err(SpliceError::Applied(r)) => Err(SpliceError::Applied(fix(r))),
        Err(SpliceError::Env(m)) => Err(SpliceError::Env(swap(&m))),
        Err(e) => Err(e),
    }
}

/// The stable, machine-detectable refusal line (`refuse: ` prefix, path in backticks).
fn drift_msg(path: &str) -> String {
    format!(
        "refuse: `{path}` exists and differs from the template's owned file; keep yours by not \
         adopting that piece, or pass --force to replace it"
    )
}
