//! `splice.toml` and `piece.toml`: parsing and validation (contract §2, §3; spec §5).

use std::path::{Path, PathBuf};

use globset::GlobBuilder;
use indexmap::IndexMap;
use regex::Regex;
use semver::Version;
use serde::Deserialize;

use crate::error::SpliceError;

type Result<T> = std::result::Result<T, SpliceError>;

/// Names a variable may not take: contract context keys (§4) and MiniJinja/Nunjucks keywords
/// that behave differently in the two engines (spec §5, controller ruling C1).
const RESERVED_VARS: [&str; 7] = [
    "pieces",
    "piece",
    "template",
    "loop",
    "self",
    "range",
    "namespace",
];

#[derive(Debug, Clone)]
pub struct VarSpec {
    pub prompt: String,
    pub default: Option<String>,
    pub pattern: Option<Regex>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
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
    pub min_splice: Version,
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
    /// Sorted by id.
    pub pieces: IndexMap<String, PieceManifest>,
    /// Unknown keys found while loading (newer templates stay readable).
    pub warnings: Vec<String>,
}

#[derive(Deserialize)]
struct RawVar {
    prompt: String,
    default: Option<String>,
    pattern: Option<String>,
}

#[derive(Deserialize)]
struct RawTemplate {
    format: u32,
    name: String,
    min_splice: String,
    base: String,
    #[serde(default)]
    vars: IndexMap<String, RawVar>,
}

#[derive(Deserialize, Default)]
struct RawFiles {
    #[serde(default)]
    owned: Vec<String>,
    #[serde(default)]
    shared: Vec<String>,
    #[serde(default)]
    seed: Vec<String>,
}

#[derive(Deserialize)]
struct RawPost {
    run: String,
    // Required by the hull renderer rule (contract §9, ruling C5).
    when: Option<Vec<When>>,
}

#[derive(Deserialize)]
struct RawPiece {
    id: String,
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
}

fn err<T>(msg: String) -> Result<T> {
    Err(SpliceError::Template(msg))
}

fn read_toml(path: &Path) -> Result<(String, toml::Table)> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| SpliceError::Template(format!("cannot read {}: {e}", path.display())))?;
    let table = text
        .parse::<toml::Table>()
        .map_err(|e| SpliceError::Template(format!("{}: {e}", path.display())))?;
    Ok((text, table))
}

fn parse<T: for<'de> Deserialize<'de>>(path: &Path, text: &str) -> Result<T> {
    toml::from_str(text).map_err(|e| SpliceError::Template(format!("{}: {e}", path.display())))
}

fn warn_unknown(
    warnings: &mut Vec<String>,
    file: &Path,
    scope: &str,
    table: &toml::Table,
    known: &[&str],
) {
    for key in table.keys().filter(|k| !known.contains(&k.as_str())) {
        let at = if scope.is_empty() {
            key.clone()
        } else {
            format!("{scope}.{key}")
        };
        warnings.push(format!("{}: unknown key `{at}` ignored", file.display()));
    }
}

fn warn_unknown_vars(warnings: &mut Vec<String>, file: &Path, root: &toml::Table) {
    let Some(toml::Value::Table(vars)) = root.get("vars") else {
        return;
    };
    for (name, spec) in vars {
        if let toml::Value::Table(t) = spec {
            let scope = format!("vars.{name}");
            warn_unknown(warnings, file, &scope, t, &["prompt", "default", "pattern"]);
        }
    }
}

fn valid_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('a'..='z' | '_'))
        && chars.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_'))
}

fn convert_vars(file: &Path, raw: IndexMap<String, RawVar>) -> Result<IndexMap<String, VarSpec>> {
    let mut out = IndexMap::new();
    for (name, v) in raw {
        let f = file.display();
        if !valid_var_name(&name) {
            return err(format!(
                "{f}: variable `{name}` must match [a-z_][a-z0-9_]*"
            ));
        }
        if RESERVED_VARS.contains(&name.as_str()) {
            return err(format!("{f}: variable name `{name}` is reserved"));
        }
        let pattern = match &v.pattern {
            Some(p) => Some(Regex::new(p).map_err(|e| {
                SpliceError::Template(format!("{f}: vars.{name}.pattern `{p}` is invalid: {e}"))
            })?),
            None => None,
        };
        if let (Some(d), Some(re)) = (&v.default, &pattern)
            && !re.is_match(d)
        {
            return err(format!(
                "{f}: vars.{name}.default `{d}` does not match its pattern `{re}`"
            ));
        }
        out.insert(
            name,
            VarSpec {
                prompt: v.prompt,
                default: v.default,
                pattern,
            },
        );
    }
    Ok(out)
}

fn globs(file: &Path, key: &str, patterns: &[String]) -> Result<Vec<globset::GlobMatcher>> {
    patterns
        .iter()
        .map(|p| {
            GlobBuilder::new(p)
                .literal_separator(true)
                .build()
                .map(|g| g.compile_matcher())
                .map_err(|e| {
                    SpliceError::Template(format!("{}: {key} glob `{p}`: {e}", file.display()))
                })
        })
        .collect()
}

