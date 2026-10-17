//! `splice explain` (spec §8.9): which piece and ownership rule govern a path.

use crate::cli::{ExplainArgs, GlobalArgs};
use crate::error::SpliceError;
use crate::fetch::Fetcher;
use crate::lock::{LOCK_PATH, Lock};
use crate::manifest::{MergeStrategy, Template};
use crate::output::{Action, Class, Report};
use crate::render::{PieceInstance, RenderInput, render_tree};
use crate::source::TemplateSource;

type Result<T> = std::result::Result<T, SpliceError>;

/// A repo-relative path with `/` separators and no `./` prefix; paths that leave the repo are a
/// usage error.
fn normalise(raw: &str) -> Result<String> {
    let unified = raw.replace('\\', "/");
    if unified.starts_with('/') {
        return Err(SpliceError::Usage(format!(
            "`{raw}` is absolute; pass a path relative to the repo root"
        )));
    }
    let mut parts: Vec<&str> = Vec::new();
    for seg in unified.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                return Err(SpliceError::Usage(format!(
                    "`{raw}` leaves the repo; pass a path inside the repo root"
                )));
            }
            s => parts.push(s),
        }
    }
    Ok(parts.join("/"))
}

fn strategy_name(s: MergeStrategy) -> &'static str {
    match s {
        MergeStrategy::Json => "json",
        MergeStrategy::Yaml => "yaml",
        MergeStrategy::Toml => "toml",
        MergeStrategy::Text => "text",
    }
}

pub fn run(g: &GlobalArgs, a: &ExplainArgs) -> Result<Report> {
    let mut report = Report::new("explain");
    let repo = g.dir.as_path();
    let path = normalise(&a.path)?;

    let Some(lock) = Lock::read(repo)? else {
        return Err(SpliceError::Usage(format!(
            "{} has no {LOCK_PATH}; run `splice new` first",
            repo.display()
        )));
    };
    let src = TemplateSource::parse(&lock.template.source)?;
    let resolved =
        Fetcher::from_env().checkout(&src, &lock.template.commit, lock.template.version.clone())?;
    let template = Template::load(&resolved.root)?;
    let instances: Vec<PieceInstance> = lock
        .pieces
        .iter()
        .map(|p| PieceInstance {
            id: p.id.clone(),
            name: p.name.clone(),
            vars: p.vars.clone(),
        })
        .collect();
    let tree = render_tree(
        &template,
        &RenderInput {
            template_vars: lock.vars.clone(),
            instances,
            version: resolved.version.clone(),
        },
    )?;

    // Human mode prints the line on stdout only; the messages carry the same facts for --json.
    let Some(file) = tree.get(&path) else {
        if g.json {
            report.msg("unmanaged");
        }
        report.lines.push(format!("{path}  unmanaged"));
        return Ok(report);
    };
    let piece = format!("{}/{}", file.piece_id, file.piece_name);
    let glob = file.glob.as_deref().unwrap_or("default");
    // Only shared files are merged; the strategy is `none` for the other classes.
    let merge = if file.class == Class::Shared {
        strategy_name(file.strategy)
    } else {
        "none"
    };
    report.push(&path, Action::Skip, file.class);
    if g.json {
        report.msg(format!("piece={piece}"));
        report.msg(format!("glob={glob}"));
        report.msg(format!("merge={merge}"));
    }
    report.lines.push(format!(
        "{path}  {}  {piece}  glob={glob}  merge={merge}",
        file.class.as_str()
    ));
    Ok(report)
}
