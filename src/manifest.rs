//! `splice.toml` and `piece.toml`: parsing and validation (contract §2, §3; spec §5).

use std::path::{Path, PathBuf};

use globset::GlobBuilder;
use indexmap::IndexMap;
use regex::Regex;
use semver::Version;
use serde::Deserialize;

use crate::error::SpliceError;
use crate::fsio;
use crate::migrate::{Migration, load_migrations};

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

/// Keyword and literal names no var may take since contract §9.17 (not checked in a merge base).
const KEYWORD_VARS: [&str; 8] = ["true", "false", "none", "not", "and", "or", "in", "is"];

/// How strictly to load. A merge base (the template a lock points at, rendered only as the base
/// of `update`) skips the rules added in contract §9.15-26 that an older template may break:
/// the raw/`[sections]` `min_splice` floor, keyword var names, piece-id segment rules, and (in
/// `render_tree`) portable path segments and case collisions. Symlinks stay rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Strictness {
    Strict,
    MergeBase,
}

/// The first splice that reads `{% raw %}` and `[sections]` (contract §9.23, §9.26).
const RAW_AND_SECTIONS_SPLICE: Version = Version::new(0, 2, 0);

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
    /// `[sections]`: host path (relative to `target`) to fragment path under `files/`.
    pub sections: IndexMap<String, String>,
    pub files_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Template {
    pub root: PathBuf,
    pub manifest: TemplateManifest,
    /// Sorted by id.
    pub pieces: IndexMap<String, PieceManifest>,
    /// `migrations/<semver>.toml`, sorted by version.
    pub migrations: Vec<Migration>,
    /// Unknown keys found while loading (newer templates stay readable).
    pub warnings: Vec<String>,
    /// Rendered paths are checked against contract §9.21 (portable segments, case collisions).
    /// False only for a merge base ([`Template::load_merge_base`]).
    pub portable_paths: bool,
}

#[derive(Deserialize)]
struct RawVar {
    // Contract §9.15: defaults to the var name (filled in by `convert_vars`).
    prompt: Option<String>,
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
    // Contract §9.15: defaults to "".
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
    #[serde(default)]
    sections: IndexMap<String, String>,
}

fn err<T>(msg: String) -> Result<T> {
    Err(SpliceError::Template(msg))
}

/// Exit 3 when `path` itself is a symlink (contract §9.20). A missing path is not an error here.
fn no_symlink(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => err(format!(
            "{}: is a symlink; symlinks are not allowed in a template",
            path.display()
        )),
        _ => Ok(()),
    }
}

fn read_toml(path: &Path) -> Result<(String, toml::Table)> {
    no_symlink(path)?;
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

fn convert_vars(
    file: &Path,
    raw: IndexMap<String, RawVar>,
    strict: Strictness,
) -> Result<IndexMap<String, VarSpec>> {
    let mut out = IndexMap::new();
    for (name, v) in raw {
        let f = file.display();
        if !valid_var_name(&name) {
            return err(format!(
                "{f}: variable `{name}` must match [a-z_][a-z0-9_]*"
            ));
        }
        if RESERVED_VARS.contains(&name.as_str())
            || (strict == Strictness::Strict && KEYWORD_VARS.contains(&name.as_str()))
        {
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

/// `[sections]` entries: `.md` host, `.md`/`.md.jinja` fragment that is a file below `files/`.
fn check_sections(dir: &Path, f: &str, sections: &IndexMap<String, String>) -> Result<()> {
    for (host, fragment) in sections {
        if !host.ends_with(".md") {
            return err(format!("{f}: [sections] host \"{host}\" must end in .md"));
        }
        if !(fragment.ends_with(".md") || fragment.ends_with(".md.jinja")) {
            return err(format!(
                "{f}: [sections] fragment \"{fragment}\" must end in .md or .md.jinja"
            ));
        }
        let safe = !fragment.starts_with('/')
            && !fragment.contains('\\')
            && fragment
                .split('/')
                .all(|seg| !seg.is_empty() && seg != "." && seg != "..");
        if !safe || !fsio::is_plain_file_below(&dir.join("files"), fragment) {
            return err(format!(
                "{f}: [sections] fragment \"{fragment}\" is not a regular file below files/ (symlinks are not allowed)"
            ));
        }
    }
    Ok(())
}

fn load_piece(
    dir: &Path,
    dir_name: &str,
    warnings: &mut Vec<String>,
    strict: Strictness,
) -> Result<PieceManifest> {
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
            "sections",
        ],
    );
    warn_unknown_vars(warnings, &file, &table);
    if let Some(toml::Value::Table(t)) = table.get("files") {
        warn_unknown(warnings, &file, "files", t, &["owned", "shared", "seed"]);
    }
    let raw: RawPiece = parse(&file, &text)?;
    let f = file.display();
    if strict == Strictness::Strict
        && let Some(why) = crate::paths::segment_problem(dir_name)
    {
        return err(format!("{f}: piece id `{dir_name}` {why}"));
    }
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
    check_sections(dir, &f.to_string(), &raw.sections)?;
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
        vars: convert_vars(&file, raw.vars, strict)?,
        owned: raw.files.owned,
        shared: raw.files.shared,
        seed: raw.files.seed,
        merge,
        post,
        sections: raw.sections,
        files_dir: dir.join("files"),
    })
}

