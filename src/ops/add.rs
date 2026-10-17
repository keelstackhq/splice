//! `splice add` (spec §8.2): a new piece instance at the lock's template commit, through the
//! full planner (§8.4) with base = the lock state and theirs = the render with the new instance.

use indexmap::IndexMap;

use crate::apply::{ApplyCtx, LockOut, apply};
use crate::cli::{AddArgs, GlobalArgs};
use crate::error::SpliceError;
use crate::fetch::Fetcher;
use crate::lock::{LOCK_PATH, Lock};
use crate::manifest::{PostCmd, Template, When};
use crate::output::{Action, Class, Report};
use crate::planner::{Disk, Plan, PlanInput, plan};
use crate::render::{PieceInstance, RenderInput, render_tree};
use crate::source::TemplateSource;
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

pub fn run(g: &GlobalArgs, a: &AddArgs) -> Result<Report> {
    let mut report = Report::new("add");
    let repo = g.dir.as_path();
    if !a.allow_dirty {
        super::update::require_clean(repo, "add")?;
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
    if !piece.multiple && lock.pieces.iter().any(|p| p.id == id) {
        return usage(format!(
            "piece `{id}` is not `multiple` and is already present"
        ));
    }
    for req in &piece.requires {
        if !lock.pieces.iter().any(|p| p.id == *req) {
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
    if lock.pieces.iter().any(|p| p.id == id && p.name == name) {
        return usage(format!(
            "piece `{id}` already has an instance named `{name}`"
        ));
    }

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
    instances.push(PieceInstance {
        id: id.to_string(),
        name,
        vars,
    });
    let theirs = render_tree(&template, &input(&instances))?;

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
        for path in &plan.drift {
            report.push(path, Action::Conflict, Class::Owned);
            report.msg(format!(
                "`{path}` is owned by the template but was edited locally"
            ));
        }
        for c in plan.changes.iter().filter(|c| c.action == Action::Conflict) {
            report.push(&c.path, c.action, c.class);
            report.msg(c.note.clone().unwrap_or_default());
        }
        report.ok = false;
        report.msg("nothing was written; edit or move the files above, or pass --force");
        return Err(SpliceError::Conflict(report));
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
