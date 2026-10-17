//! TOML merge: the key-path merge decides what changes, then the changes are applied to ours'
//! `toml_edit` document in place, so comments and layout of everything untouched survive.

use indexmap::IndexMap;
use serde_json::Value;
use toml_edit::{DocumentMut, Item, TableLike};

use super::keypath::{Leaf, Tree, merge_tree};
use super::{Merged, ParseError};

pub fn merge_toml(base: &str, ours: &str, theirs: &str) -> Result<Merged, ParseError> {
    let parse = |s: &str| {
        s.parse::<DocumentMut>()
            .map_err(|e| ParseError(e.to_string()))
    };
    let (b, mut o, t) = (parse(base)?, parse(ours)?, parse(theirs)?);
    let (tb, to, tt) = (
        table_tree(b.as_table()),
        table_tree(o.as_table()),
        table_tree(t.as_table()),
    );
    let mut conflicts = Vec::new();
    let merged = merge_tree(&tb, &to, &tt, &mut Vec::new(), &mut conflicts);
    if let (Tree::Map(result), Tree::Map(ours_map)) = (&merged, &to) {
        apply(o.as_table_mut(), result, ours_map, t.as_table());
    }
    Ok(Merged {
        text: o.to_string(),
        conflicts,
    })
}

fn table_tree(table: &dyn TableLike) -> Tree {
    Tree::Map(
        table
            .iter()
            .map(|(k, item)| (k.to_string(), item_tree(item)))
            .collect(),
    )
}

/// Tables and inline tables are maps; values (arrays included) and arrays of tables are leaves,
/// compared by their text without surrounding whitespace or comments.
fn item_tree(item: &Item) -> Tree {
    match item.as_table_like() {
        Some(table) => table_tree(table),
        None => Tree::Leaf(Leaf(Value::String(match item {
            Item::Value(v) => v.clone().decorated("", "").to_string(),
            other => other.to_string(),
        }))),
    }
}

/// Turn `ours` into `result`: remove keys that are gone, add keys new from theirs at the end of
/// their parent, replace changed values by theirs' (keeping ours' comment around a value), and
/// recurse into tables changed on both sides.
fn apply(
    ours: &mut dyn TableLike,
    result: &IndexMap<String, Tree>,
    ours_tree: &IndexMap<String, Tree>,
    theirs: &dyn TableLike,
) {
    for key in ours_tree.keys().filter(|k| !result.contains_key(*k)) {
        ours.remove(key);
    }
    for (key, want) in result {
        let Some(have) = ours_tree.get(key) else {
            if let Some(item) = theirs.get(key) {
                ours.insert(key, item.clone());
            }
            continue;
        };
        if want == have {
            continue;
        }
        let Some(theirs_item) = theirs.get(key) else {
            continue;
        };
        if let (Tree::Map(want), Tree::Map(have)) = (want, have)
            && let Some(sub) = ours.get_mut(key).and_then(|i| i.as_table_like_mut())
            && let Some(theirs_sub) = theirs_item.as_table_like()
        {
            apply(sub, want, have, theirs_sub);
            continue;
        }
        let mut new = theirs_item.clone();
        if let (Some(Item::Value(old)), Item::Value(v)) = (ours.get(key), &mut new) {
            *v.decor_mut() = old.decor().clone();
        }
        ours.insert(key, new);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_comment_survives_replacement() {
        let m = merge_toml("a = 1\n", "a = 1 # keep\nb = 1\n", "a = 2\n").unwrap();
        assert_eq!(m.text, "a = 2 # keep\nb = 1\n");
    }

    #[test]
    fn template_deletes_unchanged_key_and_adds_table() {
        let b = "a = 1\nold = 1\n";
        let o = "# top\na = 1\nold = 1\n";
        let t = "a = 1\n\n[new]\nx = 1\n";
        let m = merge_toml(b, o, t).unwrap();
        assert!(m.conflicts.is_empty());
        assert_eq!(m.text, "# top\na = 1\n\n[new]\nx = 1\n");
    }

    #[test]
    fn nested_table_changes_merge() {
        let b = "[p]\na = 1\n[p.q]\nx = 1\n";
        let o = "[p]\na = 2\n[p.q]\nx = 1\n";
        let t = "[p]\na = 1\n[p.q]\nx = 5\ny = 6\n";
        let m = merge_toml(b, o, t).unwrap();
        assert!(m.conflicts.is_empty());
        assert_eq!(m.text, "[p]\na = 2\n[p.q]\nx = 5\ny = 6\n");
    }

    #[test]
    fn invalid_toml_is_a_parse_error() {
        assert!(merge_toml("a = 1\n", "a = \n", "a = 1\n").is_err());
    }
}
