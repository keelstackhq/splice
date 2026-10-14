//! `splice new` (spec §8.1).

use crate::cli::{GlobalArgs, NewArgs};
use crate::error::SpliceError;
use crate::fetch::Fetcher;
use crate::fsio::write_file;
use crate::git::Git;
use crate::lock::Lock;
use crate::manifest::{PostCmd, Template, When};
use crate::output::{Action, Class, Report};
use crate::paths::validate_segment;
use crate::post::run_post;
use crate::render::{PieceInstance, RenderInput, render_tree};
use crate::source::TemplateSource;
use crate::vars::{VarSource, instance_name, interactive, resolve_vars};
use indexmap::IndexMap;
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, SpliceError>;

fn usage(msg: impl Into<String>) -> SpliceError {
    SpliceError::Usage(msg.into())
}

/// `K=V` flags into a map; the first `=` splits, later flags win.
fn parse_data(raw: &[String]) -> Result<IndexMap<String, String>> {
    let mut m = IndexMap::new();
    for item in raw {
        let (k, v) = item
            .split_once('=')
            .filter(|(k, _)| !k.is_empty())
            .ok_or_else(|| usage(format!("invalid --data `{item}`: expected K=V")))?;
        m.insert(k.to_string(), v.to_string());
    }
    Ok(m)
}

/// A name that becomes a path segment must be safe: a bad value is a usage error (exit 2).
fn safe_segment(what: &str, v: &str) -> Result<()> {
    validate_segment(v).map_err(|e| usage(format!("{what} {v:?} is not a safe name: {e}")))
}

/// `--piece ID[:NAME]` into `(id, explicit name)`.
fn parse_piece_flags(raw: &[String]) -> Result<Vec<(String, Option<String>)>> {
    raw.iter()
        .map(|p| {
            let (id, name) = match p.split_once(':') {
                Some((i, n)) => (i, Some(n)),
                None => (p.as_str(), None),
            };
            safe_segment("piece id", id)?;
            if let Some(n) = name {
                safe_segment("piece name", n)?;
            }
            Ok((id.to_string(), name.map(str::to_string)))
        })
        .collect()
}

/// Dest must be a directory (or absent) holding at most `.git`, unless `--force`; any lock at
/// all is refused, and `.splice` must not be a regular file or symlink.
fn check_dest(dest: &Path, force: bool) -> Result<()> {
    if dest.exists() && !dest.is_dir() {
        return Err(usage(format!("{} is not a directory", dest.display())));
    }
    match std::fs::symlink_metadata(dest.join(".splice")) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => {
            return Err(usage(format!(
                "{}: `.splice` exists but is not a directory (file or symlink); remove or rename it",
                dest.display()
            )));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    if std::fs::symlink_metadata(dest.join(".splice/lock.toml")).is_ok() {
        return Err(usage(format!(
            "{} already has .splice/lock.toml; use `splice update` or `splice add`",
            dest.display()
        )));
    }
    match std::fs::read_dir(dest) {
        Ok(entries) => {
            if !dest.is_dir() {
                return Err(usage(format!("{} is not a directory", dest.display())));
            }
            let busy = entries
                .filter_map(|e| e.ok())
                .any(|e| e.file_name() != ".git");
            if busy && !force {
                return Err(usage(format!(
                    "{} is not empty; pass --force to overwrite owned and shared files (seeds are kept)",
                    dest.display()
                )));
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) if dest.is_file() => Err(usage(format!(
            "{} is not a directory ({e})",
            dest.display()
        ))),
        Err(e) => Err(e.into()),
    }
}

/// Refuse to write through a symlink or over a directory below `dest` (checked before any write).
fn unsafe_path(msg: String) -> SpliceError {
    SpliceError::Template(format!("unsafe path: {msg}"))
}

fn check_target(dest: &Path, rel: &str) -> Result<PathBuf> {
    let mut p = dest.to_path_buf();
    let segs: Vec<&str> = rel.split('/').collect();
    for (i, seg) in segs.iter().enumerate() {
        p.push(seg);
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(unsafe_path(format!(
                    "refusing to write through symlink {}",
                    p.display()
                )));
            }
            Ok(m) if i + 1 == segs.len() && m.is_dir() => {
                return Err(unsafe_path(format!("{rel} exists and is a directory")));
            }
            Ok(m) if i + 1 < segs.len() && !m.is_dir() => {
                return Err(unsafe_path(format!(
                    "{} exists and is not a directory",
                    p.display()
                )));
            }
            _ => {}
        }
    }
    Ok(p)
}

