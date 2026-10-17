//! JSON merge: key-path 3-way merge, re-serialised with ours' indentation.

use serde::Serialize;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashSet};

use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use serde_json::ser::{PrettyFormatter, Serializer};

use super::keypath::{Tree, merge_tree};
use super::{Merged, ParseError};

/// True when `text` is one JSON value without a repeated object key at any depth.
pub fn parses_strict(text: &str) -> bool {
    duplicate_keys(text).is_some_and(|d| d.is_empty())
}

/// Dotted paths of object keys that repeat within one object (array positions are not part of
/// the path), sorted; `None` when `text` is not one JSON value.
pub fn duplicate_keys(text: &str) -> Option<Vec<String>> {
    let found = RefCell::new(BTreeSet::new());
    let seed = Seed {
        path: String::new(),
        found: &found,
    };
    let mut de = serde_json::Deserializer::from_str(text);
    seed.deserialize(&mut de).ok()?;
    de.end().ok()?;
    Some(found.into_inner().into_iter().collect())
}

struct Seed<'a> {
    path: String,
    found: &'a RefCell<BTreeSet<String>>,
}

impl<'de> DeserializeSeed<'de> for Seed<'_> {
    type Value = ();

    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Seed<'_> {
    type Value = ();

    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_bool<E>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E>(self, _: f64) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E>(self, _: &str) -> Result<(), E> {
        Ok(())
    }
    fn visit_unit<E>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        while seq
            .next_element_seed(Seed {
                path: self.path.clone(),
                found: self.found,
            })?
            .is_some()
        {}
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut seen = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            let path = if self.path.is_empty() {
                key.clone()
            } else {
                format!("{}.{key}", self.path)
            };
            if !seen.insert(key) {
                self.found.borrow_mut().insert(path.clone());
            }
            map.next_value_seed(Seed {
                path,
                found: self.found,
            })?;
        }
        Ok(())
    }
}

pub fn merge_json(base: &str, ours: &str, theirs: &str) -> Result<Merged, ParseError> {
    let parse = |s: &str| {
        if !parses_strict(s) {
            return Err(ParseError("invalid JSON or duplicate key".to_string()));
        }
        serde_json::from_str::<Value>(s)
            .map(Tree::from_value)
            .map_err(|e| ParseError(e.to_string()))
    };
    let b = if base.trim().is_empty() {
        Tree::empty_map()
    } else {
        parse(base)?
    };
    let (o, t) = (parse(ours)?, parse(theirs)?);
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
