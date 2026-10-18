//! `splice update` (spec §8.3): move the repo to a newer template version.

use std::collections::BTreeMap;
use std::path::Path;

use indexmap::IndexMap;
use semver::Version;

use crate::apply::{ApplyCtx, LockOut, apply};
use crate::cli::{GlobalArgs, UpdateArgs};
use crate::error::SpliceError;
use crate::fetch::{Fetcher, ResolvedTemplate};
use crate::filter::Filter;
use crate::git::Git;
use crate::lock::{ChannelLock, LOCK_PATH, Lock};
use crate::manifest::{PostCmd, Template, When};
use crate::migrate::select;
use crate::output::{Class, Report};
use crate::planner::{Disk, Plan, PlanInput, plan, render_removes};
use crate::render::{
    PieceInstance, RenderInput, RenderedSections, RenderedTree, render_sections, render_tree,
};
use crate::sections::{SectionChange, SectionInput, SectionPlan, plan_sections};
use crate::source::TemplateSource;
use crate::sources::{Claim, SourceId, Sources, check_claims, claims_last};
use crate::vars::{VarSource, instance_name, interactive, resolve_vars};

type Result<T> = std::result::Result<T, SpliceError>;

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
    /// A tracked branch head: `from`/`to` are `0.0.0+g…` and never compared; no migration
    /// steps run (B5).
    pub tracked: bool,
    /// Report messages (template warnings, moved tag).
    pub messages: Vec<String>,
    /// A channel update's lock, section plan and planned claim; `None` for the main template.
    pub channel: Option<ChannelPrepared>,
}

/// The channel part of [`Prepared`].
pub struct ChannelPrepared {
    /// The channel lock to write after applying the plan (`lock_new` of [`Prepared`] is its
    /// `lock`).
    pub lock_new: ChannelLock,
    pub sections: SectionPlan,
    /// What the channel owns after the update (checked against every other source).
    pub claim: Claim,
}

fn usage<T>(msg: String) -> Result<T> {
    Err(SpliceError::Usage(msg))
}

/// `base` first, then each `--piece ID[:NAME]`; checks ids, `multiple` and `requires`.
pub(crate) fn select_pieces(
    t: &Template,
    base: &str,
    specs: &[String],
) -> Result<Vec<(String, Option<String>)>> {
    let mut picked: Vec<(String, Option<String>)> = vec![(base.to_string(), None)];
    for spec in specs {
        let (id, name) = match spec.split_once(':') {
            Some((id, name)) if !name.is_empty() => (id, Some(name.to_string())),
            Some(_) => return usage(format!("--piece `{spec}`: the name after `:` is empty")),
            None => (spec.as_str(), None),
        };
        let Some(piece) = t.pieces.get(id) else {
            let known: Vec<&str> = t.pieces.keys().map(String::as_str).collect();
            return usage(format!(
                "unknown piece `{id}`; the template has: {}",
                known.join(", ")
            ));
        };
        if name.is_some() && !piece.multiple {
            return usage(format!(
                "piece `{id}` is not `multiple`; it cannot be named (`--piece {spec}`)"
            ));
        }
        if !piece.multiple && picked.iter().any(|(p, _)| p == id) {
            return usage(format!(
                "piece `{id}` is not `multiple` and is listed twice"
            ));
        }
        picked.push((id.to_string(), name));
    }
    for (id, _) in &picked {
        for req in &t.pieces[id].requires {
            if !picked.iter().any(|(p, _)| p == req) {
                return usage(format!(
                    "piece `{id}` requires piece `{req}`; add `--piece {req}`"
                ));
            }
        }
    }
    Ok(picked)
}

