//! `splice update` (spec §8.3): move the repo to a newer template version.

use std::collections::BTreeMap;
use std::path::Path;

use indexmap::IndexMap;
use semver::Version;

use crate::apply::{ApplyCtx, LockOut, apply, apply_files, check_targets, write_locks};
use crate::cli::{GlobalArgs, UpdateArgs};
use crate::error::SpliceError;
use crate::fetch::{Fetcher, ResolvedTemplate, is_untagged};
use crate::filter::Filter;
use crate::git::Git;
use crate::lock::{ChannelLock, LOCK_PATH, Lock};
use crate::manifest::{PostCmd, Template, When};
use crate::migrate::{Migration, Step, select};
use crate::output::{Action, Class, Report};
use crate::planner::{Disk, Plan, PlanInput, plan, render_removes};
use crate::post::run_post;
use crate::render::{
    PieceInstance, RenderInput, RenderedSections, RenderedTree, render_sections, render_tree,
};
use crate::sections::{SectionChange, SectionInput, SectionPlan, plan_sections};
use crate::source::TemplateSource;
use crate::sources::{Claim, SourceId, Sources, check_claims, claims_last};
use crate::vars::{
    VarSource, check_instance_name, instance_name, interactive, name_source, resolve_vars,
};

type Result<T> = std::result::Result<T, SpliceError>;

