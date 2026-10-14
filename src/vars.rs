//! Variable resolution (spec §3, §8.1): known values, `--data`, prompt, default.

use crate::cli::GlobalArgs;
use crate::error::SpliceError;
use crate::manifest::{PieceManifest, VarSpec};
use indexmap::IndexMap;
use std::io::IsTerminal;

type Result<T> = std::result::Result<T, SpliceError>;

pub struct VarSource<'a> {
    pub data: &'a IndexMap<String, String>,
    pub interactive: bool,
}

/// Prompts only on a terminal, without `--json` and without `--yes`.
pub fn interactive(global: &GlobalArgs) -> bool {
    !global.json && !global.yes && std::io::stdin().is_terminal()
}

fn check_pattern(name: &str, spec: &VarSpec, value: &str) -> Result<()> {
    match &spec.pattern {
        Some(re) if !re.is_match(value) => Err(SpliceError::Usage(format!(
            "value {value:?} for var `{name}` does not match pattern {re}"
        ))),
        _ => Ok(()),
    }
}

/// Resolve every var in `specs`, in order: `known`, then `--data`, then a prompt (when
/// interactive), then the default; otherwise a usage error naming the `--data` flag.
pub fn resolve_vars(
    specs: &IndexMap<String, VarSpec>,
    known: &IndexMap<String, String>,
    src: &VarSource,
) -> Result<IndexMap<String, String>> {
    let mut out = IndexMap::new();
    for (name, spec) in specs {
        let given = known.get(name).or_else(|| src.data.get(name));
        let value = if let Some(v) = given {
            check_pattern(name, spec, v)?;
            v.clone()
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

fn prompt(name: &str, spec: &VarSpec) -> Result<String> {
    let mut input = dialoguer::Input::<String>::new().with_prompt(spec.prompt.as_str());
    if let Some(d) = &spec.default {
        input = input.default(d.clone());
    }
    let spec2 = spec.clone();
    let name2 = name.to_string();
    input
        .validate_with(move |v: &String| {
            check_pattern(&name2, &spec2, v).map_err(|e| e.to_string())
        })
        .interact_text()
        .map_err(|e| SpliceError::Usage(format!("could not read var `{name}`: {e}")))
}

/// Instance name (spec §8.1): a `multiple` piece takes `explicit`, else its `name` var if it
/// declares one, else its id; any other piece is named by its id.
pub fn instance_name(
    piece: &PieceManifest,
    explicit: Option<&str>,
    vars: &IndexMap<String, String>,
) -> String {
    if !piece.multiple {
        return piece.id.clone();
    }
    if let Some(n) = explicit {
        return n.to_string();
    }
    if piece.vars.contains_key("name")
        && let Some(n) = vars.get("name")
    {
        return n.clone();
    }
    piece.id.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;

    fn spec(default: Option<&str>, pattern: Option<&str>) -> VarSpec {
        VarSpec {
            prompt: "p".into(),
            default: default.map(String::from),
            pattern: pattern.map(|p| Regex::new(p).unwrap()),
        }
    }

    fn map(pairs: &[(&str, &str)]) -> IndexMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn src(data: &IndexMap<String, String>) -> VarSource<'_> {
        VarSource {
            data,
            interactive: false,
        }
    }

    #[test]
    fn known_beats_data_beats_default() {
        let specs: IndexMap<_, _> = [
            ("a".to_string(), spec(Some("d"), None)),
            ("b".to_string(), spec(Some("d"), None)),
            ("c".to_string(), spec(Some("d"), None)),
        ]
        .into();
        let data = map(&[("a", "data"), ("b", "data")]);
        let out = resolve_vars(&specs, &map(&[("a", "known")]), &src(&data)).unwrap();
        assert_eq!(out, map(&[("a", "known"), ("b", "data"), ("c", "d")]));
    }

    #[test]
    fn missing_without_default_names_the_flag() {
        let specs: IndexMap<_, _> = [("project".to_string(), spec(None, None))].into();
        let data = map(&[]);
        let e = resolve_vars(&specs, &map(&[]), &src(&data)).unwrap_err();
        assert!(matches!(&e, SpliceError::Usage(m) if m.contains("--data project=")));
    }

    #[test]
    fn data_failing_pattern_is_usage() {
        let specs: IndexMap<_, _> = [("scope".to_string(), spec(None, Some("^[a-z]+$")))].into();
        let data = map(&[("scope", "Bad!")]);
        let e = resolve_vars(&specs, &map(&[]), &src(&data)).unwrap_err();
        assert_eq!(e.exit_code(), 2);
    }

    fn piece(multiple: bool, with_name: bool) -> PieceManifest {
        let mut vars = IndexMap::new();
        if with_name {
            vars.insert("name".to_string(), spec(None, None));
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
            files_dir: "x".into(),
        }
    }

    #[test]
    fn instance_name_rules() {
        let v = map(&[("name", "web")]);
        assert_eq!(instance_name(&piece(true, true), Some("x"), &v), "x");
        assert_eq!(instance_name(&piece(true, true), None, &v), "web");
        assert_eq!(instance_name(&piece(true, false), None, &v), "app");
        assert_eq!(instance_name(&piece(false, true), Some("x"), &v), "app");
    }
}
