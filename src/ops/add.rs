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
use crate::post::run_post;
use crate::render::{PieceInstance, RenderInput, RenderedFile, RenderedTree, render_tree};
use crate::source::TemplateSource;
use crate::vars::{VarSource, instance_name, interactive, resolve_vars};

type Result<T> = std::result::Result<T, SpliceError>;

/// One step of the reduced plan. `bytes` is set for `create` and `update`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub path: String,
    pub action: Action,
    pub class: Class,
    pub bytes: Option<Vec<u8>>,
    /// Why the step is a conflict.
    pub note: Option<String>,
}

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
/// With `force`, conflicts become replacements (and deletions). Seeds are never touched.
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
            (None, Some(_)) if class == Class::Seed => {
                planned(path, Action::Create, class, Some(new.bytes.clone()))
            }
            (Some(_), _) if class == Class::Seed => planned(path, Action::Skip, class, None),
            (Some(d), _) if *d == new.bytes => planned(path, Action::Skip, class, None),
            (Some(_), None) => {
                // The file is new to the template state but exists with other content.
                let step = conflict(path, class, "already exists with different content");
                forced(step, force, new)
            }
            (Some(d), Some(_)) => {
                let locked = lock.files.get(path).map(|f| f.sha256.as_str());
                if locked == Some(sha256_hex(d).as_str()) {
                    planned(path, Action::Update, class, Some(new.bytes.clone()))
                } else {
                    forced(conflict(path, class, EDITED), force, new)
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
        plan.push(if force || locked == Some(sha256_hex(&disk).as_str()) {
            planned(path, Action::Delete, old.class, None)
        } else {
            conflict(path, old.class, EDITED)
        });
    }
    Ok(plan)
}

/// A conflict stays one unless `force` turns it into an overwrite.
fn forced(step: Planned, force: bool, new: &RenderedFile) -> Planned {
    if force {
        planned(
            &new.path,
            Action::Update,
            new.class,
            Some(new.bytes.clone()),
        )
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
    for step in &plan {
        match (&step.bytes, step.action) {
            (Some(bytes), _) => fsio::write_file(repo, &step.path, bytes)?,
            (None, Action::Delete) => std::fs::remove_file(repo.join(&step.path))?,
            _ => {}
        }
        report.push(&step.path, step.action, step.class);
        if let Some(note) = &step.note {
            report.msg(note.clone());
        }
    }
    let conflicts = plan.iter().any(|s| s.action == Action::Conflict);

    // 5. Post commands of every piece present after the add, unless something conflicted.
    if !a.no_post && !conflicts {
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
    if conflicts {
        return Err(SpliceError::Conflict(report));
    }
    Ok(report)
}