/// Planned writes by path (`None`: deleted), see [`prepare_channel`].
pub type Written = BTreeMap<String, Option<Vec<u8>>>;

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
    /// The versions of the selected migrations (`[[remove]]` and steps; none when tracked or up
    /// to date).
    pub migrations: Vec<Version>,
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
        check_instance_name(
            piece,
            &name,
            name_source(piece, explicit.as_deref(), &source),
        )?;
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
#[derive(Clone, Copy)]
pub enum Target<'a> {
    /// The latest stable tag (none newer: up to date).
    Latest,
    /// `--to`.
    Version(&'a Version),
    /// `--ref`: a tag, branch or sha that must descend from the locked commit (spec §4).
    Ref(&'a str),
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
    /// The versions of the selected migrations.
    migrations: Vec<Version>,
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
    force: bool,
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
    let src = TemplateSource::parse_from(&lock.template.source, g.dir.as_path())?;
    let fetcher = Fetcher::from_env();
    let (target, mut up_to_date, no_newer, tracked) = match target {
        Target::Branch(branch) => {
            let head = fetcher.branch_head(&src, branch)?;
            let current = head.commit == lock.template.commit;
            (head, current, false, true)
        }
        Target::Latest | Target::Version(_) | Target::Ref(_) => {
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
            // Untagged versions are never ordered by semver: `--ref`, an untagged lock or an
            // untagged target is compared by git ancestry (spec §4, §10 item 21).
            let by_ancestry = matches!(target, Target::Ref(_)) || is_untagged(&from);
            if !by_ancestry && let Some(v) = to.filter(|v| *v < &from) {
                return Err(downgrade(v));
            }
            // Without `--to`: the latest stable tag. A template with prerelease tags only has
            // none, so there is nothing newer than the lock either.
            let latest = match (target, to) {
                (Target::Ref(r), _) => Some(fetcher.resolve(&src, Some(r), false)?),
                (_, Some(v)) => Some(fetcher.resolve_version(&src, v)?),
                // An untagged lock: only a stable tag that descends from its commit (item 21).
                (_, None) if is_untagged(&from) => {
                    fetcher.newest_descendant_tag(&src, &lock.template.commit)?
                }
                (_, None) => match fetcher.resolve(&src, None, false) {
                    Ok(t) => Some(t),
                    Err(_) if fetcher.available_versions(&src, false)?.is_empty() => None,
                    Err(e) => return Err(e),
                },
            };
            // No newer stable tag (e.g. a prerelease lock): stay on the lock's commit.
            let no_newer = to.is_none()
                && !matches!(target, Target::Ref(_))
                && latest
                    .as_ref()
                    .is_none_or(|t| !is_untagged(&from) && t.version < from);
            let target = match latest {
                Some(t) if !no_newer => t,
                _ => fetcher.checkout(&src, &lock.template.commit, from.clone())?,
            };
            let current =
                target.commit == lock.template.commit && (by_ancestry || target.version == from);
            if by_ancestry && !current {
                if !fetcher.is_ancestor(&src, &lock.template.commit, &target.commit)? {
                    let (t7, l7) = (&target.commit[..7], &lock.template.commit[..7]);
                    if !force {
                        return usage(format!(
                            "target {} ({t7}) is not a descendant of the locked commit {l7}{at}; \
                             splice does not downgrade (pass --force to update anyway)",
                            target.version
                        ));
                    }
                    messages.push(format!(
                        "--force: {t7} is not a descendant of the locked {l7}; updating anyway"
                    ));
                }
            } else if !by_ancestry && target.version < from {
                return Err(downgrade(&target.version));
            }
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
    if !tracked && !up_to_date && !is_untagged(&from) && target.version == from {
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
    let lock_path = match kind {
        Kind::Main => crate::lock::LOCK_PATH,
        Kind::Channel { lock_path, .. } => lock_path,
    };
    let mut instances = Vec::new();
    for inst in &lock.pieces {
        let piece = &template.pieces[&inst.id];
        // The target is strict (contract §9.21): a locked name it rejects is a template error.
        if let Some(why) = crate::paths::segment_problem(&inst.name) {
            return Err(SpliceError::Template(format!(
                "{lock_path}: instance name `{}` of piece `{}` {why}; the template no longer \
                 accepts it (contract §9.21)",
                inst.name, inst.id
            )));
        }
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
            // Only the merge base: the contract §9.15-26 gates apply to the target alone.
            let old_template = Template::load_merge_base(&old.root)?;
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
        let (removes, migrations) = if tracked {
            (Vec::new(), Vec::new())
        } else {
            let migrations = select_migrations(
                &fetcher,
                &src,
                &template,
                (&from, &lock.template.commit),
                (&target.version, &target.commit),
            )?;
            let removes: Vec<_> = migrations.iter().flat_map(|m| m.removes.iter()).collect();
            // Contract §9.22: removes render with the version updated FROM.
            (
                render_removes(&template, &from, &removes, &instances, &vars)?,
                migrations.iter().map(|m| m.version.clone()).collect(),
            )
        };
        Some(Rendered {
            base,
            theirs,
            theirs_sections,
            removes,
            migrations,
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

/// The migrations an update from `from` to `to` (each a version and its commit) runs, in semver
/// order. Two tagged sides: `from < v <= to`, as the contract says. When either side is
/// untagged (spec §10 item 21): migration `v` runs when tag `v<v>` exists, its commit is an
/// ancestor of (or is) the target commit and is not an ancestor of (nor is) the lock commit.
fn select_migrations<'a>(
    fetcher: &Fetcher,
    src: &TemplateSource,
    template: &'a Template,
    (from, from_commit): (&Version, &str),
    (to, to_commit): (&Version, &str),
) -> Result<Vec<&'a Migration>> {
    if !is_untagged(from) && !is_untagged(to) {
        return Ok(select(&template.migrations, from, to));
    }
    if template.migrations.is_empty() {
        return Ok(Vec::new());
    }
    let tags = fetcher.tag_commits(src, true)?;
    let mut picked = Vec::new();
    for m in &template.migrations {
        let Some((_, tag_commit)) = tags.iter().find(|(v, _)| *v == m.version) else {
            continue;
        };
        if fetcher.is_ancestor(src, tag_commit, to_commit)?
            && !fetcher.is_ancestor(src, tag_commit, from_commit)?
        {
            picked.push(m);
        }
    }
    Ok(picked)
}

/// The "up to date" message of step 2.
fn up_to_date_message(r: &Resolved) -> String {
    if r.no_newer && is_untagged(&r.from) {
        format!(
            "up to date (no stable tag descends from the locked commit {}; use --ref \
             <branch|sha> for an untagged commit)",
            &r.target.commit[..7]
        )
    } else if r.no_newer {
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
    target: Target,
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
    let mut r = prepare_lock(g, &lock, target, data, Kind::Main, force)?;
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
            migrations: Vec::new(),
            messages: r.messages,
            channel: None,
        });
    };
    let Rendered {
        base,
        theirs,
        removes,
        migrations,
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
    let lock_new = Lock::from_tree(&r.target, &r.vars, &r.instances, &theirs)
        .keep_relative_source(&lock.template.source);
    Ok(Prepared {
        plan,
        lock_new,
        from: r.from,
        to: r.target.version,
        template: r.template,
        instances: r.instances,
        up_to_date: false,
        tracked: r.tracked,
        migrations,
        messages: r.messages,
        channel: None,
    })
}

/// [`prepare`] for channel `cl`: its own lock, vars and pieces (never the main template's),
/// base filtered with the stored filter and theirs with `filter_new` (or the stored one), the
/// claim check against `others` (every current source; this channel's claim is replaced and
/// checked last), the G10 seed drop, then the file and section plans. A new filter is stored in
/// the new lock's `[channel]` and replaces both lists (G16). `earlier` holds the planned writes
/// of the sources applied before this one in the same run (`update --all`; `None` deletes): the
/// plans see them instead of the disk, so a host another source seeds is planned against that
/// seed (`created = false`, G17), a seed it writes is not written twice and its section edits are
/// kept.
pub fn prepare_channel(
    g: &GlobalArgs,
    cl: &ChannelLock,
    others: &[Claim],
    target: Target,
    filter_new: Option<(&[String], &[String])>,
    force: bool,
    earlier: &Written,
) -> Result<Prepared> {
    let repo = g.dir.as_path();
    let name = cl.meta.name.as_str();
    let lock_path = cl.path();
    let stored = Filter::from_meta(&cl.meta, &lock_path)?;
    let filter = match filter_new {
        Some((only, exclude)) => Filter::new(only, exclude)?,
        None => stored.clone(),
    };
    let target = match (&cl.meta.track, target) {
        (Some(branch), Target::Version(_) | Target::Ref(_)) => {
            return usage(format!(
                "channel {name} tracks {branch}; re-add it with --ref to pin a version"
            ));
        }
        (Some(branch), _) => Target::Branch(branch),
        (None, t) => t,
    };
    let kind = Kind::Channel {
        render_current: filter_new.is_some(),
        lock_path: &lock_path,
    };
    let mut r = prepare_lock(g, &cl.lock, target, &IndexMap::new(), kind, force)?;
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
            migrations: Vec::new(),
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
        migrations,
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

    let mut disk = Disk::snapshot(
        repo,
        base.keys()
            .chain(theirs.keys())
            .chain(removes.iter())
            .chain(hosts.iter())
            .chain(cl.sections.keys())
            .map(String::as_str),
    )?;
    for (path, bytes) in earlier {
        if let Some(slot) = disk.files.get_mut(path) {
            slot.clone_from(bytes);
        }
    }
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
    let lock = Lock::from_tree(&r.target, &r.vars, &r.instances, &theirs)
        .keep_relative_source(&cl.lock.template.source);
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
        migrations,
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
        let parsed = Version::parse(v.strip_prefix('v').unwrap_or(v))
            .map_err(|e| SpliceError::Usage(format!("--to `{v}` is not a version: {e}")))?;
        if is_untagged(&parsed) {
            let sha = parsed.build.as_str().strip_prefix('g').unwrap_or("<sha>");
            return Err(SpliceError::Usage(format!(
                "--to `{v}` names an untagged commit, not a release; use --ref {sha}"
            )));
        }
        Ok(parsed)
    })
    .transpose()
}

/// `--to`/`--ref` as a [`Target`] (`Latest` when neither).
pub fn target_of<'a>(to: Option<&'a Version>, git_ref: Option<&'a str>) -> Target<'a> {
    match (to, git_ref) {
        (Some(v), _) => Target::Version(v),
        (None, Some(r)) => Target::Ref(r),
        (None, None) => Target::Latest,
    }
}

pub fn run(g: &GlobalArgs, a: &UpdateArgs) -> Result<Report> {
    if a.all {
        return run_all(g, a);
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
        return run_channel(
            g,
            a,
            &sources,
            name,
            target_of(to.as_ref(), a.git_ref.as_deref()),
        );
    }
    if sources.main.is_none() && !sources.channels.is_empty() {
        return usage(format!("no {LOCK_PATH}; use --channel NAME or --all"));
    }
    let target = target_of(to.as_ref(), a.git_ref.as_deref());
    let prep = prepare(g, target, &data, a.force)?;

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
    target: Target,
) -> Result<Report> {
    let cl = sources.channel(name)?;
    let filter_new = (!a.only.is_empty() || !a.exclude.is_empty())
        .then_some((a.only.as_slice(), a.exclude.as_slice()));
    let prep = prepare_channel(
        g,
        cl,
        &sources.claims(),
        target,
        filter_new,
        a.force,
        &Written::new(),
    )?;
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
    let messages = prep.messages.clone();
    let result = apply(ApplyCtx {
        repo: g.dir.as_path(),
        plan: &prep.plan,
        steps: steps_of(prep),
        from: prep.from.clone(),
        to: prep.to.clone(),
        lock,
        post: post_of(prep),
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

/// The migration steps between `from` and `to`, each with its migration's version (none for a
/// tracked head, B5).
/// The write-phase context of one `update --all` source: no post, no lock (written last).
fn all_ctx<'a>(repo: &'a Path, s: &'a AllSource, json: bool) -> ApplyCtx<'a> {
    ApplyCtx {
        repo,
        plan: &s.prep.plan,
        steps: steps_of(&s.prep),
        from: s.prep.from.clone(),
        to: s.prep.to.clone(),
        lock: s.lock_out(),
        post: Vec::new(),
        when: When::Update,
        no_post: true,
        json,
        command: "update",
        sections: s.sections(),
    }
}

fn steps_of(prep: &Prepared) -> Vec<(&Version, &Step)> {
    if prep.tracked {
        return Vec::new();
    }
    prep.template
        .migrations
        .iter()
        .filter(|m| prep.migrations.contains(&m.version))
        .flat_map(|m| m.steps.iter().map(move |s| (&m.version, s)))
        .collect()
}

/// The `[[post]]` commands of every piece in the update, once per piece id.
fn post_of(prep: &Prepared) -> Vec<PostCmd> {
    let mut post: Vec<PostCmd> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for inst in &prep.instances {
        if !seen.contains(&inst.id.as_str()) {
            seen.push(&inst.id);
            post.extend(prep.template.pieces[&inst.id].post.iter().cloned());
        }
    }
    post
}

/// One source of `update --all` with its prepared update.
struct AllSource {
    id: SourceId,
    prep: Prepared,
}

impl AllSource {
    fn sections(&self) -> &[SectionChange] {
        self.prep
            .channel
            .as_ref()
            .map_or(&[][..], |c| c.sections.changes.as_slice())
    }

    fn lock_out(&self) -> LockOut {
        match &self.prep.channel {
            None => LockOut::Main(self.prep.lock_new.clone()),
            Some(c) => LockOut::Channel(c.lock_new.clone()),
        }
    }

    /// What this source writes, by path, for the plans of the sources after it.
    fn record_writes(&self, written: &mut Written) {
        for c in &self.prep.plan.changes {
            match (&c.bytes, c.action) {
                (Some(b), _) => {
                    written.insert(c.path.clone(), Some(b.clone()));
                }
                (None, Action::Delete) => {
                    written.insert(c.path.clone(), None);
                }
                _ => {}
            }
        }
        for s in self.sections() {
            match (&s.bytes, s.action) {
                (Some(b), _) => {
                    written.insert(s.host.clone(), Some(b.clone()));
                }
                (None, Action::Delete) => {
                    written.insert(s.host.clone(), None);
                }
                _ => {}
            }
        }
    }
}

const RESTORE: &str = "`git checkout -- . && git clean -fd` restores the previous state";

/// A failure after the first write of `update --all`: no lock was written. `done` holds the
/// changes applied so far; the error lists them (`Applied`) when there are any.
fn all_failed(mut done: Report, what: &str, e: SpliceError) -> SpliceError {
    let mut text = e.to_string();
    if let SpliceError::Applied(r) = e {
        done.changes.extend(r.changes);
    }
    if let Some(t) = text.strip_suffix(&format!("; {RESTORE}")) {
        text = t.to_string();
    }
    let msg = format!(
        "{what}{text}; no lock was written, every lock still records its old version; {RESTORE}"
    );
    if done.changes.is_empty() {
        return SpliceError::Env(msg);
    }
    done.ok = false;
    done.msg(msg);
    SpliceError::Applied(done)
}

/// `update --all` (channels spec §8, G12): plan the main template (if any) and every channel
/// (by name) in memory, check claims, refuse drift; then every source's files, sections,
/// removals and steps, the deduplicated post commands once, and every lock last.
fn run_all(g: &GlobalArgs, a: &UpdateArgs) -> Result<Report> {
    let repo = g.dir.as_path();
    if !a.allow_dirty {
        require_clean(repo, "update")?;
    }
    let sources = Sources::load(repo)?;
    if sources.main.is_none() && sources.channels.is_empty() {
        return usage(format!(
            "{} has no {LOCK_PATH} and no channel locks; run `splice new` or `splice channel \
             add` first",
            repo.display()
        ));
    }

    // 1. Plan every source. `planned` holds the new claims in source order; a channel is checked
    // against them and the current claims of the channels after it.
    let current = sources.claims();
    let mut planned: Vec<Claim> = Vec::new();
    let mut written = Written::new();
    let mut all: Vec<AllSource> = Vec::new();
    if sources.main.is_some() {
        let prep = prepare(g, Target::Latest, &IndexMap::new(), a.force)?;
        planned.push(Claim::from_lock(
            SourceId::Main,
            &prep.lock_new,
            &BTreeMap::new(),
        ));
        let src = AllSource {
            id: SourceId::Main,
            prep,
        };
        src.record_writes(&mut written);
        all.push(src);
    }
    for cl in &sources.channels {
        let id = SourceId::Channel(cl.meta.name.clone());
        // Not `claims_last`: that takes one source's new claim, but here every earlier source
        // already has a planned claim (replacing its current one) and later ones keep theirs;
        // `prepare_channel` puts this channel's claim last itself.
        let others: Vec<Claim> = planned
            .iter()
            .chain(
                current
                    .iter()
                    .filter(|c| !planned.iter().any(|p| p.source == c.source)),
            )
            .cloned()
            .collect();
        let prep = prepare_channel(g, cl, &others, Target::Latest, None, a.force, &written)?;
        let ch = prep.channel.as_ref().expect("a channel update");
        planned.push(ch.claim.clone());
        let src = AllSource { id, prep };
        src.record_writes(&mut written);
        all.push(src);
    }

    // 2. One owner per path across the planned results.
    check_claims(&planned)?;

    // 3. Drift or broken markers in any source: write nothing, exit 1.
    let prefixed = |id: &SourceId, msgs: &[String]| -> Vec<String> {
        msgs.iter().map(|m| format!("{id}: {m}")).collect()
    };
    let mut refused = Report::new("update");
    for s in &all {
        let (drift, broken) = match &s.prep.channel {
            None => (s.prep.plan.drift.clone(), Vec::new()),
            Some(c) => (
                s.prep
                    .plan
                    .drift
                    .iter()
                    .chain(&c.sections.drift)
                    .cloned()
                    .collect(),
                c.sections.broken.clone(),
            ),
        };
        let owner = match &s.id {
            SourceId::Main => "the template".to_string(),
            id => id.to_string(),
        };
        let mut lines: Vec<String> = drift
            .iter()
            .map(|p| format!("`{p}` is owned by {owner} but was edited locally"))
            .collect();
        lines.extend(broken);
        refused.messages.extend(prefixed(&s.id, &lines));
    }
    if !refused.messages.is_empty() {
        refused.ok = false;
        refused.msg(
            "nothing was written; revert the files above or pass --force to replace them \
             (broken markers are fixed by hand)",
        );
        return Err(SpliceError::Drift(refused));
    }

    // 4. Files, sections, removals and steps of every source; no post, no lock.
    let mut report = Report::new("update");
    let changed: Vec<&AllSource> = all.iter().filter(|s| !s.prep.up_to_date).collect();
    for s in &all {
        if s.prep.up_to_date {
            report.messages.extend(prefixed(&s.id, &s.prep.messages));
        }
    }
    let ctx_of = |s| all_ctx(repo, s, g.json);
    // Every source's targets before the first write of any source: a bad target in a later
    // source must not leave an earlier source's files written.
    for s in &changed {
        if let Err(e) = check_targets(&ctx_of(s)) {
            return Err(SpliceError::Env(format!(
                "{}: {e}; nothing was written",
                s.id
            )));
        }
    }
    let mut conflicts: Vec<(&SourceId, &Vec<String>)> = Vec::new();
    for s in &changed {
        let ctx = ctx_of(s);
        report.msg(s.id.tag());
        report.messages.extend(prefixed(&s.id, &s.prep.messages));
        match apply_files(&ctx) {
            Ok(r) => {
                report.changes.extend(r.changes);
                report.messages.extend(prefixed(&s.id, &r.messages));
            }
            Err(e) => return Err(all_failed(report, &format!("{}: ", s.id), e)),
        }
        if !s.prep.plan.conflicts.is_empty() {
            conflicts.push((&s.id, &s.prep.plan.conflicts));
        }
    }

    // 5. The post commands of every changed source once (first occurrence wins), before any
    // lock: post sees the old locks.
    if conflicts.is_empty() && !a.no_post {
        let post: Vec<PostCmd> = changed.iter().flat_map(|s| post_of(&s.prep)).collect();
        if let Err(e) = run_post(repo, &post, When::Update, g.json) {
            return Err(all_failed(report, "", e));
        }
    }

    // 6. Every lock last, each atomically.
    let locks: Vec<LockOut> = changed.iter().map(|s| s.lock_out()).collect();
    report.changes.sort_by(|x, y| x.path.cmp(&y.path));
    if let Err(e) = write_locks(repo, &locks.iter().collect::<Vec<_>>()) {
        report.ok = false;
        report.msg(e.to_string());
        return Err(if report.changes.is_empty() {
            e
        } else {
            SpliceError::Applied(report)
        });
    }

    if conflicts.is_empty() {
        Ok(report)
    } else {
        report.ok = false;
        // One line per source: a global line after the last source's tag would read as its own.
        for (id, paths) in conflicts {
            report.msg(format!(
                "{id}: conflicts in {}: resolve the diff3 markers",
                paths.join(", ")
            ));
        }
        Err(SpliceError::Conflict(report))
    }
}
