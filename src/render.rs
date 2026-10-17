//! MiniJinja engine configured for the portable subset (spec §6).

use std::collections::BTreeMap;

use indexmap::IndexMap;

use crate::classify::Classifier;
use crate::error::SpliceError;
use crate::fsio;
use crate::manifest::{MergeStrategy, Template};
use crate::output::Class;
use crate::paths::render_rel_path;
use crate::subset;

type Result<T> = std::result::Result<T, SpliceError>;

pub struct Renderer {
    env: minijinja::Environment<'static>,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        let mut env = minijinja::Environment::empty();
        env.add_filter("lower", |s: String| s.to_lowercase());
        env.add_filter("upper", |s: String| s.to_uppercase());
        env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
        env.set_auto_escape_callback(|_| minijinja::AutoEscape::None);
        env.set_keep_trailing_newline(true);
        Renderer { env }
    }

    /// Validate against the subset, render, and normalise `\r\n` to `\n`.
    pub fn render_str(&self, text: &str, ctx: &minijinja::Value, origin: &str) -> Result<String> {
        subset::validate(text, origin)?;
        let out = self
            .env
            .render_named_str(origin, text, ctx)
            .map_err(|e| SpliceError::Template(format!("{origin}: {e}")))?;
        Ok(out.replace("\r\n", "\n"))
    }

    /// Validate a path segment (only `{{ }}` output tags allowed) and render it.
    pub fn render_path_segment(
        &self,
        seg: &str,
        ctx: &minijinja::Value,
        origin: &str,
    ) -> Result<String> {
        subset::validate_path_segment(seg, origin)?;
        self.env
            .render_named_str(origin, seg, ctx)
            .map_err(|e| SpliceError::Template(format!("{origin}: {e}")))
    }
}

/// One piece instance to render: `name` is the instance name (the `name` var, ruling C6).
#[derive(Debug, Clone)]
pub struct PieceInstance {
    pub id: String,
    pub name: String,
    pub vars: IndexMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct RenderInput {
    pub template_vars: IndexMap<String, String>,
    pub instances: Vec<PieceInstance>,
    pub version: semver::Version,
}

#[derive(Debug, Clone)]
pub struct RenderedFile {
    pub path: String,
    pub bytes: Vec<u8>,
    pub class: Class,
    pub strategy: MergeStrategy,
    pub piece_id: String,
    pub piece_name: String,
    pub glob: Option<String>,
}

pub type RenderedTree = BTreeMap<String, RenderedFile>;

/// A rendered `[sections]` fragment for a host file (path relative to the repo).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSection {
    pub host: String,
    pub body: String,
    pub piece_id: String,
    pub piece_name: String,
}

pub type RenderedSections = BTreeMap<String, RenderedSection>;

fn context(
    t: &Template,
    input: &RenderInput,
    inst: &PieceInstance,
    pieces: &[String],
) -> minijinja::Value {
    let mut vars: IndexMap<&str, minijinja::Value> = IndexMap::new();
    for (k, v) in input.template_vars.iter().chain(inst.vars.iter()) {
        vars.insert(k, minijinja::Value::from(v.as_str()));
    }
    // The instance name always wins: target paths use `{{ name }}` (ruling C6).
    vars.insert("name", minijinja::Value::from(inst.name.as_str()));
    vars.insert("pieces", minijinja::Value::from_serialize(pieces));
    vars.insert(
        "piece",
        minijinja::context! { id => inst.id.as_str(), name => inst.name.as_str() },
    );
    vars.insert(
        "template",
        minijinja::context! {
            name => t.manifest.name.as_str(),
            version => input.version.to_string(),
        },
    );
    minijinja::Value::from_serialize(&vars)
}

fn template_err(msg: String) -> SpliceError {
    SpliceError::Template(msg)
}

