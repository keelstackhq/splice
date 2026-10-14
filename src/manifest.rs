//! Template and piece manifests (spec §5, contract §2 and §3).

use crate::error::SpliceError;
use indexmap::IndexMap;
use regex::Regex;
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, SpliceError>;

#[derive(Debug, Clone)]
pub struct VarSpec {
    pub prompt: String,
    pub default: Option<String>,
    pub pattern: Option<Regex>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    New,
    Add,
    Update,
}

#[derive(Debug, Clone)]
pub struct PostCmd {
    pub run: String,
    pub when: Vec<When>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeStrategy {
    Json,
    Yaml,
    Toml,
    Text,
}

#[derive(Debug, Clone)]
pub struct TemplateManifest {
    pub format: u32,
    pub name: String,
    pub min_splice: semver::Version,
    pub base: String,
    pub vars: IndexMap<String, VarSpec>,
}

#[derive(Debug, Clone)]
pub struct PieceManifest {
    pub id: String,
    pub description: String,
    pub multiple: bool,
    pub requires: Vec<String>,
    pub target: Option<String>,
    pub vars: IndexMap<String, VarSpec>,
    pub owned: Vec<String>,
    pub shared: Vec<String>,
    pub seed: Vec<String>,
    pub merge: IndexMap<String, MergeStrategy>,
    pub post: Vec<PostCmd>,
    pub files_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Template {
    pub root: PathBuf,
    pub manifest: TemplateManifest,
    pub pieces: IndexMap<String, PieceManifest>,
    pub migrations: Vec<crate::migrate::Migration>,
    pub warnings: Vec<String>,
}

pub(crate) type Extra = IndexMap<String, toml::Value>;

#[derive(serde::Deserialize)]
struct RawVar {
    prompt: Option<String>,
    default: Option<String>,
    pattern: Option<String>,
    #[serde(flatten)]
    extra: Extra,
}

#[derive(serde::Deserialize)]
struct RawTemplate {
    format: u32,
    name: String,
    min_splice: String,
    base: String,
    #[serde(default)]
    vars: IndexMap<String, RawVar>,
    #[serde(flatten)]
    extra: Extra,
}

#[derive(serde::Deserialize, Default)]
struct RawFiles {
    #[serde(default)]
    owned: Vec<String>,
    #[serde(default)]
    shared: Vec<String>,
    #[serde(default)]
    seed: Vec<String>,
    #[serde(flatten)]
    extra: Extra,
}

#[derive(serde::Deserialize)]
struct RawPost {
    run: String,
    when: Vec<String>,
    #[serde(flatten)]
    extra: Extra,
}

#[derive(serde::Deserialize)]
struct RawPiece {
    id: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    multiple: bool,
    #[serde(default)]
    requires: Vec<String>,
    target: Option<String>,
    #[serde(default)]
    vars: IndexMap<String, RawVar>,
    #[serde(default)]
    files: RawFiles,
    #[serde(default)]
    merge: IndexMap<String, String>,
    #[serde(default)]
    post: Vec<RawPost>,
    #[serde(flatten)]
    extra: Extra,
}

const RESERVED: [&str; 3] = ["pieces", "piece", "template"];

pub(crate) fn bad(file: &str, msg: impl std::fmt::Display) -> SpliceError {
    SpliceError::Template(format!("{file}: {msg}"))
}

/// Symlinks in a template could lead reads outside its root: exit 3.
pub(crate) fn reject_symlink(path: &Path, label: &str) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => Err(bad(
            label,
            "is a symlink; symlinks are not allowed in templates",
        )),
        _ => Ok(()),
    }
}

pub(crate) fn read_toml<T: serde::de::DeserializeOwned>(path: &Path, label: &str) -> Result<T> {
    reject_symlink(path, label)?;
    let text =
        std::fs::read_to_string(path).map_err(|e| bad(label, format!("cannot read ({e})")))?;
    toml::from_str(&text).map_err(|e| bad(label, e.to_string().trim_end()))
}