/// The template vars and piece instances of a fresh render (`new`, `channel add`): `base` plus
/// each `--piece ID[:NAME]`, `--data` checked against the template and the selected pieces,
/// then every var resolved (flags, prompts or defaults).
pub(crate) fn instances_for(
    template: &Template,
    base: &str,
    pieces: &[String],
    data: &IndexMap<String, String>,
    g: &GlobalArgs,
) -> Result<(IndexMap<String, String>, Vec<PieceInstance>)> {
    let picked = select_pieces(template, base, pieces)?;
    let known_keys = template.manifest.vars.keys().chain(
        picked
            .iter()
            .flat_map(|(id, _)| template.pieces[id].vars.keys()),
    );
    if let Some(key) = data.keys().find(|k| !known_keys.clone().any(|n| n == *k)) {
        return usage(format!(
            "--data `{key}`: no such variable in the template or the selected pieces"
        ));
    }
    let source = VarSource {
        data,
        interactive: interactive(g),
    };
    let template_vars = resolve_vars(&template.manifest.vars, &IndexMap::new(), &source)?;
    let mut instances: Vec<PieceInstance> = Vec::new();
    for (id, explicit) in &picked {
        let piece = &template.pieces[id];
        let mut known = IndexMap::new();
        if let Some(name) = explicit {
            known.insert("name".to_string(), name.clone());
        }
        let vars = resolve_vars(&piece.vars, &known, &source)?;
        let name = instance_name(piece, explicit.as_deref(), &vars);
        if instances.iter().any(|i| i.id == *id && i.name == name) {
            return usage(format!("piece `{id}` has two instances named `{name}`"));
        }
        instances.push(PieceInstance {
            id: id.clone(),
            name,
            vars,
        });
    }
    Ok((template_vars, instances))
}

/// The version an update moves to.
pub enum Target<'a> {
    /// The latest stable tag (none newer: up to date).
    Latest,
    /// `--to`.
    Version(&'a Version),
    /// A tracked branch head (Task 13).
    Branch(&'a str),
}

/// What a lock's source renders: base (lock state), theirs (target) and theirs' sections. The
/// section planner compares with the lock's sha, so there are no base sections (A9).
struct Rendered {
    base: RenderedTree,
    theirs: RenderedTree,
    theirs_sections: RenderedSections,
    /// The rendered `[[remove]]` paths of the selected migrations.
    removes: Vec<String>,
}

/// Steps 2–5 for one lock: the target, its manifest, the vars and instances and, unless up to
/// date, both renders.
struct Resolved {
    from: Version,
    target: ResolvedTemplate,
    template: Template,
    vars: IndexMap<String, String>,
    instances: Vec<PieceInstance>,
    /// No `--to` and no newer stable tag.
    no_newer: bool,
    /// A tracked branch head: no version comparison, no migrations (B5).
    tracked: bool,
    messages: Vec<String>,
    /// `None` when up to date and `render_current` is false.
    rendered: Option<Rendered>,
}

/// Which source a lock belongs to: the main template refuses `[sections]`; a channel may need
/// the renders even when its version is current (a filter change).
#[derive(Clone, Copy)]
enum Kind<'a> {
    Main,
    Channel {
        render_current: bool,
        /// Named in the downgrade message.
        lock_path: &'a str,
    },
}

