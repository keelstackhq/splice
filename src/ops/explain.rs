//! `splice explain` (spec §8.9): which piece and ownership rule govern a path.

use crate::cli::{ExplainArgs, GlobalArgs};
use crate::error::SpliceError;
use crate::fetch::Fetcher;
use crate::lock::{LOCK_PATH, Lock};
use crate::manifest::{MergeStrategy, Template};
use crate::output::{Action, Class, Report};
use crate::render::{PieceInstance, RenderInput, RenderedTree, render_tree};
use crate::source::TemplateSource;
use crate::sources::{SourceId, Sources};

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

/// The tree a lock renders from its pinned commit (offline after the first fetch).
fn render_lock(lock: &Lock) -> Result<RenderedTree> {
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
    render_tree(
        &template,
        &RenderInput {
            template_vars: lock.vars.clone(),
            instances,
            version: resolved.version.clone(),
        },
    )
}

/// Searches the main template, then every channel by name (one owner per path, so the first
/// hit is the owner). `source=` follows the `piece=`/`glob=`/`merge=` rule: messages with
/// `--json`, a column in the human line. A section host adds `section=NAME` per block.
pub fn run(g: &GlobalArgs, a: &ExplainArgs) -> Result<Report> {
    let mut report = Report::new("explain");
    let repo = g.dir.as_path();
    let path = normalise(&a.path)?;

    let sources = Sources::load(repo)?;
    if sources.main.is_none() && sources.channels.is_empty() {
        return Err(SpliceError::Usage(format!(
            "{} has no {LOCK_PATH}; run `splice new` first",
            repo.display()
        )));
    }
    let mut found = None;
    if let Some(lock) = &sources.main
        && let Some(file) = render_lock(lock)?.remove(&path)
    {
        found = Some((SourceId::Main, file));
    }
    if found.is_none() {
        for cl in &sources.channels {
            if let Some(file) = render_lock(&cl.lock)?.remove(&path) {
                found = Some((SourceId::Channel(cl.meta.name.clone()), file));
                break;
            }
        }
    }
    let sections: Vec<&str> = sources
        .channels
        .iter()
        .filter(|c| c.sections.contains_key(&path))
        .map(|c| c.meta.name.as_str())
        .collect();

    // Human mode prints the lines on stdout only; the messages carry the same facts for --json.
    match found {
        None => {
            if g.json {
                report.msg("unmanaged");
            }
            report.lines.push(format!("{path}  unmanaged"));
        }
        Some((source, file)) => {
            let piece = format!("{}/{}", file.piece_id, file.piece_name);
            let glob = file.glob.as_deref().unwrap_or("default");
            // Only shared files are merged; the strategy is `none` for the other classes.
            let merge = if file.class == Class::Shared {
                strategy_name(file.strategy)
            } else {
                "none"
            };
            // `source=` only when channels exist: a repo without channels explains as M2.
            let source = match source {
                _ if sources.channels.is_empty() => None,
                SourceId::Main => Some("source=main".to_string()),
                SourceId::Channel(n) => Some(format!("source=channel:{n}")),
            };
            report.push(&path, Action::Skip, file.class);
            if g.json {
                report.msg(format!("piece={piece}"));
                report.msg(format!("glob={glob}"));
                report.msg(format!("merge={merge}"));
                if let Some(s) = &source {
                    report.msg(s.clone());
                }
            }
            let column = source.map(|s| format!("  {s}")).unwrap_or_default();
            report.lines.push(format!(
                "{path}  {}  {piece}  glob={glob}  merge={merge}{column}",
                file.class.as_str()
            ));
        }
    }
    for name in sections {
        if g.json {
            report.msg(format!("section={name}"));
        }
        report.lines.push(format!("{path}  section={name}"));
    }
    Ok(report)
}