/// A template that uses `[sections]` or `{% raw %}` in a `.jinja` file must set `min_splice` to
/// 0.2.0 or later: older splices would misread it (contract §9.23, §9.26).
fn check_raw_and_sections(
    f: &str,
    min_splice: &Version,
    pieces: &IndexMap<String, PieceManifest>,
) -> Result<()> {
    if *min_splice >= RAW_AND_SECTIONS_SPLICE {
        return Ok(());
    }
    let need = format!("must set min_splice = \"{RAW_AND_SECTIONS_SPLICE}\" or later");
    for piece in pieces.values() {
        if !piece.sections.is_empty() {
            return err(format!(
                "{f}: piece `{}` uses [sections]; a template with [sections] {need} \
                 (contract §9.26)",
                piece.id
            ));
        }
        if !piece.files_dir.is_dir() {
            continue;
        }
        let walk = walkdir::WalkDir::new(&piece.files_dir)
            .follow_links(false)
            .follow_root_links(false);
        for entry in walk {
            let entry = entry.map_err(|e| SpliceError::Template(format!("{f}: {e}")))?;
            let path = entry.path();
            if !entry.file_type().is_file() || path.extension().is_none_or(|e| e != "jinja") {
                continue;
            }
            // Unreadable or invalid files are reported when they render.
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            if crate::subset::uses_raw(&text) {
                return err(format!(
                    "{}: uses `{{% raw %}}`; a template with raw blocks {need} (contract §9.23)",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

impl Template {
    /// Load the template a lock points at, used only as the merge base of `update`.
    /// Skips the contract §9.15-26 gates an older template may break (see `Strictness`); the
    /// result renders without portable-path and case-collision checks.
    pub fn load_merge_base(root: &Path) -> Result<Template> {
        Self::load_with(root, Strictness::MergeBase)
    }

    /// Load `splice.toml` and every `pieces/<id>/piece.toml`, running every check in spec §5
    /// except `min_splice` (see [`Template::check_min_splice`]).
    pub fn load(root: &Path) -> Result<Template> {
        Self::load_with(root, Strictness::Strict)
    }

    fn load_with(root: &Path, strict: Strictness) -> Result<Template> {
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
        let vars = convert_vars(&file, raw.vars, strict)?;

        let pieces_dir = root.join("pieces");
        no_symlink(&pieces_dir)?;
        let mut names = Vec::new();
        let entries = std::fs::read_dir(&pieces_dir).map_err(|e| {
            SpliceError::Template(format!("cannot read {}: {e}", pieces_dir.display()))
        })?;
        for entry in entries {
            let entry = entry?;
            let ft = entry.file_type()?;
            if ft.is_symlink() {
                no_symlink(&entry.path())?;
            }
            if ft.is_dir() {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        names.sort();
        let mut pieces = IndexMap::new();
        for name in names {
            let piece = load_piece(&pieces_dir.join(&name), &name, &mut warnings, strict)?;
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
        if strict == Strictness::Strict {
            check_raw_and_sections(&f.to_string(), &min_splice, &pieces)?;
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
            migrations: load_migrations(root)?,
            warnings,
            portable_paths: strict == Strictness::Strict,
        })
    }

    /// The first of `ids` whose piece declares `[sections]`.
    pub fn has_sections(&self, ids: &[&str]) -> Option<&str> {
        ids.iter()
            .filter_map(|id| self.pieces.get(*id))
            .find(|p| !p.sections.is_empty())
            .map(|p| p.id.as_str())
    }

    /// Exit 3 when one of `ids` declares `[sections]`: only `splice channel add` applies them.
    pub fn ensure_no_sections(&self, ids: &[&str]) -> Result<()> {
        match self.has_sections(ids) {
            Some(id) => err(format!(
                "piece `{id}` declares [sections]; sections are only applied by \
                 `splice channel add`"
            )),
            None => Ok(()),
        }
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
    fn non_portable_piece_ids_rejected() {
        // The id is also the piece's directory name. Windows cannot create the reserved names,
        // `:` or a trailing dot, so those run only on Unix; `paths::tests` checks every rule on
        // all platforms without touching the disk.
        let ids: &[&str] = if cfg!(windows) {
            &[".git", ".GIT"]
        } else {
            &["con", "Nul", "lpt1.x", "a:b", "x.", ".git"]
        };
        for &id in ids {
            let piece = format!("id = \"{id}\"\ndescription = \"x\"\n");
            let e = load_with(
                &template_toml("0.1.0", ""),
                &[("base", BASE_PIECE), (id, &piece)],
            )
            .unwrap_err();
            assert_eq!(e.exit_code(), 3, "{id}");
            assert!(e.to_string().contains("piece id"), "{id}: {e}");
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
