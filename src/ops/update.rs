//! `splice update` (spec §8.3). `prepare` is the pure part (steps 1-6, minus the dirty check) and
//! is shared with `diff` and `check`; `run` adds the dirty-tree check and applies the plan.

use super::{Locked, load_locked, parse_data, usage};
use crate::apply::{ApplyCtx, apply};
use crate::cli::{GlobalArgs, UpdateArgs};
use crate::error::SpliceError;
use crate::fetch::Fetcher;
use crate::git::Git;
use crate::lock::Lock;
use crate::manifest::{PostCmd, Template, When};
use crate::migrate::{Remove, select};
use crate::output::Report;
use crate::planner::{Disk, Plan, PlanInput, plan, render_removes};
use crate::render::{PieceInstance, RenderInput, render_tree};
use crate::source::TemplateSource;
use crate::vars::{VarSource, interactive, resolve_vars};
use indexmap::IndexMap;
use semver::Version;
use std::collections::BTreeSet;

type Result<T> = std::result::Result<T, SpliceError>;

/// Everything `update`, `diff` and `check` need to know about a move from the lock to a target.
pub struct Prepared {
    pub plan: Plan,
    /// The lock to write: the target state.
    pub lock_new: Lock,
    pub from: Version,
    pub to: Version,
    /// The target template (the lock's own template when `up_to_date`).
    pub template: Template,
    /// The piece instances with the target's vars.
    pub instances: Vec<PieceInstance>,
    /// Same version and same commit: nothing to do.
    pub up_to_date: bool,
    /// Lock and template warnings and notes, to become report messages.
    pub messages: Vec<String>,
}

fn running() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version is semver")
}

