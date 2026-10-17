//! `splice update` (spec §8.3): move the repo to a newer template version.

use indexmap::IndexMap;
use semver::Version;

use crate::apply::{ApplyCtx, apply};
use crate::cli::{GlobalArgs, UpdateArgs};
use crate::error::SpliceError;
use crate::fetch::Fetcher;
use crate::git::Git;
use crate::lock::{LOCK_PATH, Lock};
use crate::manifest::{PostCmd, Template, When};
use crate::migrate::select;
use crate::output::Report;
use crate::planner::{Disk, Plan, PlanInput, plan, render_removes};
use crate::render::{PieceInstance, RenderInput, render_tree};
use crate::source::TemplateSource;
use crate::vars::{VarSource, interactive, resolve_vars};

type Result<T> = std::result::Result<T, SpliceError>;

const RESTORE: &str = "`git checkout -- . && git clean -fd` restores the previous state";

/// Everything an update would do, computed without writing (spec §8.3 steps 1–6, without the
/// dirty-tree check). `plan` may hold drift and conflicts: callers decide what to do with them.
pub struct Prepared {
    pub plan: Plan,
    /// The lock to write after applying `plan`.
    pub lock_new: Lock,
    pub from: Version,
    pub to: Version,
    /// The target template.
    pub template: Template,
    /// The piece instances with their vars for the target.
    pub instances: Vec<PieceInstance>,
    /// Target version and commit equal the lock's: `plan` is empty and nothing is to be done.
    pub up_to_date: bool,
    /// Report messages (template warnings, moved tag).
    pub messages: Vec<String>,
}

fn usage<T>(msg: String) -> Result<T> {
    Err(SpliceError::Usage(msg))
}

/// Spec §8.3 steps 1–6 except the dirty check: read the lock, pick the target, fetch both trees,
/// check `min_splice`, resolve vars, render base and theirs, and plan.
pub fn prepare(
    g: &GlobalArgs,
    to: Option<&Version>,
    data: &IndexMap<String, String>,
    force: bool,
) -> Result<Prepared> {
    let repo = g.dir.as_path();
    let Some(lock) = Lock::read(repo)? else {
        return usage(format!(
            "{} has no {LOCK_PATH}; run `splice new` first",
            repo.display()
        ));
    };
    let from = lock.template.version.clone();
    let mut messages = Vec::new();

    // 2. Target: `--to` or the latest stable tag; never older than the lock.
    let src = TemplateSource::parse(&lock.template.source)?;
    let fetcher = Fetcher::from_env();
    let downgrade = |v: &Version| {
        SpliceError::Usage(format!(
            "target {v} is older than the locked {from}; splice does not downgrade"
        ))
    };
    if let Some(v) = to.filter(|v| *v < &from) {
        return Err(downgrade(v));
    }
    let target = match to {
        Some(v) => fetcher.resolve_version(&src, v)?,
        None => fetcher.resolve(&src, None, false)?,
    };
    if target.version < from {
        return Err(downgrade(&target.version));
    }
    let same_version = target.version == from;
    let up_to_date = same_version && target.commit == lock.template.commit;
    if same_version && !up_to_date {
        messages.push(format!(
            "tag v{from} moved from {} to {}; updating to the new commit",
            lock.template.commit, target.commit
        ));
    }

    // 3. The target manifest: format (checked by `load`) and min_splice, before any write.
    let template = Template::load(&target.root)?;
    let running = Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version");
    template.check_min_splice(&running)?;
    messages.extend(template.warnings.iter().cloned());

    // 4. Pieces and vars: every instance's piece must still exist and meet its `requires`.
    for inst in &lock.pieces {
        let Some(piece) = template.pieces.get(&inst.id) else {
            return Err(SpliceError::Template(format!(
                "template {} no longer has piece `{}` (instance `{}`); splice cannot update \
                 this repo to it",
                target.version, inst.id, inst.name
            )));
        };
        if let Some(req) = piece
            .requires
            .iter()
            .find(|r| !lock.pieces.iter().any(|p| p.id == **r))
        {
            return Err(SpliceError::Template(format!(
                "in template {}, piece `{}` requires piece `{req}`, which this repo does not have",
                target.version, inst.id
            )));
        }
    }
    let known_keys = || {
        template.manifest.vars.keys().chain(
            lock.pieces
                .iter()
                .flat_map(|p| template.pieces[&p.id].vars.keys()),
        )
    };
    if let Some(key) = data.keys().find(|k| !known_keys().any(|n| n == *k)) {
        return usage(format!(
            "--data `{key}`: no such variable in template {} or its pieces in this repo",
            target.version
        ));
    }
    let source = VarSource {
        data,
        interactive: interactive(g),
    };
    // Lock values are kept; vars new in the target are resolved; dropped vars disappear.
    let vars = resolve_vars(&template.manifest.vars, &lock.vars, &source)?;
    let mut instances = Vec::new();
    for inst in &lock.pieces {
        let piece = &template.pieces[&inst.id];
        instances.push(PieceInstance {
            id: inst.id.clone(),
            name: inst.name.clone(),
            vars: resolve_vars(&piece.vars, &inst.vars, &source)?,
        });
    }

    if up_to_date {
        return Ok(Prepared {
            plan: Plan::default(),
            lock_new: lock,
            from: from.clone(),
            to: from,
            template,
            instances,
            up_to_date,
            messages,
        });
    }

    // 5. base = the lock state, theirs = the target with the new vars, same pieces.
    let old = fetcher.checkout(&src, &lock.template.commit, from.clone())?;
    let old_template = Template::load(&old.root)?;
    let base = render_tree(
        &old_template,
        &RenderInput {
            template_vars: lock.vars.clone(),
            instances: lock.instances(),
            version: from.clone(),
        },
    )?;
    let theirs = render_tree(
        &template,
        &RenderInput {
            template_vars: vars.clone(),
            instances: instances.clone(),
            version: target.version.clone(),
        },
    )?;

    // 6. Plan (pure) over a disk snapshot of every path involved.
    let migrations = select(&template.migrations, &from, &target.version);
    let removes: Vec<_> = migrations.iter().flat_map(|m| m.removes.iter()).collect();
    let removes = render_removes(&template, &removes, &instances, &vars)?;
    let disk = Disk::snapshot(
        repo,
        base.keys()
            .chain(theirs.keys())
            .chain(removes.iter())
            .map(String::as_str),
    )?;
    let plan = plan(&PlanInput {
        base: &base,
        theirs: &theirs,
        disk: &disk,
        lock: &lock,
        removes,
        force,
    });
    let lock_new = Lock::from_tree(&target, &vars, &instances, &theirs);
    Ok(Prepared {
        plan,
        lock_new,
        from,
        to: target.version,
        template,
        instances,
        up_to_date,
        messages,
    })
}

