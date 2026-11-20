//! `splice pieces` (spec §8.10): list a template's pieces and variables, read-only.

use indexmap::IndexMap;

use crate::cli::{GlobalArgs, PiecesArgs};
use crate::error::SpliceError;
use crate::fetch::Fetcher;
use crate::manifest::{Template, VarSpec};
use crate::output::{PieceInfo, Report, TemplateInfo, VarInfo};
use crate::source::TemplateSource;

fn var_infos(vars: &IndexMap<String, VarSpec>) -> Vec<VarInfo> {
    vars.iter()
        .map(|(name, v)| VarInfo {
            name: name.clone(),
            prompt: v.prompt.clone(),
            default: v.default.clone(),
        })
        .collect()
}

fn var_line(v: &VarInfo) -> String {
    match &v.default {
        Some(d) => format!("  var {}  \"{}\" [default: {d}]", v.name, v.prompt),
        None => format!("  var {}  \"{}\"", v.name, v.prompt),
    }
}

/// Resolve `a.source`, load the template and describe it. Needs no repo, lock or clean tree.
pub fn run(_g: &GlobalArgs, a: &PiecesArgs) -> Result<Report, SpliceError> {
    let src = TemplateSource::parse(&a.source)?;
    let resolved = Fetcher::from_env().resolve(&src, a.git_ref.as_deref(), a.pre)?;
    let template = Template::load(&resolved.root)?;
    let running = semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("crate version");
    template.check_min_splice(&running)?;
    let mut report = Report::new("pieces");
    report.messages.extend(template.warnings.iter().cloned());

    for piece in template.pieces.values() {
        // What `new --yes --piece ID` names the instance: a `name` var without a default has no
        // name (the run is exit 2), so null.
        let default_name = if !piece.multiple {
            None
        } else if let Some(v) = piece.vars.get("name") {
            v.default.clone()
        } else {
            Some(piece.id.clone())
        };
        report.pieces.push(PieceInfo {
            id: piece.id.clone(),
            description: piece.description.clone(),
            base: piece.id == template.manifest.base,
            multiple: piece.multiple,
            default_name,
            requires: piece.requires.clone(),
            vars: var_infos(&piece.vars),
        });
    }
    let info = TemplateInfo {
        name: template.manifest.name.clone(),
        version: Some(resolved.version.to_string()),
        vars: var_infos(&template.manifest.vars),
    };

    report.lines.push(format!(
        "template {} {}",
        info.name,
        info.version.as_deref().unwrap_or("")
    ));
    for v in &info.vars {
        report.lines.push(var_line(v));
    }
    for p in &report.pieces {
        let mut line = p.id.clone();
        if p.base {
            line.push_str("  [base]");
        }
        if let Some(d) = &p.default_name {
            line.push_str(&format!("  multiple (default name: {d})"));
        }
        if !p.requires.is_empty() {
            line.push_str(&format!("  requires: {}", p.requires.join(", ")));
        }
        if !p.description.is_empty() {
            line.push_str(&format!("  - {}", p.description));
        }
        report.lines.push(line);
        report.lines.extend(p.vars.iter().map(var_line));
    }
    report.template = Some(info);
    Ok(report)
}
