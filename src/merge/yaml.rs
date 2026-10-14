//! YAML merge via `yaml_serde`. Multi-document input fails to parse and falls back to text.

use super::keypath::{Leaf, Tree, merge_tree};
use super::{Merged, ParseError};
use yaml_serde::{Mapping, Value};

fn to_tree(v: &Value) -> Tree {
    if let Value::Mapping(m) = v {
        // Only string-keyed mappings are merged by key; anything else is one value.
        if m.keys().all(Value::is_string) {
            return Tree::Map(
                m.iter()
                    .map(|(k, v)| (k.as_str().unwrap_or_default().to_string(), to_tree(v)))
                    .collect(),
            );
        }
    }
    Tree::Leaf(Leaf::Yaml(v.clone()))
}

fn from_tree(t: &Tree) -> Value {
    match t {
        Tree::Map(m) => {
            let mut out = Mapping::new();
            for (k, v) in m {
                out.insert(Value::String(k.clone()), from_tree(v));
            }
            Value::Mapping(out)
        }
        Tree::Leaf(Leaf::Yaml(v)) => v.clone(),
        Tree::Leaf(_) => unreachable!("yaml tree holds only yaml leaves"),
    }
}

fn parse(s: &str) -> Result<Tree, ParseError> {
    yaml_serde::from_str::<Value>(s)
        .map(|v| to_tree(&v))
        .map_err(|e| ParseError(e.to_string()))
}

pub fn merge_yaml(base: &str, ours: &str, theirs: &str) -> Result<Merged, ParseError> {
    let (b, o, t) = (parse(base)?, parse(ours)?, parse(theirs)?);
    let mut conflicts = Vec::new();
    let merged = merge_tree(&b, &o, &t, &mut Vec::new(), &mut conflicts);
    let text = yaml_serde::to_string(&from_tree(&merged)).map_err(|e| ParseError(e.to_string()))?;
    Ok(Merged { text, conflicts })
}
