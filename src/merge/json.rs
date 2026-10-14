//! JSON merge: `serde_json` with `preserve_order`, indent detected from ours.

use super::keypath::{Leaf, Tree, merge_tree};
use super::{Merged, ParseError};
use serde::Serialize;
use serde_json::Value;

fn to_tree(v: &Value) -> Tree {
    match v {
        Value::Object(m) => Tree::Map(m.iter().map(|(k, v)| (k.clone(), to_tree(v))).collect()),
        v => Tree::Leaf(Leaf::Json(v.clone())),
    }
}

fn from_tree(t: &Tree) -> Value {
    match t {
        Tree::Map(m) => Value::Object(m.iter().map(|(k, v)| (k.clone(), from_tree(v))).collect()),
        Tree::Leaf(Leaf::Json(v)) => v.clone(),
        Tree::Leaf(_) => unreachable!("json tree holds only json leaves"),
    }
}

fn parse(s: &str) -> Result<Tree, ParseError> {
    serde_json::from_str::<Value>(s)
        .map(|v| to_tree(&v))
        .map_err(|e| ParseError(e.to_string()))
}

/// The indent unit of the first indented line: a tab, or that many spaces (default 2).
fn detect_indent(s: &str) -> String {
    for line in s.lines() {
        let ws: String = line
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        if ws.is_empty() || ws.len() == line.len() {
            continue;
        }
        return if ws.starts_with('\t') {
            "\t".into()
        } else {
            ws
        };
    }
    "  ".into()
}

pub fn merge_json(base: &str, ours: &str, theirs: &str) -> Result<Merged, ParseError> {
    let (b, o, t) = (parse(base)?, parse(ours)?, parse(theirs)?);
    let mut conflicts = Vec::new();
    let merged = merge_tree(&b, &o, &t, &mut Vec::new(), &mut conflicts);

    let indent = detect_indent(ours);
    let mut buf = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
    from_tree(&merged)
        .serialize(&mut ser)
        .map_err(|e| ParseError(e.to_string()))?;
    let mut text = String::from_utf8(buf).map_err(|e| ParseError(e.to_string()))?;
    if ours.ends_with('\n') {
        text.push('\n');
    }
    Ok(Merged { text, conflicts })
}