pub fn run(g: &GlobalArgs, a: &NewArgs) -> Result<Report> {
    let data = parse_data(&a.data)?;
    let flags = parse_piece_flags(&a.pieces)?;
    let source = TemplateSource::parse(&a.source)?;
    let dest = g.dir.join(&a.dest);

    // 1. resolve, load, validate, min_splice
    let resolved = Fetcher::from_env().resolve(&source, a.git_ref.as_deref(), a.pre)?;
    let template = Template::load(&resolved.root)?;
    template.check_min_splice(
        &semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version is semver"),
    )?;

    // 2. destination
    check_dest(&dest, a.force)?;

    // 3. instances: base, then each --piece in order
    let mut wanted: Vec<(String, Option<String>)> = vec![(template.manifest.base.clone(), None)];
    for (id, name) in flags {
        let piece = template
            .pieces
            .get(&id)
            .ok_or_else(|| usage(format!("unknown piece `{id}`")))?;
        if id == template.manifest.base {
            return Err(usage(format!(
                "`{id}` is the base piece and is always included"
            )));
        }
        if name.is_some() && !piece.multiple {
            return Err(usage(format!(
                "piece `{id}` is not `multiple`; it cannot be named"
            )));
        }
        if !piece.multiple && wanted.iter().any(|(i, _)| *i == id) {
            return Err(usage(format!(
                "piece `{id}` is not `multiple` and was given twice"
            )));
        }
        wanted.push((id, name));
    }
    for (id, _) in &wanted {
        for r in &template.pieces[id].requires {
            if !wanted.iter().any(|(i, _)| i == r) {
                return Err(usage(format!(
                    "piece `{id}` requires piece `{r}`; add --piece {r}"
                )));
            }
        }
    }

    // 4. vars: template vars, then each instance's piece vars
    let known_keys: Vec<&String> = template
        .manifest
        .vars
        .keys()
        .chain(
            wanted
                .iter()
                .flat_map(|(id, _)| template.pieces[id].vars.keys()),
        )
        .collect();
    if let Some(k) = data.keys().find(|k| !known_keys.contains(k)) {
        return Err(usage(format!("unknown var `{k}` in --data")));
    }
    let src = VarSource {
        data: &data,
        interactive: interactive(g),
    };
    let template_vars = resolve_vars(&template.manifest.vars, &IndexMap::new(), &src)?;
    let mut instances: Vec<PieceInstance> = Vec::new();
    for (id, explicit) in &wanted {
        let piece = &template.pieces[id];
        let mut known = IndexMap::new();
        if let (true, Some(n)) = (piece.multiple && piece.vars.contains_key("name"), explicit) {
            known.insert("name".to_string(), n.clone());
        }
        let vars = resolve_vars(&piece.vars, &known, &src)?;
        let name = if id == &template.manifest.base {
            id.clone()
        } else {
            instance_name(piece, explicit.as_deref(), &vars)
        };
        safe_segment("instance name", &name)?;
        if instances.iter().any(|i| i.id == *id && i.name == name) {
            return Err(usage(format!(
                "duplicate instance `{id}:{name}`; give it another name"
            )));
        }
        instances.push(PieceInstance {
            id: id.clone(),
            name,
            vars,
        });
    }

    // 5. render, check targets, write, lock, git init, post
    let tree = render_tree(
        &template,
        &RenderInput {
            template_vars: template_vars.clone(),
            instances: instances.clone(),
            version: resolved.version.clone(),
        },
    )?;
    let mut targets = Vec::new();
    for (path, f) in &tree {
        targets.push((f, check_target(&dest, path)?));
    }
    let mut report = Report::new("new");
    for w in &template.warnings {
        report.msg(w.clone());
    }
    std::fs::create_dir_all(&dest)?;
    for (f, target) in targets {
        let existed = std::fs::symlink_metadata(&target).is_ok();
        if f.class == Class::Seed && existed {
            report.push(f.path.clone(), Action::Skip, f.class);
            continue;
        }
        if existed && std::fs::read(&target).is_ok_and(|b| b == f.bytes) {
            continue;
        }
        write_file(&dest, &f.path, &f.bytes)?;
        let action = if existed {
            Action::Update
        } else {
            Action::Create
        };
        report.push(f.path.clone(), action, f.class);
    }
    Lock::from_tree(&resolved, &template_vars, &instances, &tree).write_atomic(&dest)?;
    if !Git::is_inside_work_tree(&dest)? {
        Git::init(&dest)?;
    }
    if !a.no_post {
        let cmds: Vec<PostCmd> = instances
            .iter()
            .flat_map(|i| template.pieces[&i.id].post.clone())
            .collect();
        run_post(&dest, &cmds, When::New, g.json)?;
    }
    Ok(report)
}
