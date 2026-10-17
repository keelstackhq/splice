//! `splice add` (spec §8.2) with the milestone 1 reduced planner.

use std::path::Path;

use indexmap::IndexMap;

use crate::cli::{AddArgs, GlobalArgs};
use crate::error::SpliceError;
use crate::fetch::Fetcher;
use crate::fsio::{self, sha256_hex};
use crate::lock::{LOCK_PATH, Lock};
use crate::manifest::{PostCmd, Template, When};
use crate::output::{Action, Class, Report};
use crate::planner::Planned;
use crate::post::run_post;
use crate::render::{PieceInstance, RenderInput, RenderedFile, RenderedTree, render_tree};
use crate::source::TemplateSource;
use crate::vars::{VarSource, instance_name, interactive, resolve_vars};

type Result<T> = std::result::Result<T, SpliceError>;

fn planned(path: &str, action: Action, class: Class, bytes: Option<Vec<u8>>) -> Planned {
    Planned {
        path: path.to_string(),
        action,
        class,
        bytes,
        note: None,
    }
}

fn conflict(path: &str, class: Class, why: &str) -> Planned {
    Planned {
        note: Some(format!("`{path}` {why}")),
        ..planned(path, Action::Conflict, class, None)
    }
}

const EDITED: &str = "edited locally; run `splice update` after milestone 2 or merge by hand";

/// Reduced planner (spec §8.2, milestone 1). `base` is the render of the lock state, `theirs` the
/// render with the new instance; `repo` is read, never written.
///
/// - A path only in `theirs`: created when absent (a seed only when absent); an existing file with
///   the same bytes is skipped, one with other content is a conflict (a seed is kept).
/// - A path in both with changed bytes: an owned or shared file is replaced when the disk still
///   has the locked content, else it is a conflict; a seed is skipped.
/// - A path only in `base` is deleted when the disk still has the locked content, else conflict.
///
/// With `force`, owned conflicts become replacements; edited shared files and edited files that are
/// no longer rendered are kept (spec §8.4).
/// A seed or shared file deleted locally stays deleted. Seeds are never touched.
pub fn plan_reduced(
    base: &RenderedTree,
    theirs: &RenderedTree,
    repo: &Path,
    lock: &Lock,
    force: bool,
) -> Result<Vec<Planned>> {
    let mut plan = Vec::new();
    for (path, new) in theirs {
        let old = base.get(path);
        if old.is_some_and(|o| o.bytes == new.bytes) {
            continue;
        }
        fsio::check_target(repo, path)?;
        let disk = fsio::read_opt(repo, path)?;
        let class = new.class;
        let step = match (&disk, old) {
            (None, None) => planned(path, Action::Create, class, Some(new.bytes.clone())),
            // A seed or shared file the user deleted stays deleted (spec §8.4).
            (None, Some(_)) if class != Class::Owned => planned(path, Action::Skip, class, None),
            (Some(_), _) if class == Class::Seed => planned(path, Action::Skip, class, None),
            (Some(d), _) if *d == new.bytes => planned(path, Action::Skip, class, None),
            (Some(_), None) => {
                // The file is new to the template state but exists with other content.
                let step = conflict(path, class, "already exists with different content");
                forced(step, force, new, false)
            }
            (Some(d), Some(_)) => {
                let locked = lock.files.get(path).map(|f| f.sha256.as_str());
                if locked == Some(sha256_hex(d).as_str()) {
                    planned(path, Action::Update, class, Some(new.bytes.clone()))
                } else {
                    forced(conflict(path, class, EDITED), force, new, false)
                }
            }
            (None, Some(_)) => forced(
                conflict(
                    path,
                    class,
                    "was deleted locally; restore it or merge by hand",
                ),
                force,
                new,
                true,
            ),
        };
        plan.push(step);
    }
    for (path, old) in base {
        if theirs.contains_key(path) || old.class == Class::Seed {
            continue;
        }
        fsio::check_target(repo, path)?;
        let Some(disk) = fsio::read_opt(repo, path)? else {
            continue;
        };
        let locked = lock.files.get(path).map(|f| f.sha256.as_str());
        plan.push(if locked == Some(sha256_hex(&disk).as_str()) {
            planned(path, Action::Delete, old.class, None)
        } else if force {
            // `--force` never deletes user edits: keep the file and say so (spec §8.4).
            Planned {
                note: Some(format!(
                    "`{path}` is no longer rendered but was edited locally; kept (the lock records the template's version)"
                )),
                ..planned(path, Action::Skip, old.class, None)
            }
        } else {
            conflict(path, old.class, EDITED)
        });
    }
    Ok(plan)
}

