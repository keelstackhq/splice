//! TOML merge: the merged tree decides which keys differ, then ours' `DocumentMut` is edited
//! in place so comments and layout survive.

use super::keypath::{Leaf, Tree, merge_tree};
use super::{Merged, ParseError};
use toml_edit::{DocumentMut, Item, TableLike};

fn leaf(item: &Item) -> Tree {
    // Compare values semantically (not by whitespace): re-parse the rendered value.
    let v = item.clone().into_value().map(|v| v.to_string());
    let parsed = v
        .ok()
        .and_then(|s| format!("v = {s}").parse::<toml::Table>().ok())
        .and_then(|mut t| t.remove("v"));
    Tree::Leaf(Leaf::Toml(
        parsed.unwrap_or(toml::Value::String(String::new())),
    ))
}

fn to_tree(item: &Item) -> Tree {
    match item.as_table_like() {
        Some(t) => Tree::Map(t.iter().map(|(k, v)| (k.to_string(), to_tree(v))).collect()),
        None => leaf(item),
    }
}

fn parse(s: &str) -> Result<DocumentMut, ParseError> {
    s.parse::<DocumentMut>()
        .map_err(|e| ParseError(e.to_string()))
}

/// Make `ours` equal to `merged`, taking changed or new items from `theirs`.
fn apply(ours: &mut dyn TableLike, merged: &IndexMap<String, Tree>, theirs: &dyn TableLike) {
    let gone: Vec<String> = ours
        .iter()
        .map(|(k, _)| k.to_string())
        .filter(|k| !merged.contains_key(k))
        .collect();
    for k in gone {
        ours.remove(&k);
    }
    for (k, mt) in merged {
        let Some(item) = ours.get_mut(k) else {
            if let Some(t) = theirs.get(k) {
                ours.insert(k, t.clone());
            }
            continue;
        };
        let cur = to_tree(item);
        if &cur == mt {
            continue;
        }
        if let (Tree::Map(sub), Some(child), Some(tchild)) = (
            mt,
            item.as_table_like_mut(),
            theirs.get(k).and_then(Item::as_table_like),
        ) {
            apply(child, sub, tchild);
            continue;
        }
        if let Some(t) = theirs.get(k) {
            let mut new = t.clone();
            if let (Item::Value(old), Item::Value(nv)) = (&*item, &mut new) {
                *nv.decor_mut() = old.decor().clone();
            }
            *item = new;
        }
    }
}

use indexmap::IndexMap;

pub fn merge_toml(base: &str, ours: &str, theirs: &str) -> Result<Merged, ParseError> {
    let (b, mut o, t) = (parse(base)?, parse(ours)?, parse(theirs)?);
    let (bt, ot, tt) = (
        to_tree(b.as_item()),
        to_tree(o.as_item()),
        to_tree(t.as_item()),
    );
    let mut conflicts = Vec::new();
    let merged = merge_tree(&bt, &ot, &tt, &mut Vec::new(), &mut conflicts);
    if conflicts.is_empty()
        && let Tree::Map(m) = &merged
    {
        apply(o.as_table_mut(), m, t.as_table());
    }
    Ok(Merged {
        text: o.to_string(),
        conflicts,
    })
}
