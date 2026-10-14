//! `splice add` with the reduced planner (spec §8.2).

use super::{check_target, parse_data, safe_segment, usage};
use crate::cli::{AddArgs, GlobalArgs};
use crate::error::SpliceError;
use crate::fetch::Fetcher;
use crate::fsio::{sha256_hex, write_file};
use crate::lock::Lock;
use crate::manifest::{Template, When};
use crate::output::{Action, Class, Report};
use crate::post::run_post;
use crate::render::{PieceInstance, RenderInput, RenderedTree, render_tree};
use crate::source::TemplateSource;
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

/// Disk state of one path: `None` when absent.
fn disk(repo: &Path, rel: &str) -> Result<Option<Vec<u8>>> {
    match std::fs::read(repo.join(rel)) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Reduced planner (spec §8.2): replace a file only when it is unedited (disk sha = lock sha) or
/// `force`; report everything else. Pure apart from reading `repo`; writes nothing. Every path it
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
        let cur = disk(repo, path)?;
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
                Some(_) if force => push(Action::Update, t.class, Some(t.bytes.clone())),
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
                    None if force => push(Action::Create, t.class, Some(t.bytes.clone())),
                    None => push(Action::Conflict, t.class, None),
                    Some(c) if c == t.bytes => {}
                    Some(c) if force || unedited(&c) => {
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
                    Some(c) if force || unedited(&c) => push(Action::Delete, b.class, None),
                    Some(_) => push(Action::Conflict, b.class, None),
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
    let (lock, lock_warnings) = Lock::read_with_warnings(repo)?.ok_or_else(|| {
        usage(format!(
            "{} has no .splice/lock.toml; use `splice new` first",
            repo.display()
        ))
    })?;
    let source = TemplateSource::parse(&lock.template.source)?;
    let resolved = Fetcher::from_env().checkout(
        &source,
        &lock.template.commit,
        lock.template.version.clone(),
    )?;
    let template = Template::load(&resolved.root)?;
    template.check_min_splice(
        &semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version is semver"),
    )?;

    // 2. validate the new instance
    let id = &a.piece_id;
    let piece = template
        .pieces
        .get(id)
        .ok_or_else(|| usage(format!("unknown piece `{id}`")))?;
    if *id == template.manifest.base {
        return Err(usage(format!(
            "`{id}` is the base piece and is always included"
        )));
    }
    if a.name.is_some() && !piece.multiple {
        return Err(usage(format!(
            "piece `{id}` is not `multiple`; it cannot be named"
        )));
    }
    let existing = lock.instances();
    if !piece.multiple && existing.iter().any(|i| i.id == *id) {
        return Err(usage(format!(
            "piece `{id}` is not `multiple` and is already present"
        )));
    }
    for r in &piece.requires {
        if !existing.iter().any(|i| i.id == *r) {
            return Err(usage(format!(
                "piece `{id}` requires piece `{r}`; add it first"
            )));
        }
    }

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
            _ => {}
        }
        report.push(p.path.clone(), p.action, p.class);
    }
    Lock::from_tree(&resolved, &lock.vars, &instances, &theirs).write_atomic(repo)?;
    if conflicts {
        return Err(SpliceError::Conflict(report));
    }
    if !a.no_post {
        run_post(repo, &template.pieces[id].post, When::Add, g.json)?;
    }
    Ok(report)
}
