//! `splice add` with the reduced planner (spec §8.2).

use super::{
    Locked, check_requires, check_target, load_locked, parse_data, safe_segment, usage,
    validate_instance,
};
use crate::cli::{AddArgs, GlobalArgs};
use crate::error::SpliceError;
use crate::fsio::{read_opt, sha256_hex, write_file};
use crate::lock::Lock;
use crate::manifest::When;
use crate::output::{Action, Class, Report};
use crate::post::{post_runs, run_post};
use crate::render::{PieceInstance, RenderInput, RenderedTree, render_tree};
use crate::vars::{VarSource, instance_name, interactive, resolve_vars};
use indexmap::IndexMap;
use std::path::Path;

type Result<T> = std::result::Result<T, SpliceError>;

/// One step of the plan: what to do with `path`; `bytes` is `Some` only for a write.
#[derive(Debug, Clone)]
pub struct Planned {
    pub path: String,
    pub action: Action,
    pub class: Class,
    pub bytes: Option<Vec<u8>>,
}

const EDITED: &str = "edited locally; run `splice update` after milestone 2 or merge by hand";

/// Reduced planner (spec §8.2): replace a file only when it is unedited (disk sha = lock sha) or
/// `force` and the file is owned (shared files and base-only deletions never yield to `force`); report everything else. Pure apart from reading `repo`; writes nothing. Every path it
/// might touch is checked for symlinks/directories first (exit 3).
pub fn plan_reduced(
    base: &RenderedTree,
    theirs: &RenderedTree,
    repo: &Path,
    lock: &Lock,
    force: bool,
) -> Result<Vec<Planned>> {
    let mut paths: Vec<&String> = base.keys().chain(theirs.keys()).collect();
    paths.sort();
    paths.dedup();
    let mut plan = Vec::new();
    for path in paths {
        check_target(repo, path)?;
        let cur = read_opt(repo, path)?;
        let unedited = |cur: &[u8]| {
            lock.files
                .get(path.as_str())
                .is_some_and(|l| l.sha256 == sha256_hex(cur))
        };
        let mut push = |action, class, bytes| {
            plan.push(Planned {
                path: path.clone(),
                action,
                class,
                bytes,
            })
        };
        match (base.get(path), theirs.get(path)) {
            (None, Some(t)) => match cur {
                None => push(Action::Create, t.class, Some(t.bytes.clone())),
                Some(_) if t.class == Class::Seed => push(Action::Skip, t.class, None),
                Some(c) if c == t.bytes => {}
                Some(_) if force && t.class == Class::Owned => {
                    push(Action::Update, t.class, Some(t.bytes.clone()))
                }
                Some(_) => push(Action::Conflict, t.class, None),
            },
            (Some(b), Some(t)) => {
                if t.class == Class::Seed {
                    if b.bytes != t.bytes {
                        push(Action::Skip, t.class, None);
                    }
                    continue;
                }
                if b.bytes == t.bytes {
                    continue;
                }
                match cur {
                    None if t.class == Class::Shared => push(Action::Skip, t.class, None),
                    None if force && t.class == Class::Owned => {
                        push(Action::Create, t.class, Some(t.bytes.clone()))
                    }
                    None => push(Action::Conflict, t.class, None),
                    Some(c) if c == t.bytes => {}
                    Some(c) if unedited(&c) || (force && t.class == Class::Owned) => {
                        push(Action::Update, t.class, Some(t.bytes.clone()))
                    }
                    Some(_) => push(Action::Conflict, t.class, None),
                }
            }
            (Some(b), None) => {
                if b.class == Class::Seed {
                    continue;
                }
                match cur {
                    None => {}
                    Some(c) if unedited(&c) => push(Action::Delete, b.class, None),
                    // kept, and unmanaged from now on: the new lock no longer lists it
                    Some(_) => push(Action::Skip, b.class, None),
                }
            }
            (None, None) => unreachable!("path comes from base or theirs"),
        }
    }
    Ok(plan)
}

