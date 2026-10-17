//! Merging of shared files: text and the structured formats (json, yaml, toml).

pub mod json;
pub mod keypath;
pub mod text;
pub mod toml;
pub mod yaml;

pub use text::{merge_text, wrap_whole};

use crate::manifest::MergeStrategy;
use crate::markers::find_markers;

/// The result of a merge. `conflicts` holds one entry per conflict: a key path for structured
/// formats, `line N` (1-based, in `text`) for text. A conflicted file keeps diff3 markers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merged {
    pub text: String,
    pub conflicts: Vec<String>,
}

/// A side of a structured merge that does not parse; `merge3` falls back to the text merge.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ParseError(pub String);

/// 3-way merge of a shared file by its `[merge]` strategy (contract §5, spec §8.5).
///
/// Fast paths first: ours == base takes theirs verbatim, theirs == base (or == ours) keeps ours
/// verbatim. A side that does not parse falls back to the text merge. Structured conflicts end as
/// diff3 markers: the text merge if it produced some, else the whole file in one block, so
/// `check` always finds them. `conflicts` then still lists the key paths.
pub fn merge3(strategy: MergeStrategy, base: &str, ours: &str, theirs: &str) -> Merged {
    if ours == base {
        return clean(theirs);
    }
    if theirs == base || theirs == ours {
        return clean(ours);
    }
    // json and yaml: a clean line merge that still parses keeps the user's layout and comments.
    if matches!(strategy, MergeStrategy::Json | MergeStrategy::Yaml) {
        let m = merge_text(base, ours, theirs);
        if m.conflicts.is_empty() && parses(strategy, &m.text) {
            return m;
        }
    }
    let structured = match strategy {
        MergeStrategy::Text => return merge_text(base, ours, theirs),
        MergeStrategy::Json => json::merge_json(base, ours, theirs),
        MergeStrategy::Yaml => yaml::merge_yaml(base, ours, theirs),
        MergeStrategy::Toml => toml::merge_toml(base, ours, theirs),
    };
    let Ok(merged) = structured else {
        return merge_text(base, ours, theirs);
    };
    if merged.conflicts.is_empty() {
        return merged;
    }
    let text = merge_text(base, ours, theirs).text;
    let text = if find_markers(&text).is_empty() {
        wrap_whole(base, ours, theirs)
    } else {
        text
    };
    Merged {
        text,
        conflicts: merged.conflicts,
    }
}

fn parses(strategy: MergeStrategy, text: &str) -> bool {
    match strategy {
        MergeStrategy::Json => json::parses_strict(text),
        MergeStrategy::Yaml => yaml_serde::from_str::<yaml_serde::Value>(text).is_ok(),
        _ => true,
    }
}