fn load_piece(dir: &Path, dir_name: &str, warnings: &mut Vec<String>) -> Result<PieceManifest> {
    let file = dir.join("piece.toml");
    let (text, table) = read_toml(&file)?;
    warn_unknown(
        warnings,
        &file,
        "",
        &table,
        &[
            "id",
            "description",
            "multiple",
            "requires",
            "target",
            "vars",
            "files",
            "merge",
            "post",
        ],
    );
    warn_unknown_vars(warnings, &file, &table);
    if let Some(toml::Value::Table(t)) = table.get("files") {
        warn_unknown(warnings, &file, "files", t, &["owned", "shared", "seed"]);
    }
    let raw: RawPiece = parse(&file, &text)?;
    let f = file.display();
    if raw.id != dir_name {
        return err(format!(
            "{f}: id `{}` must equal the directory name `{dir_name}`",
            raw.id
        ));
    }
    let mut post = Vec::new();
    for (i, p) in raw.post.into_iter().enumerate() {
        let Some(when) = p.when else {
            return err(format!("{f}: [[post]] #{} needs `when`", i + 1));
        };
        post.push(PostCmd { run: p.run, when });
    }
    globs(&file, "files.owned", &raw.files.owned)?;
    globs(&file, "files.seed", &raw.files.seed)?;
    let shared = globs(&file, "files.shared", &raw.files.shared)?;
    let mut merge = IndexMap::new();
    for (key, value) in raw.merge {
        let strategy = match value.as_str() {
            "json" => MergeStrategy::Json,
            "yaml" => MergeStrategy::Yaml,
            "toml" => MergeStrategy::Toml,
            "text" => MergeStrategy::Text,
            other => {
                return err(format!(
                    "{f}: [merge] \"{key}\" = \"{other}\" must be json, yaml, toml or text"
                ));
            }
        };
        if !shared.iter().any(|g| g.is_match(&key)) {
            return err(format!(
                "{f}: [merge] key \"{key}\" does not match any `shared` glob"
            ));
        }
        merge.insert(key, strategy);
    }
    Ok(PieceManifest {
        id: raw.id,
        description: raw.description,
        multiple: raw.multiple,
        requires: raw.requires,
        target: raw.target,
        vars: convert_vars(&file, raw.vars)?,
        owned: raw.files.owned,
        shared: raw.files.shared,
        seed: raw.files.seed,
        merge,
        post,
        files_dir: dir.join("files"),
    })
}

