//! `splice add` (spec §8.2): a new piece instance at the lock's template commit, through the
//! full planner (§8.4) with base = the lock state and theirs = the render with the new instance.

use indexmap::IndexMap;

use crate::apply::{ApplyCtx, LockOut, apply};
use crate::cli::{AddArgs, GlobalArgs};
use crate::error::SpliceError;
use crate::fetch::Fetcher;
use crate::filter::Filter;
use crate::lock::{ChannelLock, LOCK_PATH, Lock, LockPiece};
use crate::manifest::{PostCmd, Template, When};
use crate::output::{Action, Class, Report};
use crate::planner::{Disk, Plan, PlanInput, plan};
use crate::render::{PieceInstance, RenderInput, render_sections, render_tree};
use crate::sections::{SectionInput, SectionPlan, plan_sections};
use crate::source::TemplateSource;
use crate::sources::{Claim, SourceId, Sources, check_claims, claims_last};
use crate::vars::{VarSource, instance_name, interactive, resolve_vars};

type Result<T> = std::result::Result<T, SpliceError>;

fn usage<T>(msg: String) -> Result<T> {
    Err(SpliceError::Usage(msg))
}

/// `--force` on `add`: owned drift is already replaced by the planner; a conflict (shared file
/// that does not merge, or a user file where a seed became tracked) is kept as it is, never
/// overwritten and never given markers.
fn keep_conflicts(plan: &mut Plan) {
    for c in plan
        .changes
        .iter_mut()
        .filter(|c| c.action == Action::Conflict)
    {
        c.action = Action::Skip;
        c.bytes = None;
        c.note = Some(format!(
            "`{}` kept, merge by hand; the lock records the template's version",
            c.path
        ));
    }
    plan.conflicts.clear();
}

/// Validate the new instance `a.piece_id` against the template and the locked instances, then
/// resolve its vars (`--data`, prompts, defaults).
fn new_instance(
    template: &Template,
    locked: &[LockPiece],
    a: &AddArgs,
    g: &GlobalArgs,
) -> Result<PieceInstance> {
    let id = a.piece_id.as_str();
    let Some(piece) = template.pieces.get(id) else {
        let known: Vec<&str> = template.pieces.keys().map(String::as_str).collect();
        return usage(format!(
            "unknown piece `{id}`; the template has: {}",
            known.join(", ")
        ));
    };
    if a.name.is_some() && !piece.multiple {
        return usage(format!(
            "piece `{id}` is not `multiple`; it cannot be named (`--name`)"
        ));
    }
    if !piece.multiple && locked.iter().any(|p| p.id == id) {
        return usage(format!(
            "piece `{id}` is not `multiple` and is already present"
        ));
    }
    for req in &piece.requires {
        if !locked.iter().any(|p| p.id == *req) {
            return usage(format!(
                "piece `{id}` requires piece `{req}`; run `splice add {req}` first"
            ));
        }
    }
    let data = super::new::parse_data(&a.data)?;
    if let Some(key) = data.keys().find(|k| !piece.vars.contains_key(*k)) {
        return usage(format!(
            "--data `{key}`: no such variable in piece `{id}` (template variables are fixed in \
             the lock)"
        ));
    }
    let source = VarSource {
        data: &data,
        interactive: interactive(g),
    };
    let mut known = IndexMap::new();
    if let Some(name) = &a.name {
        known.insert("name".to_string(), name.clone());
    }
    let vars = resolve_vars(&piece.vars, &known, &source)?;
    let name = instance_name(piece, a.name.as_deref(), &vars);
    if locked.iter().any(|p| p.id == id && p.name == name) {
        return usage(format!(
            "piece `{id}` already has an instance named `{name}`"
        ));
    }
    Ok(PieceInstance {
        id: id.to_string(),
        name,
        vars,
    })
}