/// Spec §8.3 steps 2–5 on any lock: pick the target, fetch both trees, check `min_splice`,
/// resolve vars and render base and theirs.
fn prepare_lock(
    g: &GlobalArgs,
    lock: &Lock,
    target: Target,
    data: &IndexMap<String, String>,
    kind: Kind,
) -> Result<Resolved> {
    let from = lock.template.version.clone();
    let mut messages = Vec::new();
    // `--data` may only set template vars the lock does not have yet (spec §8.3 step 4).
    if let Some((k, v)) = data.keys().find_map(|k| lock.vars.get_key_value(k)) {
        return usage(format!(
            "`{k}` is locked at `{v}`; update keeps lock values (spec §8.3 step 4)"
        ));
    }
    // 2. Target: `--to`, the latest stable tag or the tracked branch head; never older than
    // the lock (tracked heads `0.0.0+g…` are never compared: semver orders build metadata as
    // text, B5).
    let src = TemplateSource::parse(&lock.template.source)?;
    let fetcher = Fetcher::from_env();
    let (target, mut up_to_date, no_newer, tracked) = match target {
        Target::Branch(branch) => {
            let head = fetcher.branch_head(&src, branch)?;
            let current = head.commit == lock.template.commit;
            (head, current, false, true)
        }
        Target::Latest | Target::Version(_) => {
            let to = match target {
                Target::Version(v) => Some(v),
                _ => None,
            };
            let at = match kind {
                Kind::Main => String::new(),
                Kind::Channel { lock_path, .. } => format!(" ({lock_path})"),
            };
            let downgrade = |v: &Version| {
                SpliceError::Usage(format!(
                    "target {v} is older than the locked {from}{at}; splice does not downgrade"
                ))
            };
            if let Some(v) = to.filter(|v| *v < &from) {
                return Err(downgrade(v));
            }
            // Without `--to`: the latest stable tag. A template with prerelease tags only has
            // none, so there is nothing newer than the lock either.
            let latest = match to {
                Some(v) => Some(fetcher.resolve_version(&src, v)?),
                None => match fetcher.resolve(&src, None, false) {
                    Ok(t) => Some(t),
                    Err(_) if fetcher.available_versions(&src, false)?.is_empty() => None,
                    Err(e) => return Err(e),
                },
            };
            // No newer stable tag (e.g. a prerelease lock): stay on the lock's commit.
            let no_newer = to.is_none() && latest.as_ref().is_none_or(|t| t.version < from);
            let target = match latest {
                Some(t) if !no_newer => t,
                _ => fetcher.checkout(&src, &lock.template.commit, from.clone())?,
            };
            if target.version < from {
                return Err(downgrade(&target.version));
            }
            let current = target.version == from && target.commit == lock.template.commit;
            (target, current, no_newer, false)
        }
    };
    // G4 (channels only): a new commit whose template subtree equals the lock's is up to date.
    if !up_to_date && matches!(kind, Kind::Channel { .. }) {
        let tree = |c: &str| fetcher.subtree_id(&src, c).ok();
        if let Some(t) =
            tree(&target.commit).filter(|t| Some(t) == tree(&lock.template.commit).as_ref())
        {
            up_to_date = true;
            messages.push(format!(
                "{} ({}) has the same template tree ({}) as the lock",
                target.version,
                &target.commit[..7],
                &t[..7.min(t.len())]
            ));
        }
    }
    if !tracked && !up_to_date && target.version == from {
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
    if let Kind::Main = kind {
        template.ensure_no_sections(
            &lock
                .pieces
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
        )?;
    }

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
    for key in data.keys() {
        if template.manifest.vars.contains_key(key) {
            continue;
        }
        let piece_var = lock
            .pieces
            .iter()
            .any(|p| template.pieces[&p.id].vars.contains_key(key));
        return usage(if piece_var {
            format!(
                "--data `{key}` is a piece variable; update sets only new template variables \
                 (piece variables come from `splice add` or `splice new --piece`)"
            )
        } else {
            format!(
                "--data `{key}`: no such variable in template {}",
                target.version
            )
        });
    }
    let source = VarSource {
        data,
        interactive: interactive(g),
    };
    let no_data = IndexMap::new();
    let piece_source = VarSource {
        data: &no_data,
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
            vars: resolve_vars(&piece.vars, &inst.vars, &piece_source)?,
        });
    }

    let render_current = matches!(
        kind,
        Kind::Channel {
            render_current: true,
            ..
        }
    );
    let rendered = if up_to_date && !render_current {
        None
    } else {
        // 5. base = the lock state, theirs = the target with the new vars, same pieces.
        let theirs_input = RenderInput {
            template_vars: vars.clone(),
            instances: instances.clone(),
            version: target.version.clone(),
        };
        let base = if up_to_date {
            None
        } else {
            let old = fetcher.checkout(&src, &lock.template.commit, from.clone())?;
            let old_template = Template::load(&old.root)?;
            Some(render_tree(
                &old_template,
                &RenderInput {
                    template_vars: lock.vars.clone(),
                    instances: lock.instances(),
                    version: from.clone(),
                },
            )?)
        };
        let theirs = render_tree(&template, &theirs_input)?;
        let theirs_sections = match kind {
            Kind::Main => RenderedSections::new(),
            Kind::Channel { .. } => render_sections(&template, &theirs_input)?,
        };
        // Up to date: same commit, same vars, so the lock state renders exactly as theirs.
        let base = base.unwrap_or_else(|| theirs.clone());
        // Tracked heads run no migrations (B5).
        let removes = if tracked {
            Vec::new()
        } else {
            let migrations = select(&template.migrations, &from, &target.version);
            let removes: Vec<_> = migrations.iter().flat_map(|m| m.removes.iter()).collect();
            render_removes(&template, &target.version, &removes, &instances, &vars)?
        };
        Some(Rendered {
            base,
            theirs,
            theirs_sections,
            removes,
        })
    };
    Ok(Resolved {
        from,
        target,
        template,
        vars,
        instances,
        no_newer,
        tracked,
        messages,
        rendered,
    })
}