/// Vars of the target: lock values are kept, vars new in the target are resolved (`--data`,
/// prompt, default), vars it no longer declares are dropped. A piece instance the target lost,
/// or whose `requires` fail, is a template error.
fn resolve_target_vars(
    lock: &Lock,
    target: &Template,
    data: &IndexMap<String, String>,
    g: &GlobalArgs,
) -> Result<(IndexMap<String, String>, Vec<PieceInstance>)> {
    let locked = lock.instances();
    let ids: Vec<&str> = locked.iter().map(|i| i.id.as_str()).collect();
    for i in &locked {
        let Some(piece) = target.pieces.get(&i.id) else {
            return Err(SpliceError::Template(format!(
                "piece `{}` (instance `{}`) no longer exists in the target template; \
                 nothing was written",
                i.id, i.name
            )));
        };
        if let Some(r) = piece.requires.iter().find(|r| !ids.contains(&r.as_str())) {
            return Err(SpliceError::Template(format!(
                "piece `{}` requires piece `{r}` in the target template, but the repo has no `{r}`; \
                 nothing was written",
                i.id
            )));
        }
    }
    if !ids.contains(&target.manifest.base.as_str()) {
        return Err(SpliceError::Template(format!(
            "the target template's base piece `{}` is not in the lock; nothing was written",
            target.manifest.base
        )));
    }

    // the vars that are new in the target: the only ones --data may name
    let new_specs = |declared: &IndexMap<String, crate::manifest::VarSpec>,
                     have: &IndexMap<String, String>| {
        declared
            .iter()
            .filter(|(k, _)| !have.contains_key(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<IndexMap<_, _>>()
    };
    let mut new_names: BTreeSet<&String> = BTreeSet::new();
    let t_new = new_specs(&target.manifest.vars, &lock.vars);
    new_names.extend(t_new.keys());
    let i_new: Vec<_> = locked
        .iter()
        .map(|i| new_specs(&target.pieces[&i.id].vars, &i.vars))
        .collect();
    new_names.extend(i_new.iter().flat_map(|m| m.keys()));
    if let Some(k) = data.keys().find(|k| !new_names.contains(k)) {
        return Err(usage(format!(
            "--data `{k}` does not name a var that is new in the target; \
             existing values come from .splice/lock.toml"
        )));
    }

    let src = VarSource {
        data,
        interactive: interactive(g),
    };
    let none = IndexMap::new();
    let merge = |declared: &IndexMap<String, crate::manifest::VarSpec>,
                 have: &IndexMap<String, String>,
                 fresh: IndexMap<String, String>| {
        declared
            .keys()
            .filter_map(|k| {
                have.get(k)
                    .or_else(|| fresh.get(k))
                    .map(|v| (k.clone(), v.clone()))
            })
            .collect::<IndexMap<_, _>>()
    };
    let fresh = resolve_vars(&t_new, &none, &src)?;
    let vars = merge(&target.manifest.vars, &lock.vars, fresh);
    let mut instances = Vec::new();
    for (i, specs) in locked.iter().zip(&i_new) {
        let fresh = resolve_vars(specs, &none, &src)?;
        instances.push(PieceInstance {
            id: i.id.clone(),
            name: i.name.clone(),
            vars: merge(&target.pieces[&i.id].vars, &i.vars, fresh),
        });
    }
    Ok((vars, instances))
}

/// Nothing is new when the lock already is the target, so any `--data` key is an error.
fn up_to_date(
    lock: Lock,
    template: Template,
    data: &IndexMap<String, String>,
    mut messages: Vec<String>,
    note: String,
) -> Result<Prepared> {
    if let Some(k) = data.keys().next() {
        return Err(usage(format!(
            "--data `{k}` does not name a var that is new in the target; \
             existing values come from .splice/lock.toml"
        )));
    }
    messages.push(note);
    let from = lock.template.version.clone();
    Ok(Prepared {
        plan: Plan::default(),
        instances: lock.instances(),
        lock_new: lock,
        to: from.clone(),
        from,
        template,
        up_to_date: true,
        messages,
    })
}

/// A lock made with `new --ref` (not a tag) has version `0.0.0+g<sha>`.
fn from_ref(v: &Version) -> bool {
    v.major == 0 && v.minor == 0 && v.patch == 0 && v.build.as_str().starts_with('g')
}

/// Spec §8.3 steps 1-6 (the dirty check is `run`'s): resolve the target, vars, base and theirs,
/// plan. Writes nothing. With `force` owned drift is planned as an update instead of refused.
pub fn prepare(
    g: &GlobalArgs,
    to: Option<&Version>,
    data: &IndexMap<String, String>,
    force: bool,
) -> Result<Prepared> {
    let repo = g.dir.as_path();
    let Locked {
        lock,
        warnings,
        resolved: base_resolved,
        template: base_template,
    } = load_locked(repo)?;
    let mut messages: Vec<String> = warnings;
    messages.extend(base_template.warnings.iter().cloned());

    let source = TemplateSource::parse(&lock.template.source)?;
    let fetcher = Fetcher::from_env();
    let target = match to {
        Some(v) => fetcher.resolve_version(&source, v)?,
        None => fetcher.resolve(&source, None, false)?,
    };
    let from = lock.template.version.clone();
    if target.version < from {
        if to.is_some() {
            return Err(usage(format!(
                "target version {} is older than the locked version {from}; splice does not \
                 downgrade",
                target.version
            )));
        }
        // e.g. a lock at a prerelease newer than the latest stable tag
        let note = format!(
            "up to date at {from} (newer than the latest stable release {})",
            target.version
        );
        return up_to_date(lock, base_template, data, messages, note);
    }
    if target.version == from {
        if target.commit == lock.template.commit {
            let note = format!("up to date at {from}");
            return up_to_date(lock, base_template, data, messages, note);
        }
        messages.push(format!(
            "tag v{from} moved from {} to {}; re-rendering at the new commit",
            &lock.template.commit[..7.min(lock.template.commit.len())],
            &target.commit[..7.min(target.commit.len())]
        ));
    } else if from_ref(&from) {
        messages.push(format!(
            "the lock came from a ref ({}), not a release; updating to {}; \
             pass --to VERSION to choose another release",
            &lock.template.commit[..7.min(lock.template.commit.len())],
            target.version
        ));
    }

    let template = Template::load(&target.root)?;
    template.check_min_splice(&running())?;
    messages.extend(template.warnings.iter().cloned());

    let (vars, instances) = resolve_target_vars(&lock, &template, data, g)?;

    let base = render_tree(
        &base_template,
        &RenderInput {
            template_vars: lock.vars.clone(),
            instances: lock.instances(),
            version: base_resolved.version.clone(),
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

    let migrations = select(&template.migrations, &from, &target.version);
    let removes: Vec<&Remove> = migrations.iter().flat_map(|m| &m.removes).collect();
    let removes = render_removes(&template, &removes, &instances, &vars, &from)?;

    let mut paths: BTreeSet<&str> = BTreeSet::new();
    paths.extend(base.keys().map(String::as_str));
    paths.extend(theirs.keys().map(String::as_str));
    paths.extend(removes.iter().map(String::as_str));
    let disk = Disk::snapshot(repo, paths.iter().copied())?;

    let plan = plan(&PlanInput {
        base: &base,
        theirs: &theirs,
        disk: &disk,
        lock: &lock,
        removes,
        force,
    });
    let to_version = target.version.clone();
    let lock_new = Lock::from_tree(&target, &vars, &instances, &theirs);
    Ok(Prepared {
        plan,
        lock_new,
        from,
        to: to_version,
        template,
        instances,
        up_to_date: false,
        messages,
    })
}

fn prepend(report: &mut Report, msgs: &[String]) {
    report.messages.splice(0..0, msgs.iter().cloned());
}

pub fn run(g: &GlobalArgs, a: &UpdateArgs) -> Result<Report> {
    let repo = g.dir.as_path();
    let data = parse_data(&a.data)?;
    let to =
        a.to.as_deref()
            .map(|v| {
                Version::parse(v.strip_prefix('v').unwrap_or(v))
                    .map_err(|e| usage(format!("invalid --to version {v:?}: {e}")))
            })
            .transpose()?;

    // no lock is a usage error before the dirty check can mislead
    if Lock::read(repo)?.is_none() {
        return Err(usage(format!(
            "{} has no .splice/lock.toml; use `splice new` first",
            repo.display()
        )));
    }
    if !a.allow_dirty {
        match Git::is_clean(repo)? {
            None => {
                return Err(usage(format!(
                    "{} is not the top level of a git work tree; pass --allow-dirty to update anyway",
                    repo.display()
                )));
            }
            Some(false) => {
                return Err(usage(
                    "the working tree has uncommitted changes; commit or stash them, or pass \
                     --allow-dirty",
                ));
            }
            Some(true) => {}
        }
    }

    let p = prepare(g, to.as_ref(), &data, a.force)?;
    let mut report = Report::new("update");
    if p.up_to_date {
        prepend(&mut report, &p.messages);
        return Ok(report);
    }
    if !p.plan.drift.is_empty() {
        prepend(&mut report, &p.messages);
        for d in &p.plan.drift {
            report.msg(format!(
                "{d}: owned file was edited or deleted locally; update refused (--force replaces it)"
            ));
        }
        report.msg("nothing was written");
        return Err(SpliceError::Drift(report));
    }

    let migrations = select(&p.template.migrations, &p.from, &p.to);
    let steps = migrations
        .iter()
        .flat_map(|m| m.steps.iter().map(move |s| (&m.version, s)))
        .collect();
    let post: Vec<PostCmd> = p
        .instances
        .iter()
        .flat_map(|i| p.template.pieces[&i.id].post.clone())
        .collect();
    let result = apply(ApplyCtx {
        repo,
        plan: &p.plan,
        steps,
        from: p.from.clone(),
        to: p.to.clone(),
        lock: p.lock_new.clone(),
        post,
        when: When::Update,
        no_post: a.no_post,
        json: g.json,
    });
    match result {
        Ok(mut r) => {
            prepend(&mut r, &p.messages);
            Ok(r)
        }
        Err(SpliceError::Conflict(mut r)) => {
            prepend(&mut r, &p.messages);
            Err(SpliceError::Conflict(r))
        }
        Err(SpliceError::Partial {
            message,
            mut report,
        }) => {
            prepend(&mut report, &p.messages);
            Err(SpliceError::Partial { message, report })
        }
        Err(e) => Err(e),
    }
}
