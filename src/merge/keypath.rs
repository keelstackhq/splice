//! Generic three-way merge over a tree of maps (contract §5).
//!
//! Arrays and scalars are leaves compared by value. Ours' key order is kept and keys that
//! only theirs has are appended. A key deleted on one side only is deleted; deleted on one
//! side and changed on the other is a conflict, as is a key changed to different values.

use indexmap::IndexMap;

/// A leaf value from one of the structured formats.
#[derive(Debug, Clone, PartialEq)]
pub enum Leaf {
    Json(serde_json::Value),
    Yaml(yaml_serde::Value),
    Toml(toml::Value),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tree {
    Map(IndexMap<String, Tree>),
    Leaf(Leaf),
}

/// Merge three trees. A conflict keeps ours' value and records its key path (`a.b.c`;
/// `<root>` for a conflict between the whole documents).
pub fn merge_tree(
    base: &Tree,
    ours: &Tree,
    theirs: &Tree,
    path: &mut Vec<String>,
    conflicts: &mut Vec<String>,
) -> Tree {
    merge_node(Some(base), Some(ours), Some(theirs), path, conflicts)
        .unwrap_or_else(|| ours.clone())
}

/// `None` means "absent / deleted".
fn merge_node(
    b: Option<&Tree>,
    o: Option<&Tree>,
    t: Option<&Tree>,
    path: &mut Vec<String>,
    conflicts: &mut Vec<String>,
) -> Option<Tree> {
    if o == t || t == b {
        return o.cloned();
    }
    if o == b {
        return t.cloned();
    }
    // Both sides changed it, differently.
    if let (Some(Tree::Map(om)), Some(Tree::Map(tm))) = (o, t) {
        let empty = IndexMap::new();
        let bm = match b {
            Some(Tree::Map(m)) => m,
            _ => &empty,
        };
        let mut out = IndexMap::new();
        for k in om.keys().chain(tm.keys().filter(|k| !om.contains_key(*k))) {
            path.push(k.clone());
            let r = merge_node(bm.get(k), om.get(k), tm.get(k), path, conflicts);
            path.pop();
            if let Some(v) = r {
                out.insert(k.clone(), v);
            }
        }
        return Some(Tree::Map(out));
    }
    conflicts.push(if path.is_empty() {
        "<root>".to_string()
    } else {
        path.join(".")
    });
    o.cloned()
}