fn clean(text: &str) -> Merged {
    Merged {
        text: text.to_string(),
        conflicts: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_both_sides_kept_order_and_indent() {
        let b = "{\n  \"name\": \"x\",\n  \"dependencies\": {}\n}\n";
        let o = "{\n  \"name\": \"x\",\n  \"dependencies\": { \"left-pad\": \"1\" }\n}\n";
        let t = "{\n  \"name\": \"x\",\n  \"dependencies\": {},\n  \"scripts\": { \"lint\": \"oxlint\" }\n}\n";
        let m = merge3(MergeStrategy::Json, b, o, t);
        assert!(m.conflicts.is_empty());
        assert_eq!(
            m.text,
            "{\n  \"name\": \"x\",\n  \"dependencies\": {\n    \"left-pad\": \"1\"\n  },\n  \"scripts\": {\n    \"lint\": \"oxlint\"\n  }\n}\n"
        );
    }

    #[test]
    fn json_tabs_kept() {
        let b = "{\n\t\"a\": {}\n}\n";
        let o = "{\n\t\"a\": {\"x\": 1}\n}\n";
        let t = "{\n\t\"a\": {},\n\t\"b\": 2\n}\n";
        let m = merge3(MergeStrategy::Json, b, o, t);
        assert!(m.conflicts.is_empty());
        assert_eq!(m.text, "{\n\t\"a\": {\n\t\t\"x\": 1\n\t},\n\t\"b\": 2\n}\n");
    }

    #[test]
    fn json_no_final_newline_kept() {
        let m = merge3(
            MergeStrategy::Json,
            "{\"a\":1}",
            "{\"a\":1,\"u\":1}",
            "{\"a\":1,\"t\":1}",
        );
        assert_eq!(m.text, "{\n  \"a\": 1,\n  \"u\": 1,\n  \"t\": 1\n}");
    }

    #[test]
    fn arrays_are_values() {
        let m = merge3(
            MergeStrategy::Json,
            r#"{"a":[1]}"#,
            r#"{"a":[1,2]}"#,
            r#"{"a":[1,3]}"#,
        );
        assert_eq!(m.conflicts, ["a"]);
        assert!(!find_markers(&m.text).is_empty());
    }

    #[test]
    fn delete_vs_change_conflicts() {
        let m = merge3(
            MergeStrategy::Json,
            "{\n  \"a\": 1,\n  \"b\": 1\n}\n",
            "{\n  \"a\": 1\n}\n",
            "{\n  \"a\": 1,\n  \"b\": 2\n}\n",
        );
        assert_eq!(m.conflicts, ["b"]);
        assert!(!find_markers(&m.text).is_empty());
    }

    #[test]
    fn delete_by_template_only() {
        let m = merge3(
            MergeStrategy::Json,
            "{\n  \"a\": 1,\n  \"b\": 1\n}\n",
            "{\n  \"a\": 5,\n  \"b\": 1\n}\n",
            "{\n  \"a\": 1\n}\n",
        );
        assert!(m.conflicts.is_empty());
        assert_eq!(m.text, "{\n  \"a\": 5\n}\n");
    }

    #[test]
    fn nested_conflict_reports_dotted_path() {
        let m = merge3(
            MergeStrategy::Json,
            r#"{"a":{"b":{"c":1}}}"#,
            r#"{"a":{"b":{"c":2}}}"#,
            r#"{"a":{"b":{"c":3}}}"#,
        );
        assert_eq!(m.conflicts, ["a.b.c"]);
    }

    #[test]
    fn same_change_on_both_sides_is_not_a_conflict() {
        let m = merge3(
            MergeStrategy::Json,
            "{\"a\":1,\"z\":0}",
            "{\"a\":2,\"z\":0}",
            "{\"a\":2,\"z\":9}",
        );
        assert!(m.conflicts.is_empty());
        assert_eq!(m.text, "{\n  \"a\": 2,\n  \"z\": 9\n}");
    }

    #[test]
    fn fast_paths_return_a_side_verbatim() {
        // ours == base: theirs verbatim, even when nothing parses.
        let m = merge3(MergeStrategy::Json, "junk", "junk", "{ \"a\":1 }");
        assert_eq!(m.text, "{ \"a\":1 }");
        // theirs == base: ours verbatim.
        let m = merge3(
            MergeStrategy::Toml,
            "x = 1\n",
            "# mine\nx   = 2\n",
            "x = 1\n",
        );
        assert_eq!(m.text, "# mine\nx   = 2\n");
        assert!(m.conflicts.is_empty());
    }

    #[test]
    fn toml_keeps_comments() {
        let b = "[build]\ntarget = \"es2022\"\n";
        let o = "# mine\n[build]\ntarget = \"es2022\"\n\n[local]\nx = 1\n";
        let t = "[build]\ntarget = \"es2024\"\n";
        assert_eq!(
            merge3(MergeStrategy::Toml, b, o, t).text,
            "# mine\n[build]\ntarget = \"es2024\"\n\n[local]\nx = 1\n"
        );
    }

    #[test]
    fn toml_conflict_has_markers() {
        let m = merge3(
            MergeStrategy::Toml,
            "[build]\ntarget = \"a\"\n",
            "[build]\ntarget = \"b\"\n",
            "[build]\ntarget = \"c\"\n",
        );
        assert_eq!(m.conflicts, ["build.target"]);
        assert!(!find_markers(&m.text).is_empty());
    }

    #[test]
    fn yaml_same_key_conflict_has_markers() {
        let b = "jobs:\n  ci:\n    runs-on: ubuntu-22.04\n";
        let o = "jobs:\n  ci:\n    runs-on: ubuntu-24.04\n";
        let t = "jobs:\n  ci:\n    runs-on: macos-latest\n";
        let m = merge3(MergeStrategy::Yaml, b, o, t);
        assert_eq!(m.conflicts, ["jobs.ci.runs-on"]);
        assert!(m.text.contains("<<<<<<< ours"));
        assert!(!find_markers(&m.text).is_empty());
    }

    #[test]
    fn yaml_disjoint_changes_merge() {
        let b = "a: 1\nb:\n  c: 1\n";
        let o = "a: 1\nb:\n  c: 1\n  d: 2\n";
        let t = "a: 9\nb:\n  c: 1\n";
        let m = merge3(MergeStrategy::Yaml, b, o, t);
        assert!(m.conflicts.is_empty());
        assert_eq!(m.text, "a: 9\nb:\n  c: 1\n  d: 2\n");
    }

    #[test]
    fn yaml_multi_document_falls_back_to_text() {
        let b = "---\na: 1\n---\nb: 2\n";
        let o = "---\na: 1\n---\nb: 2\nc: 3\n";
        let t = "---\na: 9\n---\nb: 2\n";
        let m = merge3(MergeStrategy::Yaml, b, o, t);
        assert!(m.conflicts.is_empty());
        assert_eq!(m.text, "---\na: 9\n---\nb: 2\nc: 3\n");
    }

    #[test]
    fn invalid_json_falls_back_to_text() {
        let b = "{\n  \"a\": 1,\n  \"b\": 1,\n  \"c\": 1,\n  \"d\": 1,\n  \"e\": 1\n}\n";
        let o = "{\n  \"a\": 5,\n  \"b\": 1,\n  \"c\": 1,\n  \"d\": 1,\n  \"e\": 1,\n}\n";
        let t = "{\n  \"a\": 1,\n  \"b\": 1,\n  \"c\": 9,\n  \"d\": 1,\n  \"e\": 1\n}\n";
        let m = merge3(MergeStrategy::Json, b, o, t);
        assert_eq!(
            m.text,
            "{\n  \"a\": 5,\n  \"b\": 1,\n  \"c\": 9,\n  \"d\": 1,\n  \"e\": 1,\n}\n"
        );
    }

    #[test]
    fn text_strategy_is_the_text_merge() {
        let m = merge3(MergeStrategy::Text, "a\nb\nc\n", "A\nb\nc\n", "a\nb\nC\n");
        assert_eq!(m.text, "A\nb\nC\n");
    }

    const WORKSPACE: &str = include_str!("../../tests/fixtures/merge/pnpm-workspace.yaml");

    fn after(text: &str, anchor: &str, add: &str) -> String {
        text.replacen(anchor, &format!("{anchor}\n{add}"), 1)
    }

    #[test]
    fn hull_workspace_user_pin_and_template_bump_keep_comments_and_indent() {
        let ours = after(WORKSPACE, "  wrangler: 4.151.0", "  zod: 3.0.0");
        let theirs = WORKSPACE.replacen("hono: 4.13.13", "hono: 4.13.14", 1);
        let m = merge3(MergeStrategy::Yaml, WORKSPACE, &ours, &theirs);
        assert!(m.conflicts.is_empty());
        let want = after(&theirs, "  wrangler: 4.151.0", "  zod: 3.0.0");
        assert_eq!(m.text, want);
        assert!(m.text.contains("# @cascivo/react peers on"));
    }

    #[test]
    fn hull_workspace_overlapping_edits_use_structured_path_with_ours_indent() {
        let ours = after(WORKSPACE, "  wrangler: 4.151.0", "  zod: 3.0.0");
        let theirs = after(WORKSPACE, "  wrangler: 4.151.0", "  zstd: 1.0.0");
        let m = merge3(MergeStrategy::Yaml, WORKSPACE, &ours, &theirs);
        assert!(m.conflicts.is_empty(), "{:?}", m.conflicts);
        assert!(find_markers(&m.text).is_empty());
        assert!(
            m.text
                .starts_with("packages:\n  - apps/*\n  - packages/*\n")
        );
        assert!(m.text.contains("\n  zod: 3.0.0\n"));
        assert!(m.text.contains("\n  zstd: 1.0.0\n"));
        assert!(m.text.contains("\n  ignoreMissing:\n    - "));
    }

    #[test]
    fn flush_sequences_in_ours_stay_flush() {
        let m = merge3(
            MergeStrategy::Yaml,
            "s:\n- a\nk: 1\n",
            "s:\n- a\nk: 1\nx: 1\n",
            "s:\n- a\nk: 1\ny: 1\n",
        );
        assert!(m.conflicts.is_empty());
        assert_eq!(m.text, "s:\n- a\nk: 1\nx: 1\ny: 1\n");
    }

    #[test]
    fn empty_base_json_merges_per_key() {
        let m = merge3(MergeStrategy::Json, "", "{\"a\":1}", "{\"b\":2}");
        assert!(m.conflicts.is_empty());
        assert_eq!(m.text, "{\n  \"a\": 1,\n  \"b\": 2\n}");
    }

    #[test]
    fn empty_base_yaml_merges_per_key() {
        let m = merge3(MergeStrategy::Yaml, "", "a: 1\n", "b: 2\n");
        assert!(m.conflicts.is_empty());
        assert_eq!(m.text, "a: 1\nb: 2\n");
    }

    #[test]
    fn yaml_non_string_key_is_not_coerced() {
        assert!(yaml::merge_yaml("1: a\n", "1: a\nx: 1\n", "1: b\n").is_err());
        let m = merge3(
            MergeStrategy::Yaml,
            "1: a\nz: 1\n",
            "1: a\nz: 2\n",
            "1: b\nz: 1\n",
        );
        assert!(!m.text.contains("'1'") && !m.text.contains("\"1\""));
    }

    #[test]
    fn text_clean_json_merge_never_duplicates_a_key() {
        let b = "{\n  \"a\": 1,\n  \"b\": 2,\n  \"c\": 3,\n  \"d\": 4\n}\n";
        let o = "{\n  \"a\": 1,\n  \"x\": 1,\n  \"b\": 2,\n  \"c\": 3,\n  \"d\": 4\n}\n";
        let t = "{\n  \"a\": 1,\n  \"b\": 2,\n  \"c\": 3,\n  \"d\": 4,\n  \"x\": 2\n}\n";
        let m = merge3(MergeStrategy::Json, b, o, t);
        assert_eq!(m.conflicts, ["x"]);
        assert!(!find_markers(&m.text).is_empty());
        let mut inside = false;
        let outside: Vec<&str> = m
            .text
            .lines()
            .filter(|l| {
                if l.starts_with("<<<<<<< ") {
                    inside = true;
                }
                let keep = !inside;
                if l.starts_with(">>>>>>> ") {
                    inside = false;
                }
                keep
            })
            .collect();
        assert!(outside.iter().filter(|l| l.contains("\"x\"")).count() <= 1);
    }

    #[test]
    fn input_with_a_duplicate_key_never_merges_cleanly() {
        let b = "{\n  \"a\": 1,\n  \"b\": 2\n}\n";
        let o = "{\n  \"a\": 1,\n  \"a\": 5,\n  \"b\": 2\n}\n";
        let t = "{\n  \"a\": 1,\n  \"b\": 2,\n  \"c\": 3\n}\n";
        assert!(json::merge_json(b, o, t).is_err());
        let m = merge3(MergeStrategy::Json, b, o, t);
        assert!(m.text.contains("\"a\": 1,") && m.text.contains("\"a\": 5,"));
    }

    #[test]
    fn duplicate_keys_are_rejected_at_any_depth() {
        assert!(json::parses_strict(r#"{"a":[{"b":1}],"c":{"d":1}}"#));
        assert!(!json::parses_strict(r#"{"a":1,"a":2}"#));
        assert!(!json::parses_strict(r#"{"a":[{"b":1,"b":2}]}"#));
        assert!(!json::parses_strict(r#"{"a":{"b":{"c":1,"c":1}}}"#));
    }
}
