//! `splice new` (spec §8.1).

use std::path::{Path, PathBuf};

use indexmap::IndexMap;

use crate::cli::{GlobalArgs, NewArgs};
use crate::error::SpliceError;
use crate::fetch::Fetcher;
use crate::fsio;
use crate::git::Git;
use crate::lock::{LOCK_PATH, Lock};
use crate::manifest::{PostCmd, Template, When};
use crate::output::{Action, Class, Report};
use crate::post::run_post;
use crate::render::{PieceInstance, RenderInput, render_tree};
use crate::source::TemplateSource;
use crate::vars::{VarSource, instance_name, interactive, resolve_vars};

type Result<T> = std::result::Result<T, SpliceError>;

fn usage<T>(msg: String) -> Result<T> {
    Err(SpliceError::Usage(msg))
}

/// `K=V` flags; a later flag for the same key wins.
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

/// `base` first, then each `--piece ID[:NAME]`; checks ids, `multiple` and `requires`.
fn select_pieces(t: &Template, specs: &[String]) -> Result<Vec<(String, Option<String>)>> {
    let mut picked: Vec<(String, Option<String>)> = vec![(t.manifest.base.clone(), None)];
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

/// Spec §8.1 step 2: no lock yet, and the destination is empty (apart from `.git`) unless forced.
fn check_dest(dest: &Path, force: bool) -> Result<()> {
    if dest.join(LOCK_PATH).exists() {
        return usage(format!(
            "{} already has a {LOCK_PATH}; use `splice update` or `splice add`",
            dest.display()
        ));
    }
    if !dest.exists() {
        return Ok(());
    }
    if !dest.is_dir() {
        return usage(format!("{} is not a directory", dest.display()));
    }
    let mut entries = std::fs::read_dir(dest)?;
    let occupied = entries.try_fold(false, |acc, e| e.map(|e| acc || e.file_name() != ".git"))?;
    if occupied && !force {
        return usage(format!(
            "{} is not empty; pass --force to write into it (seed files are kept)",
            dest.display()
        ));
    }
    Ok(())
}

pub fn run(g: &GlobalArgs, a: &NewArgs) -> Result<Report> {
    let mut report = Report::new("new");

    // 1. Resolve, load, validate, check min_splice.
    let src = TemplateSource::parse(&a.source)?;
    let resolved = Fetcher::from_env().resolve(&src, a.git_ref.as_deref(), a.pre)?;
    let template = Template::load(&resolved.root)?;
    let running = semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version");
    template.check_min_splice(&running)?;
    report.messages.extend(template.warnings.iter().cloned());

    // 2. Destination.
    let dest: PathBuf = g.dir.join(&a.dest);
    check_dest(&dest, a.force)?;

    // 3. Instances, then 4. variables.
    let picked = select_pieces(&template, &a.pieces)?;
    let data = parse_data(&a.data)?;
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
        data: &data,
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
        if instances.iter().any(|i| i.id == *id && i.name == name) {
            return usage(format!("piece `{id}` has two instances named `{name}`"));
        }
        instances.push(PieceInstance {
            id: id.clone(),
            name,
            vars,
        });
    }

    // 5. Render everything before touching the destination.
    let tree = render_tree(
        &template,
        &RenderInput {
            template_vars: template_vars.clone(),
            instances: instances.clone(),
            version: resolved.version.clone(),
        },
    )?;

    // Write every file; seeds only when absent.
    std::fs::create_dir_all(&dest)?;
    for file in tree.values() {
        let exists = dest.join(&file.path).symlink_metadata().is_ok();
        if file.class == Class::Seed && exists {
            report.push(&file.path, Action::Skip, file.class);
            continue;
        }
        fsio::write_file(&dest, &file.path, &file.bytes)?;
        let action = if exists {
            Action::Update
        } else {
            Action::Create
        };
        report.push(&file.path, action, file.class);
    }

    if Git::is_clean(&dest)?.is_none() {
        Git::init(&dest)?;
    }

    // Post commands run before the lock exists; the lock is the very last write, so a failed
    // post leaves a destination that `new --force` can redo.
    if !a.no_post {
        let mut cmds: Vec<PostCmd> = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for inst in &instances {
            if !seen.contains(&inst.id.as_str()) {
                seen.push(&inst.id);
                cmds.extend(template.pieces[&inst.id].post.iter().cloned());
            }
        }
        run_post(&dest, &cmds, When::New, g.json).map_err(|e| match e {
            SpliceError::Env(m) => SpliceError::Env(format!(
                "{m}; files were written but no {LOCK_PATH} yet: fix the command and rerun \
                 `splice new … --force`"
            )),
            other => other,
        })?;
    }

    Lock::from_tree(&resolved, &template_vars, &instances, &tree).write_atomic(&dest)?;
    Ok(report)
}
