//! MiniJinja engine configured for the portable subset (spec section 6).

use crate::error::SpliceError;
use crate::subset;
use minijinja::{Environment, UndefinedBehavior, Value};

pub struct Renderer {
    env: Environment<'static>,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        let mut env = Environment::empty();
        env.set_undefined_behavior(UndefinedBehavior::Strict);
        env.set_keep_trailing_newline(true);
        env.add_filter("lower", |s: String| s.to_lowercase());
        env.add_filter("upper", |s: String| s.to_uppercase());
        Renderer { env }
    }

    /// Gate, render, then normalise `\r\n` to `\n`.
    pub fn render_str(&self, text: &str, ctx: &Value, origin: &str) -> Result<String, SpliceError> {
        subset::validate(text, origin)?;
        let out = self
            .env
            .render_str(text, ctx)
            .map_err(|e| SpliceError::Template(format!("{origin}: {e}")))?;
        Ok(out.replace("\r\n", "\n"))
    }
}

use crate::classify::Classifier;
use crate::manifest::{MergeStrategy, Template};
use crate::output::Class;
use crate::paths::render_rel_path;
use indexmap::IndexMap;
use std::collections::BTreeMap;
use walkdir::WalkDir;

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

fn tpl_err(msg: String) -> SpliceError {
    SpliceError::Template(msg)
}

/// Context for one instance (spec §6): template vars, instance vars, `pieces`, `piece`, `template`.
fn context(t: &Template, input: &RenderInput, inst: &PieceInstance, pieces: &[&str]) -> Value {
    let mut m = serde_json::Map::new();
    for (k, v) in input.template_vars.iter().chain(inst.vars.iter()) {
        m.insert(k.clone(), serde_json::Value::String(v.clone()));
    }
    m.insert("pieces".into(), serde_json::json!(pieces));
    m.insert(
        "piece".into(),
        serde_json::json!({ "id": inst.id, "name": inst.name }),
    );
    m.insert(
        "template".into(),
        serde_json::json!({ "name": t.manifest.name, "version": input.version.to_string() }),
    );
    Value::from_serialize(serde_json::Value::Object(m))
}

/// Render every instance's `files/` into a path-keyed tree. Symlinks anywhere under `files/`
/// are rejected so rendering can never read outside the template root.
pub fn render_tree(t: &Template, input: &RenderInput) -> Result<RenderedTree, SpliceError> {
    let renderer = Renderer::new();
    let mut pieces: Vec<&str> = Vec::new();
    for i in &input.instances {
        if !pieces.contains(&i.id.as_str()) {
            pieces.push(&i.id);
        }
    }
    for i in &input.instances {
        for (what, v) in [("id", &i.id), ("name", &i.name)] {
            crate::paths::validate_segment(v)
                .map_err(|e| tpl_err(format!("piece {what} {v:?} is not a safe name: {e}")))?;
        }
    }
    let root = t.root.canonicalize()?;
    let mut tree = RenderedTree::new();
    for inst in &input.instances {
        let piece = t
            .pieces
            .get(&inst.id)
            .ok_or_else(|| tpl_err(format!("unknown piece `{}`", inst.id)))?;
        let classifier = Classifier::for_piece(piece)?;
        let ctx = context(t, input, inst, &pieces);
        let files = &piece.files_dir;
        let shown = format!("pieces/{}/files", piece.id);
        match std::fs::symlink_metadata(files) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(tpl_err(format!(
                    "{shown} is a symlink; symlinks are not allowed"
                )));
            }
            Ok(m) if m.is_dir() => {}
            Ok(_) => return Err(tpl_err(format!("{shown} is not a directory"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        }
        if !files.canonicalize()?.starts_with(&root) {
            return Err(tpl_err(format!(
                "{shown} resolves through a symlink outside the template root"
            )));
        }
        for entry in WalkDir::new(files)
            .follow_links(false)
            .min_depth(1)
            .sort_by_file_name()
        {
            let entry = entry.map_err(|e| tpl_err(format!("{shown}: {e}")))?;
            let rel = entry
                .path()
                .strip_prefix(files)
                .expect("walk stays under files/")
                .components()
                .map(|c| c.as_os_str().to_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| tpl_err(format!("{shown}: non-UTF-8 file name")))?
                .join("/");
            let ft = entry.file_type();
            if ft.is_symlink() {
                return Err(tpl_err(format!(
                    "{shown}/{rel} is a symlink; symlinks are not allowed in templates"
                )));
            }
            if !ft.is_file() {
                continue; // directories (anything else is not a regular file)
            }
            let (path, key) = render_rel_path(&rel, piece.target.as_deref(), &renderer, &ctx)?;
            let origin = format!("{shown}/{rel}");
            let raw = std::fs::read(entry.path())?;
            let bytes = if rel.ends_with(".jinja") {
                let text = String::from_utf8(raw)
                    .map_err(|_| tpl_err(format!("{origin}: not valid UTF-8")))?;
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
                return Err(tpl_err(format!(
                    "{origin}: binary (non-UTF-8) file matches a `shared` glob; binary files must be owned or seed"
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
                return Err(tpl_err(format!(
                    "path `{path}` is produced twice (pieces `{}:{}` and `{}:{}`)",
                    prev.piece_id, prev.piece_name, inst.id, inst.name
                )));
            }
        }
    }
    Ok(tree)
}
