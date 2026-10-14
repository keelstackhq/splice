//! `splice new` (spec §8.1).

use crate::cli::{GlobalArgs, NewArgs};
use crate::error::SpliceError;
use crate::fetch::Fetcher;
use crate::fsio::write_file;
use crate::git::Git;
use crate::lock::Lock;
use crate::manifest::{PostCmd, Template, When};
use crate::output::{Action, Class, Report};
use crate::post::run_post;
use crate::render::{PieceInstance, RenderInput, render_tree};
use crate::source::TemplateSource;
use crate::vars::{VarSource, instance_name, interactive, resolve_vars};
use indexmap::IndexMap;
use std::path::Path;

use super::{check_requires, check_target, parse_data, safe_segment, usage, validate_instance};

type Result<T> = std::result::Result<T, SpliceError>;

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
        Err(e) => Err(e.into()),
    }
}

pub fn run(g: &GlobalArgs, a: &NewArgs) -> Result<Report> {
    let data = parse_data(&a.data)?;
    let flags = parse_piece_flags(&a.pieces)?;
    let source = TemplateSource::parse_from(&a.source, &g.dir)?;
    let dest = g.dir.join(&a.dest);

    // 1. resolve, load, validate, min_splice
    let mut resolved = Fetcher::from_env().resolve(&source, a.git_ref.as_deref(), a.pre)?;
    // the lock must resolve a relative local source from the new repo's root
    resolved.source.spec = source.spec_for_dest(&dest);
    let template = Template::load(&resolved.root)?;
    template.check_min_splice(
        &semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version is semver"),
    )?;

    // 2. destination
    check_dest(&dest, a.force)?;

    // 3. instances: base, then each --piece in order
    let mut wanted: Vec<(String, Option<String>)> = vec![(template.manifest.base.clone(), None)];
    for (id, name) in flags {
        let present: Vec<&str> = wanted.iter().map(|(i, _)| i.as_str()).collect();
        validate_instance(&template, &id, name.is_some(), &present)?;
        wanted.push((id, name));
    }
    let present: Vec<&str> = wanted.iter().map(|(i, _)| i.as_str()).collect();
    for id in &present {
        check_requires(&template, id, &present, "add --piece {r}")?;
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
    // from here on files are on disk: a failure must still report them
    let finish = || -> Result<()> {
        Lock::from_tree(&resolved, &template_vars, &instances, &tree).write_atomic(&dest)?;
        if !Git::is_inside_work_tree(&dest)? {
            Git::init(&dest)?;
        }
        if !a.no_post {
            let cmds: Vec<PostCmd> = instances
                .iter()
                .flat_map(|i| template.pieces[&i.id].post.clone())
                .collect();
            run_post(&dest, &cmds, When::New)?;
        }
        Ok(())
    };
    match finish() {
        Ok(()) => Ok(report),
        Err(e) => Err(e.with_report(report)),
    }
}