pub(crate) fn warn_extra(warnings: &mut Vec<String>, file: &str, ctx: &str, extra: &Extra) {
    for k in extra.keys() {
        warnings.push(format!("{file}: unknown key `{ctx}{k}` ignored"));
    }
}

fn convert_vars(
    file: &str,
    raw: IndexMap<String, RawVar>,
    warnings: &mut Vec<String>,
) -> Result<IndexMap<String, VarSpec>> {
    let name_re = Regex::new("^[a-z_][a-z0-9_]*$").expect("static regex");
    let mut out = IndexMap::new();
    for (name, v) in raw {
        if !name_re.is_match(&name) {
            return Err(bad(
                file,
                format!("var `{name}` must match ^[a-z_][a-z0-9_]*$"),
            ));
        }
        warn_extra(warnings, file, &format!("vars.{name}."), &v.extra);
        let pattern = match &v.pattern {
            Some(p) => Some(Regex::new(p).map_err(|e| {
                bad(
                    file,
                    format!("var `{name}` has an invalid pattern {p:?}: {e}"),
                )
            })?),
            None => None,
        };
        if let (Some(d), Some(re)) = (&v.default, &pattern)
            && !re.is_match(d)
        {
            return Err(bad(
                file,
                format!("var `{name}` default {d:?} does not match its pattern {re}"),
            ));
        }
        let prompt = v.prompt.unwrap_or_else(|| name.clone());
        out.insert(
            name,
            VarSpec {
                prompt,
                default: v.default,
                pattern,
            },
        );
    }
    Ok(out)
}

fn glob_matcher(file: &str, key: &str, globs: &[String]) -> Result<globset::GlobSet> {
    let mut b = globset::GlobSetBuilder::new();
    for g in globs {
        let glob = globset::GlobBuilder::new(g)
            .literal_separator(true)
            .build()
            .map_err(|e| bad(file, format!("[files].{key} glob {g:?}: {e}")))?;
        b.add(glob);
    }
    b.build()
        .map_err(|e| bad(file, format!("[files].{key}: {e}")))
}

fn load_piece(
    dir: &Path,
    dir_name: &str,
    template_vars: &IndexMap<String, VarSpec>,
    warnings: &mut Vec<String>,
) -> Result<PieceManifest> {
    let file = format!("pieces/{dir_name}/piece.toml");
    let raw: RawPiece = read_toml(&dir.join("piece.toml"), &file)?;
    if raw.id != dir_name {
        return Err(bad(
            &file,
            format!("id {:?} must equal its directory name {dir_name:?}", raw.id),
        ));
    }
    warn_extra(warnings, &file, "", &raw.extra);
    warn_extra(warnings, &file, "files.", &raw.files.extra);
    let vars = convert_vars(&file, raw.vars, warnings)?;
    for name in vars.keys() {
        if RESERVED.contains(&name.as_str()) {
            return Err(bad(&file, format!("var `{name}` uses a reserved name")));
        }
        if template_vars.contains_key(name) {
            return Err(bad(
                &file,
                format!("var `{name}` shadows the template var `{name}` in splice.toml"),
            ));
        }
    }
    let _ = glob_matcher(&file, "owned", &raw.files.owned)?;
    let shared = glob_matcher(&file, "shared", &raw.files.shared)?;
    let _ = glob_matcher(&file, "seed", &raw.files.seed)?;
    let mut merge = IndexMap::new();
    for (key, strategy) in raw.merge {
        let s = match strategy.as_str() {
            "json" => MergeStrategy::Json,
            "yaml" => MergeStrategy::Yaml,
            "toml" => MergeStrategy::Toml,
            "text" => MergeStrategy::Text,
            other => {
                return Err(bad(
                    &file,
                    format!("[merge] {key:?}: unknown strategy {other:?} (json|yaml|toml|text)"),
                ));
            }
        };
        globset::GlobBuilder::new(&key)
            .literal_separator(true)
            .build()
            .map_err(|e| {
                bad(
                    &file,
                    format!("[merge] key {key:?} is not a valid glob: {e}"),
                )
            })?;
        if !shared.is_match(&key) {
            return Err(bad(
                &file,
                format!("[merge] key {key:?} does not match any `shared` glob"),
            ));
        }
        merge.insert(key, s);
    }
    let mut post = Vec::new();
    for p in raw.post {
        warn_extra(warnings, &file, "post.", &p.extra);
        let mut when = Vec::new();
        for w in &p.when {
            when.push(match w.as_str() {
                "new" => When::New,
                "add" => When::Add,
                "update" => When::Update,
                other => {
                    return Err(bad(
                        &file,
                        format!("[[post]].when value {other:?} must be new|add|update"),
                    ));
                }
            });
        }
        post.push(PostCmd { run: p.run, when });
    }
    Ok(PieceManifest {
        id: raw.id,
        description: raw.description,
        multiple: raw.multiple,
        requires: raw.requires,
        target: raw.target,
        vars,
        owned: raw.files.owned,
        shared: raw.files.shared,
        seed: raw.files.seed,
        merge,
        post,
        files_dir: dir.join("files"),
    })
}