pub fn run(g: &GlobalArgs, a: &AddArgs) -> Result<Report> {
    let data = parse_data(&a.data)?;
    let repo = g.dir.as_path();
    safe_segment("piece id", &a.piece_id)?;
    if let Some(n) = &a.name {
        safe_segment("instance name", n)?;
    }

    // 1. lock, template at the lock's commit
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
    let plan = plan_reduced(&base, &theirs, repo, &lock, a.force)?;
    let mut report = Report::new("add");
    for w in lock_warnings.iter().chain(&template.warnings) {
        report.msg(w.clone());
    }
    let mut conflicts = false;
    for p in &plan {
        match (&p.action, &p.bytes) {
            (_, Some(bytes)) => write_file(repo, &p.path, bytes)?,
            (Action::Delete, None) => std::fs::remove_file(repo.join(&p.path))?,
            (Action::Conflict, None) => {
                conflicts = true;
                report.msg(format!("{}: {EDITED}", p.path));
            }
            (Action::Skip, None) if !theirs.contains_key(&p.path) => report.msg(format!(
                "{}: edited locally and no longer rendered; kept, but no longer managed by splice",
                p.path
            )),
            _ => {}
        }
        report.push(p.path.clone(), p.action, p.class);
    }
    Lock::from_tree(&resolved, &lock.vars, &instances, &theirs)
        .write_atomic(repo)
        .map_err(|e| e.with_report(report.clone()))?;
    if conflicts {
        let cmds = post_runs(&template.pieces[id].post, When::Add);
        if !a.no_post && !cmds.is_empty() {
            let list = cmds
                .iter()
                .map(|c| format!("`{c}`"))
                .collect::<Vec<_>>()
                .join(", ");
            report.msg(format!(
                "post commands skipped because of conflicts; run them by hand after resolving: {list}"
            ));
        }
        return Err(SpliceError::Conflict(report));
    }
    if !a.no_post {
        run_post(repo, &template.pieces[id].post, When::Add, g.json)
            .map_err(|e| e.with_report(report.clone()))?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::{LockFile, LockTemplate};
    use crate::manifest::MergeStrategy;
    use crate::render::RenderedFile;

    fn file(path: &str, bytes: &str, class: Class) -> RenderedFile {
        RenderedFile {
            path: path.into(),
            bytes: bytes.as_bytes().to_vec(),
            class,
            strategy: MergeStrategy::Text,
            piece_id: "base".into(),
            piece_name: "base".into(),
            glob: None,
        }
    }

    fn tree(files: &[RenderedFile]) -> RenderedTree {
        files.iter().map(|f| (f.path.clone(), f.clone())).collect()
    }

    fn lock_for(base: &RenderedTree) -> Lock {
        Lock {
            format: 1,
            splice: semver::Version::new(0, 1, 0),
            template: LockTemplate {
                source: "x".into(),
                version: semver::Version::new(1, 0, 0),
                commit: "0".repeat(40),
            },
            vars: IndexMap::new(),
            pieces: vec![],
            files: base
                .iter()
                .map(|(p, f)| {
                    (
                        p.clone(),
                        LockFile {
                            class: f.class,
                            sha256: sha256_hex(&f.bytes),
                        },
                    )
                })
                .collect(),
        }
    }

    fn actions(plan: &[Planned]) -> Vec<(String, Action)> {
        plan.iter().map(|p| (p.path.clone(), p.action)).collect()
    }

    #[test]
    fn base_only_unedited_is_deleted() {
        let d = tempfile::tempdir().unwrap();
        let base = tree(&[file("gone.txt", "a\n", Class::Owned)]);
        std::fs::write(d.path().join("gone.txt"), "a\n").unwrap();
        let plan = plan_reduced(&base, &tree(&[]), d.path(), &lock_for(&base), false).unwrap();
        assert_eq!(actions(&plan), [("gone.txt".into(), Action::Delete)]);
    }

    #[test]
    fn base_only_edited_is_kept_even_with_force() {
        let d = tempfile::tempdir().unwrap();
        let base = tree(&[file("gone.txt", "a\n", Class::Owned)]);
        std::fs::write(d.path().join("gone.txt"), "mine\n").unwrap();
        for force in [false, true] {
            let plan = plan_reduced(&base, &tree(&[]), d.path(), &lock_for(&base), force).unwrap();
            assert_eq!(actions(&plan), [("gone.txt".into(), Action::Skip)]);
            assert!(plan[0].bytes.is_none());
        }
    }

    #[test]
    fn force_does_not_overwrite_user_created_shared_file() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("new.txt"), "mine\n").unwrap();
        let theirs = tree(&[file("new.txt", "t\n", Class::Shared)]);
        let lock = lock_for(&tree(&[]));
        let plan = plan_reduced(&tree(&[]), &theirs, d.path(), &lock, true).unwrap();
        assert_eq!(actions(&plan), [("new.txt".into(), Action::Conflict)]);
        let owned = tree(&[file("new.txt", "t\n", Class::Owned)]);
        let plan = plan_reduced(&tree(&[]), &owned, d.path(), &lock, true).unwrap();
        assert_eq!(actions(&plan), [("new.txt".into(), Action::Update)]);
    }
}