pub fn run(g: &GlobalArgs, a: &AddArgs) -> Result<Report> {
    let mut report = Report::new("add");
    let repo = g.dir.as_path();
    if !a.allow_dirty {
        super::update::require_clean(repo, "add")?;
    }
    if let Some(name) = &a.channel {
        return run_channel(g, a, name);
    }

    // 1. The lock, then the template at the lock's commit (no version change).
    let Some(lock) = Lock::read(repo)? else {
        return usage(format!(
            "{} has no {LOCK_PATH}; run `splice new` first",
            repo.display()
        ));
    };
    let src = TemplateSource::parse(&lock.template.source)?;
    let resolved =
        Fetcher::from_env().checkout(&src, &lock.template.commit, lock.template.version.clone())?;
    let template = Template::load(&resolved.root)?;
    report.messages.extend(template.warnings.iter().cloned());

    // 2. Validate the new instance before anything is rendered or written.
    let id = a.piece_id.as_str();
    // Refuses sections before the unknown-piece usage error below: a locked piece with
    // `[sections]` is exit 3 whatever piece is asked for.
    let mut used: Vec<&str> = lock.pieces.iter().map(|p| p.id.as_str()).collect();
    used.push(id);
    template.ensure_no_sections(&used)?;
    let new = new_instance(&template, &lock.pieces, a, g)?;

    // 3. Base = the lock state, theirs = with the new instance; both at the lock's commit.
    let mut instances: Vec<PieceInstance> = lock
        .pieces
        .iter()
        .map(|p| PieceInstance {
            id: p.id.clone(),
            name: p.name.clone(),
            vars: p.vars.clone(),
        })
        .collect();
    let input = |instances: &[PieceInstance]| RenderInput {
        template_vars: lock.vars.clone(),
        instances: instances.to_vec(),
        version: resolved.version.clone(),
    };
    let base = render_tree(&template, &input(&instances))?;
    instances.push(new);
    let theirs = render_tree(&template, &input(&instances))?;

    // B4: one owner per path across the main template and every channel (only when channels
    // exist: without them `add` is exactly M2's).
    let sources = Sources::load(repo)?;
    if !sources.channels.is_empty() {
        check_claims(&claims_last(
            sources.claims(),
            Claim::from_plan(SourceId::Main, &theirs, []),
        ))?;
    }

    // 4. Plan (pure) over a disk snapshot; no migrations: the commit does not change.
    let disk = Disk::snapshot(repo, base.keys().chain(theirs.keys()).map(String::as_str))?;
    let mut plan = plan(&PlanInput {
        base: &base,
        theirs: &theirs,
        disk: &disk,
        lock: &lock,
        removes: Vec::new(),
        force: a.force,
    });
    if a.force {
        keep_conflicts(&mut plan);
    }
    // Drift or a conflict (only without `--force`): write nothing, no files and no lock (C3).
    if !plan.drift.is_empty() || !plan.conflicts.is_empty() {
        return Err(refuse(report, &plan, &SectionPlan::default()));
    }

    // 5. Apply: files, removals, post of every piece present after the add, then the lock last.
    let mut post: Vec<PostCmd> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for inst in &instances {
        if !seen.contains(&inst.id.as_str()) {
            seen.push(&inst.id);
            post.extend(template.pieces[&inst.id].post.iter().cloned());
        }
    }
    let applied = apply(ApplyCtx {
        repo,
        plan: &plan,
        steps: Vec::new(),
        from: lock.template.version.clone(),
        to: resolved.version.clone(),
        lock: LockOut::Main(Lock::from_tree(&resolved, &lock.vars, &instances, &theirs)),
        post,
        when: When::Add,
        no_post: a.no_post,
        json: g.json,
        command: "add",
        sections: &[],
    })?;
    report.changes = applied.changes;
    report.messages.extend(applied.messages);
    Ok(report)
}