impl Template {
    /// Load and validate a template (spec §5), except for `min_splice`, which is checked
    /// separately by [`Template::check_min_splice`].
    pub fn load(root: &Path) -> Result<Template> {
        let mut warnings = Vec::new();
        let raw: RawTemplate = read_toml(&root.join("splice.toml"), "splice.toml")?;
        if raw.format != 1 {
            return Err(bad(
                "splice.toml",
                format!(
                    "unsupported format {} (this splice reads format 1)",
                    raw.format
                ),
            ));
        }
        let min_splice = semver::Version::parse(&raw.min_splice).map_err(|e| {
            bad(
                "splice.toml",
                format!(
                    "min_splice {:?} is not a semver version: {e}",
                    raw.min_splice
                ),
            )
        })?;
        warn_extra(&mut warnings, "splice.toml", "", &raw.extra);
        let vars = convert_vars("splice.toml", raw.vars, &mut warnings)?;
        for name in vars.keys() {
            if RESERVED.contains(&name.as_str()) {
                return Err(bad(
                    "splice.toml",
                    format!("var `{name}` uses a reserved name"),
                ));
            }
        }

        let mut dirs = Vec::new();
        let pieces_dir = root.join("pieces");
        reject_symlink(&pieces_dir, "pieces")?;
        if pieces_dir.is_dir() {
            for e in std::fs::read_dir(&pieces_dir)? {
                let e = e?;
                reject_symlink(
                    &e.path(),
                    &format!("pieces/{}", e.file_name().to_string_lossy()),
                )?;
                if e.path().is_dir() {
                    dirs.push(e.file_name().to_string_lossy().into_owned());
                }
            }
        }
        dirs.sort();
        let migrations = crate::migrate::load_migrations(root, &mut warnings)?;
        let mut pieces = IndexMap::new();
        for d in dirs {
            let piece = load_piece(&pieces_dir.join(&d), &d, &vars, &mut warnings)?;
            pieces.insert(d, piece);
        }

        match pieces.get(&raw.base) {
            None => {
                return Err(bad(
                    "splice.toml",
                    format!("base piece {:?} does not exist under pieces/", raw.base),
                ));
            }
            Some(p) if p.multiple => {
                return Err(bad(
                    "splice.toml",
                    format!("base piece {:?} must have multiple = false", raw.base),
                ));
            }
            Some(_) => {}
        }
        for p in pieces.values() {
            for r in &p.requires {
                if !pieces.contains_key(r) {
                    return Err(bad(
                        &format!("pieces/{}/piece.toml", p.id),
                        format!("requires unknown piece {r:?} (ghost reference)"),
                    ));
                }
            }
        }

        Ok(Template {
            root: root.to_path_buf(),
            manifest: TemplateManifest {
                format: raw.format,
                name: raw.name,
                min_splice,
                base: raw.base,
                vars,
            },
            pieces,
            migrations,
            warnings,
        })
    }

