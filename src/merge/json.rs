//! JSON merge: key-path 3-way merge, re-serialised with ours' indentation.

use serde::Serialize;
use serde_json::Value;
use serde_json::ser::{PrettyFormatter, Serializer};

use super::keypath::{Tree, merge_tree};
use super::{Merged, ParseError};

pub fn merge_json(base: &str, ours: &str, theirs: &str) -> Result<Merged, ParseError> {
    let parse = |s: &str| {
        serde_json::from_str::<Value>(s)
            .map(Tree::from_value)
            .map_err(|e| ParseError(e.to_string()))
    };
    let (b, o, t) = (parse(base)?, parse(ours)?, parse(theirs)?);
    let mut conflicts = Vec::new();
    let merged = merge_tree(&b, &o, &t, &mut Vec::new(), &mut conflicts).into_value();
    Ok(Merged {
        text: print(&merged, ours),
        conflicts,
    })
}

/// Pretty-print with the indent detected from `ours` (tabs or N spaces, default 2), keeping its
/// final newline and CRLF line endings.
fn print(value: &Value, ours: &str) -> String {
    let indent = detect_indent(ours);
    let mut buf = Vec::new();
    let formatter = PrettyFormatter::with_indent(indent.as_bytes());
    let mut ser = Serializer::with_formatter(&mut buf, formatter);
    value.serialize(&mut ser).expect("a JSON value serialises");
    let mut text = String::from_utf8(buf).expect("serde_json writes UTF-8");
    if ours.ends_with('\n') {
        text.push('\n');
    }
    if ours.contains("\r\n") {
        text = text.replace('\n', "\r\n");
    }
    text
}

fn detect_indent(ours: &str) -> String {
    ours.lines()
        .find(|l| l.starts_with(' ') || l.starts_with('\t'))
        .map(|l| {
            let first = l.chars().next().unwrap();
            l.chars().take_while(|&c| c == first).collect()
        })
        .unwrap_or_else(|| "  ".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indent_detection() {
        assert_eq!(detect_indent("{\n    \"a\": 1\n}\n"), "    ");
        assert_eq!(detect_indent("{\n\t\"a\": 1\n}\n"), "\t");
        assert_eq!(detect_indent("{\"a\":1}"), "  ");
    }

    #[test]
    fn four_space_indent_kept() {
        let m = merge_json("{\"a\":1}", "{\n    \"a\": 2\n}\n", "{\"a\":1,\"b\":1}").unwrap();
        assert_eq!(m.text, "{\n    \"a\": 2,\n    \"b\": 1\n}\n");
    }

    #[test]
    fn invalid_side_is_a_parse_error() {
        assert!(merge_json("{}", "{,}", "{}").is_err());
    }
}