/// The "up to date" message of step 2.
fn up_to_date_message(r: &Resolved) -> String {
    if r.no_newer {
        format!(
            "up to date (no newer stable than {}; use --to <tag> for a prerelease)",
            r.from
        )
    } else {
        format!("up to date at {}", r.from)
    }
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
    let target = to.map_or(Target::Latest, Target::Version);
    let mut r = prepare_lock(g, &lock, target, data, Kind::Main)?;
    let Some(rendered) = r.rendered.take() else {
        r.messages.push(up_to_date_message(&r));
        return Ok(Prepared {
            plan: Plan::default(),
            lock_new: lock,
            from: r.from.clone(),
            to: r.from,
            template: r.template,
            instances: r.instances,
            up_to_date: true,
            tracked: r.tracked,
            messages: r.messages,
            channel: None,
        });
    };
    let Rendered {
        base,
        theirs,
        removes,
        ..
    } = rendered;

    // 6. Plan (pure) over a disk snapshot of every path involved.
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
    let lock_new = Lock::from_tree(&r.target, &r.vars, &r.instances, &theirs);
    Ok(Prepared {
        plan,
        lock_new,
        from: r.from,
        to: r.target.version,
        template: r.template,
        instances: r.instances,
        up_to_date: false,
        tracked: r.tracked,
        messages: r.messages,
        channel: None,
    })
}