/// Drift or conflicts (owned files, sections): the C3 refusal, nothing written, exit 1.
fn refuse(mut report: Report, plan: &Plan, sections: &SectionPlan) -> SpliceError {
    for path in plan.drift.iter().chain(&sections.drift) {
        report.push(path, Action::Conflict, Class::Owned);
        report.msg(format!(
            "`{path}` is owned by the template but was edited locally"
        ));
    }
    report.messages.extend(sections.broken.iter().cloned());
    for c in plan.changes.iter().filter(|c| c.action == Action::Conflict) {
        report.push(&c.path, c.action, c.class);
        report.msg(c.note.clone().unwrap_or_default());
    }
    report.ok = false;
    report.msg("nothing was written; edit or move the files above, or pass --force");
    SpliceError::Conflict(report)
}

/// `splice add --channel NAME`: the new instance at the channel's commit, base = the channel lock
/// state, both through the stored filter; claims checked across every source; only this
/// channel's lock is written (last). Drift or a conflict writes nothing (C3, G11).
fn run_channel(g: &GlobalArgs, a: &AddArgs, name: &str) -> Result<Report> {
    let repo = g.dir.as_path();
    let mut report = Report::new("add");
    let sources = Sources::load(repo)?;
    let cl = sources.channel(name)?;
    let lock = &cl.lock;
    let lock_path = cl.path();
    let src = TemplateSource::parse(&lock.template.source)?;
    let resolved =
        Fetcher::from_env().checkout(&src, &lock.template.commit, lock.template.version.clone())?;
    let template = Template::load(&resolved.root)?;
    report.messages.extend(template.warnings.iter().cloned());
    let new = new_instance(&template, &lock.pieces, a, g)?;

    let filter = Filter::from_meta(&cl.meta, &lock_path)?;
    let mut instances = lock.instances();
    let input = |instances: &[PieceInstance]| RenderInput {
        template_vars: lock.vars.clone(),
        instances: instances.to_vec(),
        version: resolved.version.clone(),
    };
    let base = filter.apply_tree(render_tree(&template, &input(&instances))?);
    instances.push(new);
    let mut theirs = filter.apply_tree(render_tree(&template, &input(&instances))?);
    let sections = render_sections(&template, &input(&instances))?;
    let hosts: Vec<String> = sections
        .keys()
        .filter(|h| filter.keeps(h))
        .cloned()
        .collect();

    let me = SourceId::Channel(name.to_string());
    let claims = claims_last(
        sources.claims(),
        Claim::from_plan(me.clone(), &theirs, hosts.iter().cloned()),
    );
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
            .chain(hosts.iter())
            .chain(cl.sections.keys())
            .map(String::as_str),
    )?;
    let mut plan = plan(&PlanInput {
        base: &base,
        theirs: &theirs,
        disk: &disk,
        lock,
        removes: Vec::new(),
        force: a.force,
    });
    if a.force {
        keep_conflicts(&mut plan);
    }
    let splan = plan_sections(&SectionInput {
        channel: name,
        theirs: &sections,
        disk: &disk,
        locked: &cl.sections,
        filter: &filter,
        removing: false,
        force: a.force,
    });
    if !plan.drift.is_empty()
        || !plan.conflicts.is_empty()
        || !splan.drift.is_empty()
        || !splan.broken.is_empty()
    {
        return Err(refuse(report, &plan, &splan));
    }

    let mut post: Vec<PostCmd> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for inst in &instances {
        if !seen.contains(&inst.id.as_str()) {
            seen.push(&inst.id);
            post.extend(template.pieces[&inst.id].post.iter().cloned());
        }
    }
    let lock_new = ChannelLock {
        meta: cl.meta.clone(),
        lock: Lock::from_tree(&resolved, &lock.vars, &instances, &theirs),
        sections: splan.locked.clone(),
    };
    let applied = apply(ApplyCtx {
        repo,
        plan: &plan,
        steps: Vec::new(),
        from: lock.template.version.clone(),
        to: resolved.version.clone(),
        lock: LockOut::Channel(lock_new),
        post,
        when: When::Add,
        no_post: a.no_post,
        json: g.json,
        command: "add",
        sections: &splan.changes,
    })?;
    report.changes = applied.changes;
    report.messages.extend(applied.messages);
    Ok(report)
}
