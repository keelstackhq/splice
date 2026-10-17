//! YAML merge on `yaml_serde`. Multi-document files and anything that does not map onto JSON
//! values (tags, non-string keys) are parse errors, so `merge3` falls back to the text merge.
//! Comments and layout are lost on a real merge; indentation is yaml_serde's two spaces.

use serde_json::Value;

use super::keypath::{Tree, merge_tree};
use super::{Merged, ParseError};

pub fn merge_yaml(base: &str, ours: &str, theirs: &str) -> Result<Merged, ParseError> {
    let parse = |s: &str| {
        yaml_serde::from_str::<Value>(s)
            .map(Tree::from_value)
            .map_err(|e| ParseError(e.to_string()))
    };
    let (b, o, t) = (parse(base)?, parse(ours)?, parse(theirs)?);
    let mut conflicts = Vec::new();
    let merged = merge_tree(&b, &o, &t, &mut Vec::new(), &mut conflicts).into_value();
    let mut text = yaml_serde::to_string(&merged).map_err(|e| ParseError(e.to_string()))?;
    if !ours.ends_with('\n') {
        text.truncate(text.trim_end_matches('\n').len());
    }
    if ours.contains("\r\n") {
        text = text.replace('\n', "\r\n");
    }
    Ok(Merged { text, conflicts })
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