/// [`prepare`] for channel `cl`: its own lock, vars and pieces (never the main template's),
/// base filtered with the stored filter and theirs with `filter_new` (or the stored one), the
/// claim check against `others` (every current source; this channel's claim is replaced and
/// checked last), the G10 seed drop, then the file and section plans. A new filter is stored in
/// the new lock's `[channel]` and replaces both lists (G16).
pub fn prepare_channel(
    g: &GlobalArgs,
    cl: &ChannelLock,
    others: &[Claim],
    to: Option<&Version>,
    filter_new: Option<(&[String], &[String])>,
    force: bool,
) -> Result<Prepared> {
    let repo = g.dir.as_path();
    let name = cl.meta.name.as_str();
    let lock_path = cl.path();
    let stored = Filter::from_meta(&cl.meta, &lock_path)?;
    let filter = match filter_new {
        Some((only, exclude)) => Filter::new(only, exclude)?,
        None => stored.clone(),
    };
    let target = match (&cl.meta.track, to) {
        (Some(branch), Some(_)) => {
            return usage(format!(
                "channel {name} tracks {branch}; re-add it with --ref to pin a version"
            ));
        }
        (Some(branch), None) => Target::Branch(branch),
        (None, Some(v)) => Target::Version(v),
        (None, None) => Target::Latest,
    };
    let kind = Kind::Channel {
        render_current: filter_new.is_some(),
        lock_path: &lock_path,
    };
    let mut r = prepare_lock(g, &cl.lock, target, &IndexMap::new(), kind)?;
    let me = SourceId::Channel(name.to_string());
    let Some(rendered) = r.rendered.take() else {
        r.messages.push(up_to_date_message(&r));
        return Ok(Prepared {
            plan: Plan::default(),
            lock_new: cl.lock.clone(),
            from: r.from.clone(),
            to: r.from,
            template: r.template,
            instances: r.instances,
            up_to_date: true,
            tracked: r.tracked,
            messages: r.messages,
            channel: Some(ChannelPrepared {
                lock_new: cl.clone(),
                sections: SectionPlan::default(),
                claim: Claim::from_lock(me, &cl.lock, &cl.sections),
            }),
        });
    };
    let Rendered {
        base,
        theirs,
        theirs_sections,
        removes,
    } = rendered;

    // Filters: base as locked (stored filter), theirs with the new one; dropped paths are
    // neither written nor locked, newly excluded locked paths are planned as gone.
    if filter_new.is_some() {
        let all: Vec<&str> = theirs
            .keys()
            .chain(theirs_sections.keys())
            .map(String::as_str)
            .collect();
        r.messages.extend(filter.unused(&all));
    }
    let base = stored.apply_tree(base);
    let mut theirs = filter.apply_tree(theirs);
    let removes: Vec<String> = removes.into_iter().filter(|p| stored.keeps(p)).collect();
    let hosts: Vec<String> = theirs_sections
        .keys()
        .filter(|h| filter.keeps(h))
        .cloned()
        .collect();

    // One owner per path across every source; this channel's new claim goes last.
    let claim = Claim::from_plan(me.clone(), &theirs, hosts.iter().cloned());
    let claims = claims_last(others.to_vec(), claim.clone());
    check_claims(&claims)?;
    // G10: a seed path another source locks is dropped from this plan.
    theirs.retain(|path, f| {
        f.class != Class::Seed
            || !claims
                .iter()
                .any(|c| c.source != me && c.files.contains(path))
    });

    let disk = Disk::snapshot(
        repo,
        base.keys()
            .chain(theirs.keys())
            .chain(removes.iter())
            .chain(hosts.iter())
            .chain(cl.sections.keys())
            .map(String::as_str),
    )?;
    let plan = plan(&PlanInput {
        base: &base,
        theirs: &theirs,
        disk: &disk,
        lock: &cl.lock,
        removes,
        force,
    });
    let sections = plan_sections(&SectionInput {
        channel: name,
        theirs: &theirs_sections,
        disk: &disk,
        locked: &cl.sections,
        filter: &filter,
        removing: false,
        force,
    });
    let mut meta = cl.meta.clone();
    if let Some((only, exclude)) = filter_new {
        meta.only = only.to_vec();
        meta.exclude = exclude.to_vec();
    }
    let lock = Lock::from_tree(&r.target, &r.vars, &r.instances, &theirs);
    let lock_new = ChannelLock {
        meta,
        lock: lock.clone(),
        sections: sections.locked.clone(),
    };
    Ok(Prepared {
        plan,
        lock_new: lock,
        from: r.from,
        to: r.target.version,
        template: r.template,
        instances: r.instances,
        up_to_date: false,
        tracked: r.tracked,
        messages: r.messages,
        channel: Some(ChannelPrepared {
            lock_new,
            sections,
            claim,
        }),
    })
}

/// Spec §8.3 step 1: refuse (exit 2) outside a git work tree or with uncommitted changes.
/// `command` names the splice command in the message.
pub fn require_clean(repo: &Path, command: &str) -> Result<()> {
    match Git::is_clean(repo)? {
        None => usage(format!(
            "{} is not inside a git work tree; splice {command} needs git to undo a bad \
             {command} (pass --allow-dirty to {command} anyway)",
            repo.display()
        )),
        Some(false) => usage(
            "the working tree has uncommitted changes; commit or stash them first \
             (or pass --allow-dirty)"
                .into(),
        ),
        Some(true) => Ok(()),
    }
}

/// Parse `--to` (a leading `v` is allowed).
pub fn parse_to(to: Option<&str>) -> Result<Option<Version>> {
    to.map(|v| {
        Version::parse(v.strip_prefix('v').unwrap_or(v))
            .map_err(|e| SpliceError::Usage(format!("--to `{v}` is not a version: {e}")))
    })
    .transpose()
}