/// A conflict stays one unless `force` turns it into an overwrite.
/// An edited shared file is never replaced (spec §8.4: `--force` replaces owned drift only).
fn forced(step: Planned, force: bool, new: &RenderedFile, absent: bool) -> Planned {
    if force && new.class == Class::Shared && !absent {
        Planned {
            note: Some(format!(
                "`{}` kept, merge by hand; the lock records the template's version",
                new.path
            )),
            ..planned(&new.path, Action::Skip, new.class, None)
        }
    } else if force {
        let action = if absent {
            Action::Create
        } else {
            Action::Update
        };
        planned(&new.path, action, new.class, Some(new.bytes.clone()))
    } else {
        step
    }
}

fn usage<T>(msg: String) -> Result<T> {
    Err(SpliceError::Usage(msg))
}

fn parse_data(flags: &[String]) -> Result<IndexMap<String, String>> {
    let mut data = IndexMap::new();
    for flag in flags {
        match flag.split_once('=') {
            Some((k, v)) if !k.is_empty() => {
                data.insert(k.to_string(), v.to_string());
            }
            _ => return usage(format!("--data expects K=V, got `{flag}`")),
        }
    }
    Ok(data)
}

pub fn run(g: &GlobalArgs, a: &AddArgs) -> Result<Report> {
    let mut report = Report::new("add");
    let repo = g.dir.as_path();

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
    let data = parse_data(&a.data)?;
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

    // 4. Plan (checks every target), then apply; the lock is the last write.
    let plan = plan_reduced(&base, &theirs, repo, &lock, a.force)?;
    // Any conflict (only possible without `--force`): write nothing, no files and no lock (C3).
    if plan.iter().any(|s| s.action == Action::Conflict) {
        for step in plan.iter().filter(|s| s.action == Action::Conflict) {
            report.push(&step.path, step.action, step.class);
            report.msg(step.note.clone().unwrap_or_default());
        }
        report.msg("nothing was written; edit or move the files above, or pass --force");
        return Err(SpliceError::Conflict(report));
    }
    for step in &plan {
        match (&step.bytes, step.action) {
            (Some(bytes), _) => fsio::write_file(repo, &step.path, bytes)?,
            (None, Action::Delete) => fsio::remove_file(repo, &step.path)?,
            _ => {}
        }
        report.push(&step.path, step.action, step.class);
        if let Some(note) = &step.note {
            report.msg(note.clone());
        }
    }

    // 5. Post commands of every piece present after the add, (a conflict returned earlier).
    if !a.no_post {
        let mut cmds: Vec<PostCmd> = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for inst in &instances {
            if !seen.contains(&inst.id.as_str()) {
                seen.push(&inst.id);
                cmds.extend(template.pieces[&inst.id].post.iter().cloned());
            }
        }
        run_post(repo, &cmds, When::Add, g.json).map_err(|e| match e {
            SpliceError::Env(m) => SpliceError::Env(format!(
                "{m}; files were written but {LOCK_PATH} was not updated: fix the command and \
                 rerun `splice add {id} …`"
            )),
            other => other,
        })?;
    }

    Lock::from_tree(&resolved, &lock.vars, &instances, &theirs).write_atomic(repo)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::{LockFile, LockTemplate};
    use crate::manifest::MergeStrategy;

    fn file(path: &str, bytes: &str, class: Class) -> RenderedFile {
        RenderedFile {
            path: path.into(),
            bytes: bytes.as_bytes().to_vec(),
            class,
            strategy: MergeStrategy::Text,
            piece_id: "p".into(),
            piece_name: "p".into(),
            glob: None,
        }
    }

    fn tree(files: &[RenderedFile]) -> RenderedTree {
        files.iter().map(|f| (f.path.clone(), f.clone())).collect()
    }

    /// A lock that lists every non-seed file of `base`.
    fn lock_of(base: &RenderedTree) -> Lock {
        Lock {
            format: 1,
            splice: semver::Version::new(0, 1, 0),
            template: LockTemplate {
                source: "x".into(),
                version: semver::Version::new(1, 0, 0),
                commit: "0".repeat(40),
            },
            vars: IndexMap::new(),
            pieces: Vec::new(),
            files: base
                .values()
                .filter(|f| f.class != Class::Seed)
                .map(|f| {
                    (
                        f.path.clone(),
                        LockFile {
                            class: f.class,
                            sha256: sha256_hex(&f.bytes),
                        },
                    )
                })
                .collect(),
        }
    }

    fn plan(
        base: &[RenderedFile],
        theirs: &[RenderedFile],
        disk: &[(&str, &str)],
        force: bool,
    ) -> Vec<Planned> {
        let dir = tempfile::tempdir().unwrap();
        for (p, c) in disk {
            std::fs::write(dir.path().join(p), c).unwrap();
        }
        let base = tree(base);
        plan_reduced(&base, &tree(theirs), dir.path(), &lock_of(&base), force).unwrap()
    }

    #[test]
    fn deleted_seed_stays_deleted() {
        for force in [false, true] {
            let p = plan(
                &[file("s.txt", "1", Class::Seed)],
                &[file("s.txt", "2", Class::Seed)],
                &[],
                force,
            );
            assert_eq!(p.len(), 1);
            assert_eq!(p[0].action, Action::Skip);
            assert!(p[0].bytes.is_none());
        }
    }

    #[test]
    fn deleted_shared_stays_deleted_deleted_owned_is_drift() {
        let shared = plan(
            &[file("a", "1", Class::Shared)],
            &[file("a", "2", Class::Shared)],
            &[],
            false,
        );
        assert_eq!(shared[0].action, Action::Skip);
        let owned = plan(
            &[file("a", "1", Class::Owned)],
            &[file("a", "2", Class::Owned)],
            &[],
            false,
        );
        assert_eq!(owned[0].action, Action::Conflict);
        // Forced recreation is a create, not an update.
        let forced = plan(
            &[file("a", "1", Class::Owned)],
            &[file("a", "2", Class::Owned)],
            &[],
            true,
        );
        assert_eq!(forced[0].action, Action::Create);
    }

    #[test]
    fn force_keeps_edited_shared_files_but_replaces_owned() {
        let base = [file("a", "1", Class::Shared)];
        let theirs = [file("a", "2", Class::Shared)];
        let kept = plan(&base, &theirs, &[("a", "mine")], true);
        assert_eq!(kept[0].action, Action::Skip);
        assert!(kept[0].bytes.is_none());
        assert!(
            kept[0]
                .note
                .as_deref()
                .unwrap()
                .contains("kept, merge by hand")
        );
        // A user-made file at a new shared path is kept too.
        let new = plan(&[], &theirs, &[("a", "mine")], true);
        assert_eq!(new[0].action, Action::Skip);
        // Owned drift is replaced under force (spec §8.4).
        let owned = plan(
            &[file("a", "1", Class::Owned)],
            &[file("a", "2", Class::Owned)],
            &[("a", "mine")],
            true,
        );
        assert_eq!(owned[0].action, Action::Update);
    }

    #[test]
    fn force_keeps_edited_files_that_are_no_longer_rendered() {
        let base = [file("gone", "1", Class::Owned)];
        let edited = plan(&base, &[], &[("gone", "mine")], true);
        assert_eq!(edited.len(), 1);
        assert_eq!(edited[0].action, Action::Skip);
        assert!(edited[0].note.as_deref().unwrap().contains("kept"));
        // Without force it is a conflict; unedited files are deleted either way.
        assert_eq!(
            plan(&base, &[], &[("gone", "mine")], false)[0].action,
            Action::Conflict
        );
        for force in [false, true] {
            assert_eq!(
                plan(&base, &[], &[("gone", "1")], force)[0].action,
                Action::Delete
            );
        }
    }
}
