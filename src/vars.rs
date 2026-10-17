//! Resolving template and piece variables (spec §3, §8.1).

use std::io::IsTerminal;

use indexmap::IndexMap;

use crate::cli::GlobalArgs;
use crate::error::SpliceError;
use crate::manifest::{PieceManifest, VarSpec};

type Result<T> = std::result::Result<T, SpliceError>;

pub struct VarSource<'a> {
    pub data: &'a IndexMap<String, String>,
    pub interactive: bool,
}

/// Prompts are allowed only for a human at a terminal: not with `--json` or `--yes`, and not when
/// stdin is not a TTY.
pub fn interactive(global: &GlobalArgs) -> bool {
    !global.json && !global.yes && std::io::stdin().is_terminal()
}

fn pattern_error(name: &str, value: &str, spec: &VarSpec) -> Option<SpliceError> {
    let re = spec.pattern.as_ref()?;
    (!re.is_match(value)).then(|| {
        SpliceError::Usage(format!(
            "value `{value}` for var `{name}` does not match its pattern `{re}`"
        ))
    })
}

fn checked(name: &str, value: String, spec: &VarSpec) -> Result<String> {
    match pattern_error(name, &value, spec) {
        Some(e) => Err(e),
        None => Ok(value),
    }
}

fn prompt(name: &str, spec: &VarSpec) -> Result<String> {
    let mut input = dialoguer::Input::<String>::new().with_prompt(spec.prompt.as_str());
    if let Some(d) = &spec.default {
        input = input.default(d.clone());
    }
    let spec = spec.clone();
    let var = name.to_string();
    input
        .validate_with(move |v: &String| match pattern_error(&var, v, &spec) {
            Some(e) => Err(e.to_string()),
            None => Ok(()),
        })
        .interact_text()
        .map_err(|e| SpliceError::Env(format!("cannot read a value for var `{name}`: {e}")))
}

/// A value for every var in `specs`: `known` first, then `--data`, then a prompt (when
/// interactive; Enter takes the default), then the default. A var left without a value, or with a
/// value that fails its `pattern`, is a usage error (exit 2). Keys of `known` and `data` that are
/// not in `specs` are ignored here.
pub fn resolve_vars(
    specs: &IndexMap<String, VarSpec>,
    known: &IndexMap<String, String>,
    src: &VarSource,
) -> Result<IndexMap<String, String>> {
    let mut out = IndexMap::new();
    for (name, spec) in specs {
        let value = if let Some(v) = known.get(name).or_else(|| src.data.get(name)) {
            checked(name, v.clone(), spec)?
        } else if src.interactive {
            prompt(name, spec)?
        } else if let Some(d) = &spec.default {
            d.clone()
        } else {
            return Err(SpliceError::Usage(format!(
                "missing value for var `{name}`; pass --data {name}=…"
            )));
        };
        out.insert(name.clone(), value);
    }
    Ok(out)
}

/// Instance name (spec §8.1): a `multiple` piece takes `explicit` (`--piece ID:NAME`), else its
/// `name` var if it declares one, else its id; any other piece is named by its id.
pub fn instance_name(
    piece: &PieceManifest,
    explicit: Option<&str>,
    vars: &IndexMap<String, String>,
) -> String {
    if !piece.multiple {
        return piece.id.clone();
    }
    explicit
        .map(str::to_string)
        .or_else(|| {
            piece
                .vars
                .contains_key("name")
                .then(|| vars.get("name").cloned())
                .flatten()
        })
        .unwrap_or_else(|| piece.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;

    fn spec(default: Option<&str>, pattern: Option<&str>) -> VarSpec {
        VarSpec {
            prompt: "p".into(),
            default: default.map(str::to_string),
            pattern: pattern.map(|p| Regex::new(p).unwrap()),
        }
    }

    fn map(pairs: &[(&str, &str)]) -> IndexMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn specs() -> IndexMap<String, VarSpec> {
        let mut m = IndexMap::new();
        m.insert("scope".to_string(), spec(Some("acme"), Some("^[a-z]+$")));
        m.insert("project".to_string(), spec(None, None));
        m
    }

    fn src(data: &IndexMap<String, String>) -> VarSource<'_> {
        VarSource {
            data,
            interactive: false,
        }
    }

    #[test]
    fn known_beats_data_beats_default() {
        let data = map(&[("scope", "fromdata"), ("project", "p")]);
        let r = resolve_vars(&specs(), &map(&[("scope", "known")]), &src(&data)).unwrap();
        assert_eq!(r, map(&[("scope", "known"), ("project", "p")]));
        let data = map(&[("project", "p")]);
        let r = resolve_vars(&specs(), &map(&[]), &src(&data)).unwrap();
        assert_eq!(r, map(&[("scope", "acme"), ("project", "p")]));
    }

    #[test]
    fn missing_without_default_names_var_and_flag() {
        let data = map(&[]);
        let e = resolve_vars(&specs(), &map(&[]), &src(&data)).unwrap_err();
        assert_eq!(e.exit_code(), 2);
        let m = e.to_string();
        assert!(
            m.contains("`project`") && m.contains("--data project="),
            "{m}"
        );
    }

    #[test]
    fn pattern_is_checked_for_data_and_known() {
        let data = map(&[("scope", "Bad!"), ("project", "p")]);
        let e = resolve_vars(&specs(), &map(&[]), &src(&data)).unwrap_err();
        assert_eq!(e.exit_code(), 2);
        assert!(e.to_string().contains("`scope`"));
        let data = map(&[("project", "p")]);
        let e = resolve_vars(&specs(), &map(&[("scope", "NO")]), &src(&data)).unwrap_err();
        assert_eq!(e.exit_code(), 2);
    }

    fn piece(multiple: bool, name_var: bool) -> PieceManifest {
        let mut vars = IndexMap::new();
        if name_var {
            vars.insert("name".to_string(), spec(Some("web"), None));
        }
        PieceManifest {
            id: "app".into(),
            description: String::new(),
            multiple,
            requires: vec![],
            target: None,
            vars,
            owned: vec![],
            shared: vec![],
            seed: vec![],
            merge: IndexMap::new(),
            post: vec![],
            files_dir: "files".into(),
        }
    }

    #[test]
    fn instance_names_follow_spec_8_1() {
        let vars = map(&[("name", "from-var")]);
        assert_eq!(instance_name(&piece(true, true), Some("x"), &vars), "x");
        assert_eq!(instance_name(&piece(true, true), None, &vars), "from-var");
        assert_eq!(instance_name(&piece(true, false), None, &vars), "app");
        assert_eq!(instance_name(&piece(false, true), Some("x"), &vars), "app");
        assert_eq!(instance_name(&piece(false, false), None, &map(&[])), "app");
    }
}
