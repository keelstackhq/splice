//! Key-path 3-way merge shared by the json, yaml and toml strategies (contract §5).

use indexmap::IndexMap;
use serde_json::Value;

/// A value that is merged as a whole: scalars, arrays (no element merge) and anything else that
/// is not a map. Compared by value.
#[derive(Debug, Clone, PartialEq)]
pub struct Leaf(pub Value);

#[derive(Debug, Clone, PartialEq)]
pub enum Tree {
    Map(IndexMap<String, Tree>),
    Leaf(Leaf),
}

impl Tree {
    /// Objects become maps; every other value is a leaf.
    pub fn from_value(value: Value) -> Tree {
        match value {
            Value::Object(map) => Tree::Map(
                map.into_iter()
                    .map(|(k, v)| (k, Tree::from_value(v)))
                    .collect(),
            ),
            other => Tree::Leaf(Leaf(other)),
        }
    }

    /// An empty (or whitespace-only) base file counts as an empty map, so a file that only the
    /// template and the user created merges per key.
    pub fn empty_map() -> Tree {
        Tree::Map(IndexMap::new())
    }

    pub fn into_value(self) -> Value {
        match self {
            Tree::Map(map) => {
                Value::Object(map.into_iter().map(|(k, v)| (k, v.into_value())).collect())
            }
            Tree::Leaf(Leaf(v)) => v,
        }
    }
}

/// 3-way merge of `ours` and `theirs` against `base`. A key changed on one side takes that side's
/// value; changed on both to different values is a conflict, which keeps ours' value and records
/// the dotted path in `conflicts`. Key order of ours is kept; keys new in theirs are appended. A
/// key deleted on one side and unchanged on the other is deleted; deleted on one side and changed
/// on the other is a conflict (ours wins: a key ours deleted stays deleted).
pub fn merge_tree(
    base: &Tree,
    ours: &Tree,
    theirs: &Tree,
    path: &mut Vec<String>,
    conflicts: &mut Vec<String>,
) -> Tree {
    match (base, ours, theirs) {
        (_, Tree::Map(o), Tree::Map(t)) => {
            let empty = IndexMap::new();
            let b = match base {
                Tree::Map(b) => b,
                Tree::Leaf(_) => &empty,
            };
            Tree::Map(merge_maps(b, o, t, path, conflicts))
        }
        _ => {
            if ours == theirs || theirs == base {
                ours.clone()
            } else if ours == base {
                theirs.clone()
            } else {
                conflicts.push(join(path));
                ours.clone()
            }
        }
    }
}

fn merge_maps(
    base: &IndexMap<String, Tree>,
    ours: &IndexMap<String, Tree>,
    theirs: &IndexMap<String, Tree>,
    path: &mut Vec<String>,
    conflicts: &mut Vec<String>,
) -> IndexMap<String, Tree> {
    let keys = ours
        .keys()
        .chain(theirs.keys().filter(|k| !ours.contains_key(*k)));
    let mut out = IndexMap::new();
    for key in keys {
        let (b, o, t) = (base.get(key), ours.get(key), theirs.get(key));
        let merged = if o == t || t == b {
            o.cloned()
        } else if o == b {
            t.cloned()
        } else if let (Some(Tree::Map(_)), Some(Tree::Map(_))) = (o, t) {
            path.push(key.clone());
            let empty = Tree::Map(IndexMap::new());
            let merged = merge_tree(b.unwrap_or(&empty), o.unwrap(), t.unwrap(), path, conflicts);
            path.pop();
            Some(merged)
        } else {
            path.push(key.clone());
            conflicts.push(join(path));
            path.pop();
            o.cloned()
        };
        if let Some(value) = merged {
            out.insert(key.clone(), value);
        }
    }
    out
}

fn join(path: &[String]) -> String {
    if path.is_empty() {
        "(root)".to_string()
    } else {
        path.join(".")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn merge(b: Value, o: Value, t: Value) -> (Value, Vec<String>) {
        let mut conflicts = Vec::new();
        let tree = merge_tree(
            &Tree::from_value(b),
            &Tree::from_value(o),
            &Tree::from_value(t),
            &mut Vec::new(),
            &mut conflicts,
        );
        (tree.into_value(), conflicts)
    }

    #[test]
    fn template_only_change_wins_user_only_change_stays() {
        let (v, c) = merge(
            json!({"a": 1, "b": 1}),
            json!({"a": 1, "b": 2}),
            json!({"a": 9, "b": 1}),
        );
        assert_eq!(v, json!({"a": 9, "b": 2}));
        assert!(c.is_empty());
    }

    #[test]
    fn ours_order_kept_theirs_new_keys_appended() {
        let (v, _) = merge(
            json!({"a": 1}),
            json!({"z": 1, "a": 1}),
            json!({"a": 1, "n": 1}),
        );
        let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["z", "a", "n"]);
    }

    #[test]
    fn both_changed_differently_conflicts_and_keeps_ours() {
        let (v, c) = merge(
            json!({"a": {"b": 1}}),
            json!({"a": {"b": 2}}),
            json!({"a": {"b": 3}}),
        );
        assert_eq!(v, json!({"a": {"b": 2}}));
        assert_eq!(c, ["a.b"]);
    }

    #[test]
    fn map_added_on_both_sides_merges_per_key() {
        let (v, c) = merge(json!({}), json!({"m": {"x": 1}}), json!({"m": {"y": 2}}));
        assert_eq!(v, json!({"m": {"x": 1, "y": 2}}));
        assert!(c.is_empty());
    }

    #[test]
    fn map_vs_scalar_is_a_conflict() {
        let (_, c) = merge(json!({"a": 1}), json!({"a": {"x": 1}}), json!({"a": 2}));
        assert_eq!(c, ["a"]);
    }

    #[test]
    fn root_leaf_conflict_is_named() {
        let (_, c) = merge(json!([1]), json!([2]), json!([3]));
        assert_eq!(c, ["(root)"]);
    }

    #[test]
    fn deleted_by_ours_stays_deleted_when_template_unchanged() {
        let (v, c) = merge(
            json!({"a": 1, "b": 1}),
            json!({"a": 1}),
            json!({"a": 1, "b": 1}),
        );
        assert_eq!(v, json!({"a": 1}));
        assert!(c.is_empty());
    }
}