pub fn run(g: &GlobalArgs, a: &UpdateArgs) -> Result<Report> {
    let repo = g.dir.as_path();

    // 1. A clean work tree is the undo button.
    if !a.allow_dirty {
        match Git::is_clean(repo)? {
            None => {
                return usage(format!(
                    "{} is not inside a git work tree; splice update needs git to undo a bad \
                     update (pass --allow-dirty to update anyway)",
                    repo.display()
                ));
            }
            Some(false) => {
                return usage(
                    "the working tree has uncommitted changes; commit or stash them first \
                     (or pass --allow-dirty)"
                        .into(),
                );
            }
            Some(true) => {}
        }
    }

    let to = match &a.to {
        Some(v) => Some(
            Version::parse(v.strip_prefix('v').unwrap_or(v))
                .map_err(|e| SpliceError::Usage(format!("--to `{v}` is not a version: {e}")))?,
        ),
        None => None,
    };
    let data = super::new::parse_data(&a.data)?;
    let prep = prepare(g, to.as_ref(), &data, a.force)?;

    if prep.up_to_date {
        let mut report = Report::new("update");
        report.messages = prep.messages;
        report.msg(format!("up to date at {}", prep.to));
        return Ok(report);
    }

    // 6. Owned drift without --force: write nothing, list the files, exit 1.
    if !prep.plan.drift.is_empty() {
        let mut report = Report::new("update");
        report.ok = false;
        report.messages = prep.messages;
        for path in &prep.plan.drift {
            report.msg(format!(
                "`{path}` is owned by the template but was edited locally"
            ));
        }
        report.msg("nothing was written; revert the files above or pass --force to replace them");
        return Err(SpliceError::Drift(report));
    }

    // 7. Apply: files, removals, migration steps, post, then the lock last.
    let migrations = select(&prep.template.migrations, &prep.from, &prep.to);
    let steps = migrations
        .iter()
        .flat_map(|m| m.steps.iter().map(move |s| (&m.version, s)))
        .collect();
    let mut post: Vec<PostCmd> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for inst in &prep.instances {
        if !seen.contains(&inst.id.as_str()) {
            seen.push(&inst.id);
            post.extend(prep.template.pieces[&inst.id].post.iter().cloned());
        }
    }
    let messages = prep.messages.clone();
    let result = apply(ApplyCtx {
        repo,
        plan: &prep.plan,
        steps,
        from: prep.from.clone(),
        to: prep.to.clone(),
        lock: prep.lock_new.clone(),
        post,
        when: When::Update,
        no_post: a.no_post,
        json: g.json,
    });
    let prepend = |mut r: Report| {
        let mut all = messages.clone();
        all.append(&mut r.messages);
        r.messages = all;
        r
    };
    match result {
        Ok(r) => Ok(prepend(r)),
        Err(SpliceError::Conflict(r)) => Err(SpliceError::Conflict(prepend(r))),
        Err(SpliceError::Drift(r)) => Err(SpliceError::Drift(prepend(r))),
        Err(SpliceError::Io(e)) => Err(SpliceError::Env(format!("{e}; {RESTORE}"))),
        Err(e) => Err(e),
    }
}
