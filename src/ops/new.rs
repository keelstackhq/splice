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
use crate::render::{RenderInput, render_tree};
use crate::source::TemplateSource;

type Result<T> = std::result::Result<T, SpliceError>;

fn usage<T>(msg: String) -> Result<T> {
    Err(SpliceError::Usage(msg))
}

/// `K=V` flags; a later flag for the same key wins.
pub(crate) fn parse_data(flags: &[String]) -> Result<IndexMap<String, String>> {
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

/// Spec §8.1 step 2: no lock yet, and the destination is empty (apart from `.git`) unless forced.
fn check_dest(dest: &Path, force: bool) -> Result<bool> {
    if dest.join(LOCK_PATH).exists() {
        return usage(format!(
            "{} already has a {LOCK_PATH}; use `splice update` or `splice add`",
            dest.display()
        ));
    }
    if !dest.exists() {
        return Ok(false);
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
    Ok(occupied)
}

/// Refuse when the enclosing git repo's ignore rules hide the lock or any rendered file.
fn refuse_ignored<'a>(dest: &Path, files: impl Iterator<Item = &'a str>) -> Result<()> {
    let mut probe = dest;
    while !probe.exists() {
        probe = probe.parent().unwrap_or(Path::new("."));
    }
    let prefix = dest
        .strip_prefix(probe)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let join = |f: &str| {
        if prefix.is_empty() {
            f.to_string()
        } else {
            format!("{prefix}/{f}")
        }
    };
    let rels: Vec<String> = std::iter::once(LOCK_PATH).chain(files).map(join).collect();
    if let Some(hit) = Git::ignored(probe, &rels)?.and_then(|v| v.into_iter().next()) {
        let hit = hit
            .strip_prefix(&format!("{prefix}/"))
            .unwrap_or(&hit)
            .to_string();
        return usage(format!(
            "{hit} is ignored by a .gitignore of the enclosing git repo, so the files would never \
             be committed; pick another destination or remove the ignore rule"
        ));
    }
    Ok(())
}

/// A failure after files were written: exit 3, with the written `changes` kept in `--json`.
fn applied(mut report: Report, msg: String) -> SpliceError {
    report.ok = false;
    report.msg(msg);
    SpliceError::Applied(report)
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
    if matches!(src.kind, crate::source::SourceKind::Local { .. }) {
        report.msg(
            "the template source is a local path: the lock records it as given, so it is not \
             portable to another machine; use a git source such as `gh:owner/repo` for a lock \
             others can update",
        );
    }

    // 2. Destination.
    let dest: PathBuf = g.dir.join(&a.dest);
    let occupied = check_dest(&dest, a.force)?;
    if occupied && !a.allow_dirty && Git::is_clean(&dest)? == Some(false) {
        return usage(
            "the destination is a git work tree with uncommitted changes; commit or stash them \
             first (or pass --allow-dirty)"
                .into(),
        );
    }

    if a.commit {
        if occupied {
            return usage("--commit needs an empty destination; commit by hand instead".into());
        }
        // The nearest existing ancestor decides: a dirty enclosing repo would be swept in.
        let mut probe = dest.as_path();
        while !probe.exists() {
            probe = probe.parent().unwrap_or(Path::new("."));
        }
        if Git::is_clean(probe)? == Some(false) {
            return usage(
                "--commit: the enclosing git work tree has uncommitted changes; commit or stash \
                 them first, or commit by hand"
                    .into(),
            );
        }
    }

    // 3. Instances, then 4. variables.
    let picked = super::update::select_pieces(&template, &template.manifest.base, &a.pieces)?;
    template.ensure_no_sections(&picked.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>())?;
    let data = parse_data(&a.data)?;
    let (template_vars, instances) =
        super::update::instances_for(&template, &template.manifest.base, &a.pieces, &data, g)?;

    // 5. Render everything before touching the destination.
    let tree = render_tree(
        &template,
        &RenderInput {
            template_vars: template_vars.clone(),
            instances: instances.clone(),
            version: resolved.version.clone(),
        },
    )?;

    // A destination the enclosing repo ignores would silently drop files from every commit.
    refuse_ignored(&dest, tree.values().map(|f| f.path.as_str()))?;

    // Write every file; seeds only when absent.
    // Check every target first so a refusal (symlink, `..`) changes nothing.
    for file in tree.values() {
        fsio::check_target(&dest, &file.path)?;
    }
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
        if let Err(e) = run_post(&dest, &cmds, When::New, g.json) {
            return Err(match e {
                SpliceError::Env(m) => applied(
                    report,
                    format!(
                        "{m}; files were written but no {LOCK_PATH} yet: fix the command and \
                         rerun `splice new … --force`"
                    ),
                ),
                other => other,
            });
        }
    }

    if let Err(e) =
        Lock::from_tree(&resolved, &template_vars, &instances, &tree).write_atomic(&dest)
    {
        return Err(applied(report, format!("writing {LOCK_PATH} failed: {e}")));
    }
    if a.commit {
        if let Err(e) = Git::commit_all(
            &dest,
            &format!("chore: apply template {}", template.manifest.name),
        ) {
            return Err(applied(
                report,
                format!(
                    "files and {LOCK_PATH} were written but {e}; fix it (identity, hooks, \
                     signing) and run `git add -A && git commit`"
                ),
            ));
        }
    } else {
        report.msg(
            "next: commit the result (`git add -A && git commit`) before `splice add` or \
             `splice update`, which refuse a dirty tree; or pass --commit next time",
        );
    }
    Ok(report)
}
