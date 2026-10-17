//! YAML merge on `yaml_serde`. Multi-document files and anything that does not map onto JSON
//! values (tags, non-string keys) are parse errors, so `merge3` falls back to the text merge.
//! Comments and layout are lost on a real merge; indentation is yaml_serde's two spaces.

use serde_json::Value;

use super::keypath::{Tree, merge_tree};
use super::{Merged, ParseError};

pub fn merge_yaml(base: &str, ours: &str, theirs: &str) -> Result<Merged, ParseError> {
    let b = if base.trim().is_empty() {
        Tree::empty_map()
    } else {
        parse(base)?
    };
    let (o, t) = (parse(ours)?, parse(theirs)?);
    let mut conflicts = Vec::new();
    let merged = merge_tree(&b, &o, &t, &mut Vec::new(), &mut conflicts).into_value();
    let mut text = yaml_serde::to_string(&merged).map_err(|e| ParseError(e.to_string()))?;
    let seq = sequence_indent(ours);
    if seq > 0 {
        text = indent_sequences(&text, seq);
    }
    if !ours.ends_with('\n') {
        text.truncate(text.trim_end_matches('\n').len());
    }
    if ours.contains("\r\n") {
        text = text.replace('\n', "\r\n");
    }
    Ok(Merged { text, conflicts })
}

/// Parse one document into a tree. Every mapping key must be a string: yaml_serde would
/// otherwise coerce `1: a` to `"1"` and the merge would rewrite the user's key.
fn parse(s: &str) -> Result<Tree, ParseError> {
    let err = |e: yaml_serde::Error| ParseError(e.to_string());
    let strict = yaml_serde::from_str::<yaml_serde::Value>(s).map_err(err)?;
    if !string_keys(&strict) {
        return Err(ParseError("non-string mapping key".to_string()));
    }
    yaml_serde::from_str::<Value>(s)
        .map(Tree::from_value)
        .map_err(err)
}

fn string_keys(value: &yaml_serde::Value) -> bool {
    match value {
        yaml_serde::Value::Mapping(m) => m.iter().all(|(k, v)| k.is_string() && string_keys(v)),
        yaml_serde::Value::Sequence(s) => s.iter().all(string_keys),
        _ => true,
    }
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

/// How far ours indents a sequence under its key: 0 for `key:\n- a`, 2 for `key:\n  - a`.
fn sequence_indent(ours: &str) -> usize {
    let lines: Vec<&str> = ours.lines().collect();
    lines
        .windows(2)
        .find_map(|w| {
            let key = w[0].trim_end();
            let is_key = key.ends_with(':') && !key.trim_start().starts_with(['#', '-']);
            let next = w[1].trim_start();
            (is_key && (next.starts_with("- ") || next == "-"))
                .then(|| indent_of(w[1]).saturating_sub(indent_of(w[0])))
        })
        .unwrap_or(0)
}

/// Shift every sequence that sits flush under its key (yaml_serde's style) right by `by`.
fn indent_sequences(text: &str, by: usize) -> String {
    let lines: Vec<String> = text.split_inclusive('\n').map(str::to_string).collect();
    shift(&lines, by).concat()
}

fn shift(lines: &[String], by: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let line = &lines[i];
        out.push(line.clone());
        i += 1;
        let key = line.trim_end();
        if !key.ends_with(':') || key.trim_start().starts_with('#') {
            continue;
        }
        let col = indent_of(line)
            + if key.trim_start().starts_with("- ") {
                2
            } else {
                0
            };
        let is_item = |l: &String| indent_of(l) == col && l.trim_start().starts_with('-');
        if !lines.get(i).is_some_and(is_item) {
            continue;
        }
        let len = lines[i..]
            .iter()
            .take_while(|l| indent_of(l) > col || is_item(l))
            .count();
        for l in shift(&lines[i..i + len], by) {
            out.push(format!("{}{l}", " ".repeat(by)));
        }
        i += len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_by_key_path() {
        let m = merge_yaml("a: 1\nb: 1\n", "a: 2\nb: 1\n", "a: 1\nb: 3\n").unwrap();
        assert_eq!(m.text, "a: 2\nb: 3\n");
        assert!(m.conflicts.is_empty());
    }

    #[test]
    fn multi_document_is_a_parse_error() {
        assert!(merge_yaml("a: 1\n", "---\na: 1\n---\nb: 2\n", "a: 1\n").is_err());
    }

    #[test]
    fn sequences_are_values() {
        let m = merge_yaml("s: [1]\n", "s: [1, 2]\n", "s: [1, 3]\n").unwrap();
        assert_eq!(m.conflicts, ["s"]);
    }
}
