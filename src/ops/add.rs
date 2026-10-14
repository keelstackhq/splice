//! `splice add` (spec §8.2): the full planner of `update` on the lock's own template commit.

use super::{
    Locked, check_requires, ensure_clean, load_locked, no_lock, parse_data, refusal, safe_segment,
    usage, validate_instance, with_messages,
};
use crate::apply::{ApplyCtx, apply};
use crate::cli::{AddArgs, GlobalArgs};
use crate::error::SpliceError;
use crate::lock::Lock;
use crate::manifest::When;
use crate::output::Report;
use crate::planner::{Disk, PlanInput, plan};
use crate::render::{PieceInstance, RenderInput, render_tree};
use crate::vars::{VarSource, instance_name, interactive, resolve_vars};
use indexmap::IndexMap;
use std::collections::BTreeSet;

type Result<T> = std::result::Result<T, SpliceError>;

pub fn run(g: &GlobalArgs, a: &AddArgs) -> Result<Report> {
    let data = parse_data(&a.data)?;
    let repo = g.dir.as_path();
    safe_segment("piece id", &a.piece_id)?;
    if let Some(n) = &a.name {
        safe_segment("instance name", n)?;
    }

    // 1. lock, dirty tree (no lock is a usage error before the dirty check can mislead),
    // template at the lock's commit
    if Lock::read(repo)?.is_none() {
        return Err(no_lock(repo));
    }
    let undo = ensure_clean(repo, a.allow_dirty, "add")?;
    let Locked {
        lock,
        warnings: lock_warnings,
        resolved,
        template,
    } = load_locked(repo)?;

    // 2. validate the new instance
    let id = &a.piece_id;
    let existing = lock.instances();
    let present: Vec<&str> = existing.iter().map(|i| i.id.as_str()).collect();
    validate_instance(&template, id, a.name.is_some(), &present)?;
    check_requires(&template, id, &present, "add it first")?;
    let piece = &template.pieces[id];

    // 3. vars: only the new piece's vars are asked for; template vars come from the lock
    if let Some(k) = data.keys().find(|k| !piece.vars.contains_key(*k)) {
        return Err(usage(format!("unknown var `{k}` in --data")));
    }
    let src = VarSource {
        data: &data,
        interactive: interactive(g),
    };
    let mut known = IndexMap::new();
    if let (true, Some(n)) = (piece.vars.contains_key("name"), &a.name) {
        known.insert("name".to_string(), n.clone());
    }
    let vars = resolve_vars(&piece.vars, &known, &src)?;
    let name = instance_name(piece, a.name.as_deref(), &vars);
    safe_segment("instance name", &name)?;
    if existing.iter().any(|i| i.id == *id && i.name == name) {
        return Err(usage(format!(
            "duplicate instance `{id}:{name}`; give it another name"
        )));
    }

    // 4. render base (lock state) and theirs (plus the new instance) at the lock commit
    let mut instances = existing.clone();
    instances.push(PieceInstance {
        id: id.clone(),
        name,
        vars,
    });
    let render = |instances: Vec<PieceInstance>| {
        render_tree(
            &template,
            &RenderInput {
                template_vars: lock.vars.clone(),
                instances,
                version: resolved.version.clone(),
            },
        )
    };
    let base = render(existing)?;
    let theirs = render(instances.clone())?;

    // 5. plan (nothing written yet), apply, lock last, post
    let mut paths: BTreeSet<&str> = BTreeSet::new();
    paths.extend(base.keys().map(String::as_str));
    paths.extend(theirs.keys().map(String::as_str));
    let disk = Disk::snapshot(repo, paths.iter().copied())?;
    let plan = plan(&PlanInput {
        base: &base,
        theirs: &theirs,
        disk: &disk,
        lock: &lock,
        removes: vec![],
        force: a.force,
    });
    let mut messages: Vec<String> = lock_warnings;
    messages.extend(template.warnings.iter().cloned());
    if let Some(e) = refusal("add", &plan, &messages) {
        return Err(e);
    }
    let result = apply(ApplyCtx {
        repo,
        plan: &plan,
        steps: vec![],
        from: resolved.version.clone(),
        to: resolved.version.clone(),
        lock: Lock::from_tree(&resolved, &lock.vars, &instances, &theirs),
        post: template.pieces[id].post.clone(),
        when: When::Add,
        no_post: a.no_post,
        json: g.json,
        undo,
    });
    with_messages(result, &messages)
}