impl Template {
    /// Load `splice.toml` and every `pieces/<id>/piece.toml`, running every check in spec §5
    /// except `min_splice` (see [`Template::check_min_splice`]).
    pub fn load(root: &Path) -> Result<Template> {
        let mut warnings = Vec::new();
        let file = root.join("splice.toml");
        let (text, table) = read_toml(&file)?;
        warn_unknown(
            &mut warnings,
            &file,
            "",
            &table,
            &["format", "name", "min_splice", "base", "vars"],
        );
        warn_unknown_vars(&mut warnings, &file, &table);
        let raw: RawTemplate = parse(&file, &text)?;
        let f = file.display();
        if raw.format != 1 {
            return err(format!(
                "{f}: unsupported template format {} (this splice reads format 1)",
                raw.format
            ));
        }
        let min_splice = Version::parse(&raw.min_splice).map_err(|e| {
            SpliceError::Template(format!(
                "{f}: min_splice `{}` is not a version: {e}",
                raw.min_splice
            ))
        })?;
        let vars = convert_vars(&file, raw.vars)?;

        let pieces_dir = root.join("pieces");
        let mut names = Vec::new();
        let entries = std::fs::read_dir(&pieces_dir).map_err(|e| {
            SpliceError::Template(format!("cannot read {}: {e}", pieces_dir.display()))
        })?;
        for entry in entries {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        names.sort();
        let mut pieces = IndexMap::new();
        for name in names {
            let piece = load_piece(&pieces_dir.join(&name), &name, &mut warnings)?;
            pieces.insert(name, piece);
        }

        match pieces.get(&raw.base) {
            None => {
                return err(format!(
                    "{f}: base piece `{}` does not exist under pieces/",
                    raw.base
                ));
            }
            Some(p) if p.multiple => {
                return err(format!(
                    "{f}: base piece `{}` must have multiple = false",
                    raw.base
                ));
            }
            Some(_) => {}
        }
        for piece in pieces.values() {
            for req in &piece.requires {
                if !pieces.contains_key(req) {
                    return err(format!(
                        "piece `{}` requires `{req}`, which does not exist",
                        piece.id
                    ));
                }
            }
            for name in piece.vars.keys() {
                if vars.contains_key(name) {
                    return err(format!(
                        "piece `{}` variable `{name}` shadows the template variable of the same name",
                        piece.id
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
            warnings,
        })
    }

    /// Fail (exit 3) when the running splice is older than the template's `min_splice`.
    pub fn check_min_splice(&self, running: &Version) -> Result<()> {
        let need = &self.manifest.min_splice;
        if need > running {
            return err(format!(
                "template needs splice >= {need} (running {running}); run `pnpm dlx @keelstackhq/splice@latest ...`"
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE_PIECE: &str = "id = \"base\"\ndescription = \"b\"\n";

    fn template_toml(min: &str, extra: &str) -> String {
        format!(
            "format = 1\nname = \"t\"\nmin_splice = \"{min}\"\nbase = \"base\"\n{extra}\n\
             [vars]\nscope = {{ prompt = \"s\", default = \"acme\", pattern = \"^[a-z0-9-]+$\" }}\n"
        )
    }

    /// Write a template with the given `splice.toml` and `(piece id, piece.toml)` pairs.
    fn load_with(splice: &str, pieces: &[(&str, &str)]) -> Result<Template> {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("splice.toml"), splice).unwrap();
        for (id, toml) in pieces {
            let p = dir.path().join("pieces").join(id);
            std::fs::create_dir_all(p.join("files")).unwrap();
            std::fs::write(p.join("piece.toml"), toml).unwrap();
        }
        Template::load(dir.path())
    }

    fn load_inline(piece_extra: &str) -> Result<Template> {
        let piece = format!("{BASE_PIECE}{piece_extra}");
        load_with(&template_toml("0.1.0", ""), &[("base", &piece)])
    }

    #[test]
    fn minimal_template_loads() {
        let t = load_inline("").unwrap();
        assert_eq!(t.manifest.vars["scope"].default.as_deref(), Some("acme"));
        assert!(t.warnings.is_empty());
    }

    #[test]
    fn rejects_bad_manifests() {
        let cases: Vec<(Result<Template>, &str)> = vec![
            (
                load_with(
                    &template_toml("0.1.0", "").replace("format = 1", "format = 2"),
                    &[("base", BASE_PIECE)],
                ),
                "format",
            ),
            (
                load_with(
                    &template_toml("0.1.0", "").replace("base = \"base\"", "base = \"nope\""),
                    &[("base", BASE_PIECE)],
                ),
                "base",
            ),
            (
                load_with(
                    &template_toml("0.1.0", ""),
                    &[(
                        "base",
                        &format!("{BASE_PIECE}[vars]\npieces = {{ prompt = \"p\" }}\n"),
                    )],
                ),
                "reserved",
            ),
            (
                load_inline("[vars]\nloop = { prompt = \"p\" }\n"),
                "reserved",
            ),
            (load_inline("[vars]\nscope = { prompt = \"p\" }\n"), "scope"),
            (
                load_inline("[vars]\nBad = { prompt = \"p\" }\n"),
                "[a-z_][a-z0-9_]*",
            ),
            (
                load_inline(
                    "[vars]\nname = { prompt = \"p\", default = \"Bad!\", pattern = \"^[a-z]+$\" }\n",
                ),
                "pattern",
            ),
            (
                load_inline("[vars]\nname = { prompt = \"p\", pattern = \"(\" }\n"),
                "pattern",
            ),
            (
                load_inline("[files]\nshared = [\"x.json\"]\n[merge]\n\"x.json\" = \"xml\"\n"),
                "merge",
            ),
            (
                load_inline("[files]\nshared = [\"a.json\"]\n[merge]\n\"x.json\" = \"json\"\n"),
                "merge",
            ),
            (load_inline("requires = [\"ghost\"]\n"), "ghost"),
            (load_inline("[[post]]\nrun = \"echo\"\n"), "when"),
            (
                load_inline("[[post]]\nrun = \"echo\"\nwhen = [\"sometimes\"]\n"),
                "sometimes",
            ),
            (load_inline("multiple = true\n"), "multiple = false"),
            (
                load_with(
                    &template_toml("0.1.0", ""),
                    &[("base", "id = \"other\"\ndescription = \"b\"\n")],
                ),
                "directory name",
            ),
        ];
        for (result, needle) in cases {
            let e = result.unwrap_err();
            assert_eq!(e.exit_code(), 3);
            assert!(
                e.to_string().contains(needle),
                "{e} should mention {needle}"
            );
        }
    }

    #[test]
    fn merge_key_may_use_a_shared_glob() {
        let t =
            load_inline("[files]\nshared = [\"**/*.json\"]\n[merge]\n\"a/b.json\" = \"json\"\n")
                .unwrap();
        assert_eq!(t.pieces["base"].merge["a/b.json"], MergeStrategy::Json);
    }

    #[test]
    fn unknown_keys_warn() {
        let splice = template_toml("0.1.0", "future = 1\n");
        let piece = format!("{BASE_PIECE}colour = \"red\"\n[files]\nbinary = []\n");
        let t = load_with(&splice, &[("base", &piece)]).unwrap();
        let all = t.warnings.join("\n");
        assert!(all.contains("future") && all.contains("colour") && all.contains("files.binary"));
    }

    #[test]
    fn min_splice_too_new() {
        let t = load_with(&template_toml("99.0.0", ""), &[("base", BASE_PIECE)]).unwrap();
        let e = t.check_min_splice(&Version::new(0, 1, 0)).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("99.0.0"));
        t.check_min_splice(&Version::new(99, 0, 0)).unwrap();
    }
}