    /// Exit 3 when the template needs a newer splice than `running`.
    pub fn check_min_splice(&self, running: &semver::Version) -> Result<()> {
        let need = &self.manifest.min_splice;
        if need > running {
            return Err(SpliceError::Template(format!(
                "template needs splice >= {need} (running {running}); run `pnpm dlx @keelstack/splice@latest ...`"
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use semver::Version;

    const SPLICE: &str = r#"
format = 1
name = "t"
min_splice = "0.1.0"
base = "base"

[vars]
scope = { prompt = "scope", default = "acme", pattern = "^[a-z0-9-]+$" }
"#;
    const BASE: &str = r#"
id = "base"
description = "base"
multiple = false
"#;

    fn load_with_migrations(names: &[&str]) -> Result<Template> {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("splice.toml"), SPLICE).unwrap();
        let p = dir.path().join("pieces/base");
        std::fs::create_dir_all(p.join("files")).unwrap();
        std::fs::write(p.join("piece.toml"), BASE).unwrap();
        std::fs::create_dir_all(dir.path().join("migrations")).unwrap();
        for n in names {
            std::fs::write(dir.path().join("migrations").join(n), "").unwrap();
        }
        Template::load(dir.path())
    }

    #[test]
    fn migration_names_are_semver_toml() {
        assert!(load_with_migrations(&["1.2.0.toml", "0.1.0-rc.1.toml"]).is_ok());
        for bad in ["latest.toml", "1.2.0.txt", "1.2.toml", "v1.2.0.toml"] {
            let e = load_with_migrations(&[bad]).unwrap_err();
            assert_eq!(e.exit_code(), 3);
            assert!(e.to_string().contains(bad), "{e}");
        }
    }

    #[test]
    fn description_and_prompt_are_optional() {
        let t = load_inline(
            SPLICE,
            &[("base", "id = \"base\"\n[vars]\nv = { default = \"x\" }")],
        )
        .unwrap();
        assert_eq!(t.pieces["base"].description, "");
        assert_eq!(t.pieces["base"].vars["v"].prompt, "v");
    }

    #[test]
    fn post_when_is_required() {
        let piece = format!("{BASE}\n[[post]]\nrun = \"x\"");
        let msg = err(SPLICE, &[("base", &piece)]);
        assert!(msg.contains("when"), "{msg}");
    }

    #[test]
    fn nested_unknown_keys_warn() {
        let piece = format!(
            "{BASE}\n[vars]\nv = {{ prompt = \"p\", zed = 1 }}\n[files]\nbogus = []\n[[post]]\nrun = \"x\"\nwhen = [\"new\"]\nextra = 1\n"
        );
        let t = load_inline(SPLICE, &[("base", &piece)]).unwrap();
        for k in ["vars.v.zed", "files.bogus", "post.extra"] {
            assert!(
                t.warnings.iter().any(|w| w.contains(k)),
                "{k}: {:?}",
                t.warnings
            );
        }
    }

    /// Write `splice.toml` and `(dir, piece.toml)` pairs into a temp template and load it.
    fn load_inline(splice: &str, pieces: &[(&str, &str)]) -> Result<Template> {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("splice.toml"), splice).unwrap();
        for (id, toml) in pieces {
            let p = dir.path().join("pieces").join(id);
            std::fs::create_dir_all(p.join("files")).unwrap();
            std::fs::write(p.join("piece.toml"), toml).unwrap();
        }
        Template::load(dir.path())
    }

    fn err(splice: &str, pieces: &[(&str, &str)]) -> String {
        let e = load_inline(splice, pieces).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        e.to_string()
    }

    #[test]
    fn inline_baseline_loads() {
        let t = load_inline(SPLICE, &[("base", BASE)]).unwrap();
        assert_eq!(t.pieces.len(), 1);
    }

    #[test]
    fn rejects_bad_manifests() {
        let with_piece = |extra: &str| format!("{BASE}\n{extra}");
        type Case<'a> = (String, Vec<(&'a str, String)>, &'a str);
        let cases: Vec<Case> = vec![
            (
                SPLICE.replace("format = 1", "format = 2"),
                vec![("base", BASE.into())],
                "format",
            ),
            (
                SPLICE.replace(r#"base = "base""#, r#"base = "nope""#),
                vec![("base", BASE.into())],
                "base",
            ),
            (
                SPLICE.into(),
                vec![("base", with_piece("[vars]\npieces = { prompt = \"p\" }"))],
                "reserved",
            ),
            (
                SPLICE.into(),
                vec![("base", with_piece("[vars]\nscope = { prompt = \"p\" }"))],
                "scope",
            ),
            (
                SPLICE.into(),
                vec![(
                    "base",
                    with_piece(
                        "[vars]\nv = { prompt = \"p\", default = \"Bad!\", pattern = \"^[a-z]+$\" }",
                    ),
                )],
                "pattern",
            ),
            (
                SPLICE.into(),
                vec![(
                    "base",
                    with_piece("[files]\nshared = [\"x.json\"]\n[merge]\n\"x.json\" = \"xml\""),
                )],
                "merge",
            ),
            (
                SPLICE.into(),
                vec![(
                    "base",
                    with_piece("[files]\nowned = [\"x.json\"]\n[merge]\n\"x.json\" = \"json\""),
                )],
                "shared",
            ),
            (
                SPLICE.into(),
                vec![("base", with_piece("requires = [\"ghost\"]"))],
                "ghost",
            ),
            (
                SPLICE.into(),
                vec![("base", BASE.replace("multiple = false", "multiple = true"))],
                "multiple",
            ),
            (
                SPLICE.into(),
                vec![("base", BASE.replace("\"base\"", "\"other\""))],
                "other",
            ),
            (
                SPLICE.into(),
                vec![("base", with_piece("[vars]\nBad-Name = { prompt = \"p\" }"))],
                "Bad-Name",
            ),
            (
                SPLICE.into(),
                vec![(
                    "base",
                    with_piece("[[post]]\nrun = \"x\"\nwhen = [\"later\"]"),
                )],
                "when",
            ),
            (
                SPLICE.replace("^[a-z0-9-]+$", "^(unclosed"),
                vec![("base", BASE.into())],
                "pattern",
            ),
            (
                SPLICE.replace("0.1.0", "banana"),
                vec![("base", BASE.into())],
                "min_splice",
            ),
            (
                "format = ".into(),
                vec![("base", BASE.into())],
                "splice.toml",
            ),
        ];
        for (splice, pieces, needle) in cases {
            let pieces: Vec<(&str, &str)> = pieces.iter().map(|(i, t)| (*i, t.as_str())).collect();
            let msg = err(&splice, &pieces);
            assert!(msg.contains(needle), "expected {needle:?} in {msg:?}");
        }
    }

    #[test]
    fn unknown_keys_warn() {
        let splice = format!("future = 1\n{SPLICE}");
        let piece = format!("{BASE}\nnovel = true\n");
        let t = load_inline(&splice, &[("base", &piece)]).unwrap();
        assert_eq!(t.warnings.len(), 2, "{:?}", t.warnings);
        assert!(t.warnings.iter().any(|w| w.contains("future")));
        assert!(t.warnings.iter().any(|w| w.contains("novel")));
    }

    #[test]
    fn min_splice_too_new() {
        let t = load_inline(&SPLICE.replace("0.1.0", "99.0.0"), &[("base", BASE)]).unwrap();
        let e = t.check_min_splice(&Version::new(0, 1, 0)).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("99.0.0"));
        assert!(t.check_min_splice(&Version::new(99, 0, 0)).is_ok());
    }
}