pub fn run(g: &GlobalArgs, a: &UpdateArgs) -> Result<Report> {
    if a.all {
        return Err(super::channel::not_implemented("update --all"));
    }
    let repo = g.dir.as_path();

    // 1. A clean work tree is the undo button.
    if !a.allow_dirty {
        require_clean(repo, "update")?;
    }

    let to = parse_to(a.to.as_deref())?;
    let data = super::new::parse_data(&a.data)?;
    let sources = Sources::load(repo)?;
    if let Some(name) = &a.channel {
        if !data.is_empty() {
            return usage(
                "--data does not apply to `update --channel`: a channel's new variables take \
                 their defaults (or prompts)"
                    .into(),
            );
        }
        return run_channel(g, a, &sources, name, to.as_ref());
    }
    if sources.main.is_none() && !sources.channels.is_empty() {
        return usage(format!("no {LOCK_PATH}; use --channel NAME or --all"));
    }
    let prep = prepare(g, to.as_ref(), &data, a.force)?;

    if prep.up_to_date {
        let mut report = Report::new("update");
        report.messages = prep.messages;
        return Ok(report);
    }

    // B4: one owner per path across the main template and every channel (no channels: M2).
    if !sources.channels.is_empty() {
        check_claims(&claims_last(
            sources.claims(),
            Claim::from_lock(SourceId::Main, &prep.lock_new, &BTreeMap::new()),
        ))?;
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

    let lock = LockOut::Main(prep.lock_new.clone());
    apply_prepared(g, a, &prep, lock, &[])
}

/// `update --channel NAME`: spec §8.3 steps 2–8 on the channel lock, plus the claim check and
/// the section planner; only this channel's lock is written (last).
fn run_channel(
    g: &GlobalArgs,
    a: &UpdateArgs,
    sources: &Sources,
    name: &str,
    to: Option<&Version>,
) -> Result<Report> {
    let cl = sources.channel(name)?;
    let filter_new = (!a.only.is_empty() || !a.exclude.is_empty())
        .then_some((a.only.as_slice(), a.exclude.as_slice()));
    let prep = prepare_channel(g, cl, &sources.claims(), to, filter_new, a.force)?;
    let ch = prep.channel.as_ref().expect("a channel update");
    if prep.up_to_date {
        let mut report = Report::new("update");
        report.messages = prep.messages;
        return Ok(report);
    }

    // Owned drift, section drift or broken markers: write nothing, exit 1.
    let splan = &ch.sections;
    if !prep.plan.drift.is_empty() || !splan.drift.is_empty() || !splan.broken.is_empty() {
        let mut report = Report::new("update");
        report.ok = false;
        report.messages = prep.messages;
        for path in prep.plan.drift.iter().chain(&splan.drift) {
            report.msg(format!(
                "`{path}` is owned by channel {name} but was edited locally"
            ));
        }
        report.messages.extend(splan.broken.iter().cloned());
        report.msg(
            "nothing was written; revert the files above or pass --force to replace them \
             (broken markers are fixed by hand)",
        );
        return Err(SpliceError::Drift(report));
    }

    let lock = LockOut::Channel(ch.lock_new.clone());
    apply_prepared(g, a, &prep, lock, &splan.changes)
}

/// Step 7: files, sections, removals, migration steps, post, then `lock` last.
fn apply_prepared(
    g: &GlobalArgs,
    a: &UpdateArgs,
    prep: &Prepared,
    lock: LockOut,
    sections: &[SectionChange],
) -> Result<Report> {
    let migrations = if prep.tracked {
        Vec::new()
    } else {
        select(&prep.template.migrations, &prep.from, &prep.to)
    };
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
        repo: g.dir.as_path(),
        plan: &prep.plan,
        steps,
        from: prep.from.clone(),
        to: prep.to.clone(),
        lock,
        post,
        when: When::Update,
        no_post: a.no_post,
        json: g.json,
        command: "update",
        sections,
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
        Err(SpliceError::Applied(r)) => Err(SpliceError::Applied(prepend(r))),
        Err(e) => Err(e),
    }
}