/// Render every instance's `files/` tree. Instances render in order; the result is keyed by final
/// path. Nothing is written here, so any error leaves the destination untouched.
pub fn render_tree(t: &Template, input: &RenderInput) -> Result<RenderedTree> {
    let renderer = Renderer::new();
    let mut pieces: Vec<String> = Vec::new();
    for inst in &input.instances {
        if !pieces.contains(&inst.id) {
            pieces.push(inst.id.clone());
        }
    }
    let mut tree = RenderedTree::new();
    for inst in &input.instances {
        let piece = t
            .pieces
            .get(&inst.id)
            .ok_or_else(|| template_err(format!("unknown piece `{}`", inst.id)))?;
        let classifier = Classifier::for_piece(piece)?;
        let ctx = context(t, input, inst, &pieces);
        // A piece without `files/` (post-only) renders nothing.
        if !piece.files_dir.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&piece.files_dir)
            .follow_links(false)
            .sort_by_file_name()
        {
            let entry =
                entry.map_err(|e| template_err(format!("{}: {e}", piece.files_dir.display())))?;
            let ft = entry.file_type();
            if ft.is_dir() {
                continue;
            }
            let rel = entry
                .path()
                .strip_prefix(&piece.files_dir)
                .expect("walk stays below files dir")
                .components()
                .map(|c| {
                    c.as_os_str().to_str().map(str::to_string).ok_or_else(|| {
                        template_err(format!("non UTF-8 file name: {}", entry.path().display()))
                    })
                })
                .collect::<Result<Vec<_>>>()?
                .join("/");
            if piece.sections.values().any(|f| *f == rel) {
                continue; // a section fragment is not a file
            }
            let origin = format!("{}:{rel}", inst.id);
            if ft.is_symlink() {
                return Err(template_err(format!(
                    "{origin}: symlinks are not allowed in a template"
                )));
            }
            if !fsio::is_plain_file_below(&piece.files_dir, &rel) {
                return Err(template_err(format!(
                    "{origin}: is not a regular file below files/ \
                     (symlinks are not allowed in a template)"
                )));
            }
            let (path, key) = render_rel_path(&rel, piece.target.as_deref(), &renderer, &ctx)?;
            let raw = std::fs::read(entry.path())?;
            let bytes = if rel.ends_with(".jinja") {
                let text = String::from_utf8(raw)
                    .map_err(|_| template_err(format!("{origin}: .jinja file is not UTF-8")))?;
                let out = renderer.render_str(&text, &ctx, &origin)?;
                if out.trim().is_empty() {
                    continue;
                }
                out.into_bytes()
            } else {
                raw
            };
            let (class, strategy, glob) = classifier.classify(&key);
            if class == Class::Shared && std::str::from_utf8(&bytes).is_err() {
                return Err(template_err(format!(
                    "{path}: binary file matched as `shared`; binary files must be `owned` or `seed`"
                )));
            }
            let file = RenderedFile {
                path: path.clone(),
                bytes,
                class,
                strategy,
                piece_id: inst.id.clone(),
                piece_name: inst.name.clone(),
                glob,
            };
            if let Some(prev) = tree.insert(path.clone(), file) {
                return Err(template_err(format!(
                    "{path}: rendered twice (pieces `{}:{}` and `{}:{}`)",
                    prev.piece_id, prev.piece_name, inst.id, inst.name
                )));
            }
        }
    }
    Ok(tree)
}

/// Render every instance's `[sections]` fragments, keyed by host path. A fragment that renders
/// to whitespace only declares nothing; two instances for one host are an error.
pub fn render_sections(t: &Template, input: &RenderInput) -> Result<RenderedSections> {
    let renderer = Renderer::new();
    let mut pieces: Vec<String> = Vec::new();
    for inst in &input.instances {
        if !pieces.contains(&inst.id) {
            pieces.push(inst.id.clone());
        }
    }
    let mut out = RenderedSections::new();
    for inst in &input.instances {
        let piece = t
            .pieces
            .get(&inst.id)
            .ok_or_else(|| template_err(format!("unknown piece `{}`", inst.id)))?;
        let ctx = context(t, input, inst, &pieces);
        for (key, fragment) in &piece.sections {
            let (host, _) = render_rel_path(key, piece.target.as_deref(), &renderer, &ctx)?;
            let origin = format!("{}:{fragment}", inst.id);
            // Checked at load too; re-checked right before the read (the template may change).
            if !fsio::is_plain_file_below(&piece.files_dir, fragment) {
                return Err(template_err(format!(
                    "{origin}: [sections] fragment is not a regular file below files/ \
                     (symlinks are not allowed in a template)"
                )));
            }
            let raw = std::fs::read(piece.files_dir.join(fragment))?;
            let text = String::from_utf8(raw)
                .map_err(|_| template_err(format!("{origin}: fragment is not UTF-8")))?;
            let body = if fragment.ends_with(".jinja") {
                renderer.render_str(&text, &ctx, &origin)?
            } else {
                text.replace("\r\n", "\n")
            };
            if body.trim().is_empty() {
                continue;
            }
            let section = RenderedSection {
                host: host.clone(),
                body,
                piece_id: inst.id.clone(),
                piece_name: inst.name.clone(),
            };
            if let Some(prev) = out.insert(host.clone(), section) {
                return Err(template_err(format!(
                    "section host `{host}` is declared by pieces `{}:{}` and `{}:{}`; \
                     one section per host",
                    prev.piece_id, prev.piece_name, inst.id, inst.name
                )));
            }
        }
    }
    Ok(out)
}
