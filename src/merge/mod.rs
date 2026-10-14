//! Three-way merges. One module per strategy.

pub mod json;
pub mod keypath;
pub mod text;
pub mod toml;
pub mod yaml;

use crate::manifest::MergeStrategy;

/// Result of a merge: the merged text and a description of each conflict
/// (a key path, or `line N` for text conflicts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merged {
    pub text: String,
    pub conflicts: Vec<String>,
}

/// A side could not be parsed in the chosen format; the caller falls back to text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

/// Three-way merge of `ours` and `theirs` against `base` with the file's strategy.
pub fn merge3(strategy: MergeStrategy, base: &str, ours: &str, theirs: &str) -> Merged {
    // Fast paths: a side that did not change takes the other verbatim.
    if ours == base {
        return clean(theirs);
    }
    if theirs == base || ours == theirs {
        return clean(ours);
    }
    // Like merge_text: ours decides the line ending; merge in LF, restore CRLF after.
    let crlf = ours.contains("\r\n");
    let lf = |s: &str| s.replace("\r\n", "\n");
    let (b, o, t) = if crlf {
        (lf(base), lf(ours), lf(theirs))
    } else {
        (base.to_string(), ours.to_string(), theirs.to_string())
    };
    let structured = match strategy {
        MergeStrategy::Json => json::merge_json(&b, &o, &t),
        MergeStrategy::Yaml => yaml::merge_yaml(&b, &o, &t),
        MergeStrategy::Toml => toml::merge_toml(&b, &o, &t),
        MergeStrategy::Text => Err(ParseError("text strategy".into())),
    };
    match structured {
        Ok(m) if m.conflicts.is_empty() => Merged {
            text: if crlf {
                m.text.replace('\n', "\r\n")
            } else {
                m.text
            },
            conflicts: m.conflicts,
        },
        Ok(m) => {
            // Structured conflict: leave diff3 markers `check` can find.
            let t = text::merge_text(base, ours, theirs);
            let text = if t.conflicts.is_empty() {
                text::wrap_whole(base, ours, theirs)
            } else {
                t.text
            };
            Merged {
                text,
                conflicts: m.conflicts,
            }
        }
        Err(_) => text::merge_text(base, ours, theirs),
    }
}

fn clean(text: &str) -> Merged {
    Merged {
        text: text.to_string(),
        conflicts: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markers::find_markers;

    fn tabs(s: &str) -> String {
        s.replace("  ", "\t")
    }

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
    fn json_package_json_user_adds_template_bumps() {
        let b = "{\n  \"name\": \"x\",\n  \"devDependencies\": {\n    \"typescript\": \"5.0\",\n    \"vite\": \"5\"\n  }\n}\n";
        let o = "{\n  \"zeta\": 1,\n  \"name\": \"x\",\n  \"dependencies\": {\n    \"left-pad\": \"1\"\n  },\n  \"devDependencies\": {\n    \"typescript\": \"5.0\",\n    \"vite\": \"5\"\n  }\n}\n";
        let t = "{\n  \"name\": \"x\",\n  \"devDependencies\": {\n    \"typescript\": \"5.5\",\n    \"vite\": \"5\"\n  },\n  \"scripts\": {\n    \"lint\": \"oxlint\"\n  }\n}\n";
        let m = merge3(MergeStrategy::Json, b, o, t);
        assert!(m.conflicts.is_empty(), "{:?}", m.conflicts);
        assert_eq!(
            m.text,
            "{\n  \"zeta\": 1,\n  \"name\": \"x\",\n  \"dependencies\": {\n    \"left-pad\": \"1\"\n  },\n  \"devDependencies\": {\n    \"typescript\": \"5.5\",\n    \"vite\": \"5\"\n  },\n  \"scripts\": {\n    \"lint\": \"oxlint\"\n  }\n}\n"
        );
    }

    #[test]
    fn json_tabs_kept() {
        let b = tabs("{\n  \"a\": 1,\n  \"b\": {\n    \"c\": 1\n  }\n}\n");
        let o = tabs("{\n  \"a\": 2,\n  \"b\": {\n    \"c\": 1\n  }\n}\n");
        let t = tabs("{\n  \"a\": 1,\n  \"b\": {\n    \"c\": 3\n  }\n}\n");
        let m = merge3(MergeStrategy::Json, &b, &o, &t);
        assert!(m.conflicts.is_empty());
        assert_eq!(
            m.text,
            tabs("{\n  \"a\": 2,\n  \"b\": {\n    \"c\": 3\n  }\n}\n")
        );
    }

    #[test]
    fn json_four_spaces_and_no_final_newline_kept() {
        let b = "{\n    \"a\": 1\n}";
        let o = "{\n    \"a\": 1,\n    \"u\": 1\n}";
        let t = "{\n    \"a\": 2\n}";
        let m = merge3(MergeStrategy::Json, b, o, t);
        assert_eq!(m.text, "{\n    \"a\": 2,\n    \"u\": 1\n}");
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
    fn delete_by_user_only_is_kept() {
        let m = merge3(
            MergeStrategy::Json,
            "{\n  \"a\": 1,\n  \"b\": 1\n}\n",
            "{\n  \"a\": 1\n}\n",
            "{\n  \"a\": 2,\n  \"b\": 1\n}\n",
        );
        assert!(m.conflicts.is_empty());
        assert_eq!(m.text, "{\n  \"a\": 2\n}\n");
    }

    #[test]
    fn same_change_on_both_sides_is_clean() {
        let m = merge3(
            MergeStrategy::Json,
            "{\"a\":1,\"z\":0}",
            "{\"a\":2,\"z\":1}",
            "{\"a\":2,\"z\":0,\"n\":1}",
        );
        assert!(m.conflicts.is_empty());
        assert_eq!(m.text, "{\n  \"a\": 2,\n  \"z\": 1,\n  \"n\": 1\n}");
    }

    #[test]
    fn fast_paths_return_input_verbatim() {
        let b = "{\"a\":1}";
        let o = "{ \"a\" : 1 }";
        let t = "{\"a\":2}";
        // ours == base content-wise is not enough; only byte equality triggers the fast path.
        assert_eq!(merge3(MergeStrategy::Json, b, b, t).text, t);
        assert_eq!(merge3(MergeStrategy::Json, b, o, b).text, o);
        assert_eq!(merge3(MergeStrategy::Json, b, o, o).text, o);
    }

    #[test]
    fn value_type_change_map_vs_scalar_conflicts() {
        let m = merge3(
            MergeStrategy::Json,
            "{\"a\":{\"x\":1}}",
            "{\"a\":{\"x\":2}}",
            "{\"a\":\"s\"}",
        );
        assert_eq!(m.conflicts, ["a"]);
        assert!(!find_markers(&m.text).is_empty());
    }

    #[test]
    fn value_type_change_by_template_only_applies() {
        let m = merge3(
            MergeStrategy::Json,
            "{\"a\":{\"x\":1},\"k\":1}",
            "{\"a\":{\"x\":1},\"k\":2}",
            "{\"a\":\"s\",\"k\":1}",
        );
        assert!(m.conflicts.is_empty());
        assert_eq!(m.text, "{\n  \"a\": \"s\",\n  \"k\": 2\n}");
    }

    #[test]
    fn invalid_json_falls_back_to_text() {
        let b = "{\n  \"a\": 1,\n  \"b\": 1\n}\n";
        let o = "{\n  \"a\": 1,\n  \"b\": 1,\n}\n";
        let t = "{\n  \"a\": 2,\n  \"b\": 1\n}\n";
        let m = merge3(MergeStrategy::Json, b, o, t);
        assert_eq!(m.text, merge_text_ref(b, o, t));
    }

    fn merge_text_ref(b: &str, o: &str, t: &str) -> String {
        text::merge_text(b, o, t).text
    }

    #[test]
    fn yaml_merges_by_key() {
        let b = "jobs:\n  test:\n    runs-on: ubuntu\nname: ci\n";
        let o = "jobs:\n  test:\n    runs-on: ubuntu\nname: mine\n";
        let t = "jobs:\n  test:\n    runs-on: ubuntu-24\n  lint:\n    runs-on: ubuntu\nname: ci\n";
        let m = merge3(MergeStrategy::Yaml, b, o, t);
        assert!(m.conflicts.is_empty(), "{:?}", m.conflicts);
        assert_eq!(
            m.text,
            "jobs:\n  test:\n    runs-on: ubuntu-24\n  lint:\n    runs-on: ubuntu\nname: mine\n"
        );
    }

    #[test]
    fn yaml_same_key_conflict_has_markers() {
        let b = "jobs:\n  test:\n    runs-on: ubuntu\n";
        let o = "jobs:\n  test:\n    runs-on: macos\n";
        let t = "jobs:\n  test:\n    runs-on: ubuntu-24\n";
        let m = merge3(MergeStrategy::Yaml, b, o, t);
        assert_eq!(m.conflicts, ["jobs.test.runs-on"]);
        assert!(m.text.contains("<<<<<<< ours"));
        assert!(m.text.contains(">>>>>>> template"));
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
    fn toml_key_changed_keeps_trailing_comment() {
        let b = "a = 1 # one\nb = 2\n";
        let o = "a = 1 # one\nb = 3 # mine\n";
        let t = "a = 5\nb = 2\n";
        let m = merge3(MergeStrategy::Toml, b, o, t);
        assert!(m.conflicts.is_empty());
        assert_eq!(m.text, "a = 5 # one\nb = 3 # mine\n");
    }

    #[test]
    fn toml_key_added_in_theirs_is_appended() {
        let b = "[build]\ntarget = \"a\"\n\n[x]\ny = 1\n";
        let o = "[build]\ntarget = \"a\"\nmine = true\n\n[x]\ny = 1\n";
        let t = "[build]\ntarget = \"a\"\nminify = true\n\n[x]\ny = 1\n";
        let m = merge3(MergeStrategy::Toml, b, o, t);
        assert!(m.conflicts.is_empty());
        assert_eq!(
            m.text,
            "[build]\ntarget = \"a\"\nmine = true\nminify = true\n\n[x]\ny = 1\n"
        );
    }

    #[test]
    fn toml_key_removed_in_theirs() {
        let b = "a = 1\nb = 2\n";
        let o = "a = 7\nb = 2\n";
        let t = "a = 1\n";
        let m = merge3(MergeStrategy::Toml, b, o, t);
        assert!(m.conflicts.is_empty());
        assert_eq!(m.text, "a = 7\n");
    }

    #[test]
    fn toml_table_added_in_theirs() {
        let b = "[build]\ntarget = \"a\"\n";
        let o = "[build]\ntarget = \"b\"\n";
        let t = "[build]\ntarget = \"a\"\n\n[lint]\nstrict = true\n";
        let m = merge3(MergeStrategy::Toml, b, o, t);
        assert!(m.conflicts.is_empty());
        assert_eq!(m.text, "[build]\ntarget = \"b\"\n\n[lint]\nstrict = true\n");
    }

    #[test]
    fn toml_inline_table_and_dotted_keys() {
        let b = "pkg = { a = 1, b = 2 }\nserver.port = 80\n";
        let o = "pkg = { a = 1, b = 9 }\nserver.port = 80\n";
        let t = "pkg = { a = 5, b = 2 }\nserver.port = 80\nserver.host = \"h\"\n";
        let m = merge3(MergeStrategy::Toml, b, o, t);
        assert!(m.conflicts.is_empty(), "{:?}", m.conflicts);
        assert_eq!(
            m.text,
            "pkg = { a = 5, b = 9 }\nserver.port = 80\nserver.host = \"h\"\n"
        );
    }

    #[test]
    fn toml_same_key_conflict_has_markers() {
        let m = merge3(
            MergeStrategy::Toml,
            "[b]\nt = 1\n",
            "[b]\nt = 2\n",
            "[b]\nt = 3\n",
        );
        assert_eq!(m.conflicts, ["b.t"]);
        assert!(m.text.contains("<<<<<<< ours"));
    }

    #[test]
    fn toml_type_change_table_vs_scalar_conflicts() {
        let m = merge3(
            MergeStrategy::Toml,
            "a = 0\n[t]\nx = 1\n",
            "a = 0\n[t]\nx = 2\n",
            "a = 0\nt = 5\n",
        );
        assert_eq!(m.conflicts, ["t"]);
        assert!(!find_markers(&m.text).is_empty());
    }

    #[test]
    fn toml_invalid_falls_back_to_text() {
        let b = "a = 1\nb = 1\n";
        let o = "a = 1\nb = = 1\n";
        let t = "a = 2\nb = 1\n";
        let m = merge3(MergeStrategy::Toml, b, o, t);
        assert_eq!(m.text, merge_text_ref(b, o, t));
    }

    fn one_whole_block(m: &Merged) {
        assert_eq!(find_markers(&m.text), [1]);
        assert!(m.text.starts_with("<<<<<<< ours\n"));
        assert!(m.text.contains("\n||||||| base\n"));
        assert!(m.text.trim_end().ends_with(">>>>>>> template"));
        assert_eq!(m.text.matches("<<<<<<< ours").count(), 1);
    }

    #[test]
    fn yaml_alias_conflict_wraps_whole_file() {
        let b = "d: &d 1\nx: 0\ny: 0\nz: 0\nk: *d\n";
        let o = "d: &d 1\nx: 0\ny: 0\nz: 0\nk: 2\n";
        let t = "d: &d 3\nx: 0\ny: 0\nz: 0\nk: *d\n";
        let m = merge3(MergeStrategy::Yaml, b, o, t);
        assert_eq!(m.conflicts, ["k"]);
        one_whole_block(&m);
    }

    #[test]
    fn json_duplicate_key_conflict_wraps_whole_file() {
        let b = "{\n  \"a\": 1,\n  \"p\": 0,\n  \"q\": 0,\n  \"r\": 0,\n  \"s\": 0\n}\n";
        let o =
            "{\n  \"a\": 1,\n  \"p\": 0,\n  \"q\": 0,\n  \"r\": 0,\n  \"s\": 0,\n  \"a\": 2\n}\n";
        let t = "{\n  \"a\": 3,\n  \"p\": 0,\n  \"q\": 0,\n  \"r\": 0,\n  \"s\": 0\n}\n";
        let m = merge3(MergeStrategy::Json, b, o, t);
        assert_eq!(m.conflicts, ["a"]);
        one_whole_block(&m);
    }

    #[test]
    fn toml_new_table_is_appended_at_the_end() {
        let b = "[b]\nx = 1\n";
        let o = "[b]\nx = 1\nmine = 2\n\n[c]\ny = 1\n";
        let t = "[new]\nz = 1\n\n[new.sub]\nw = 2\n\n[b]\nx = 1\n";
        let m = merge3(MergeStrategy::Toml, b, o, t);
        assert!(m.conflicts.is_empty());
        assert_eq!(
            m.text,
            "[b]\nx = 1\nmine = 2\n\n[c]\ny = 1\n\n[new]\nz = 1\n\n[new.sub]\nw = 2\n"
        );
    }

    #[test]
    fn crlf_survives_clean_structured_merges() {
        let cr = |s: &str| s.replace('\n', "\r\n");
        let j = merge3(
            MergeStrategy::Json,
            &cr("{\n  \"a\": 1\n}\n"),
            &cr("{\n  \"a\": 1,\n  \"u\": 1\n}\n"),
            &cr("{\n  \"a\": 2\n}\n"),
        );
        assert_eq!(j.text, cr("{\n  \"a\": 2,\n  \"u\": 1\n}\n"));
        let y = merge3(
            MergeStrategy::Yaml,
            &cr("a: 1\nb: 1\n"),
            &cr("a: 1\nb: 5\n"),
            &cr("a: 2\nb: 1\n"),
        );
        assert_eq!(y.text, cr("a: 2\nb: 5\n"));
        let t = merge3(
            MergeStrategy::Toml,
            &cr("a = 1\nb = 1\n"),
            &cr("a = 1\nb = 5\n"),
            &cr("a = 2\nb = 1\n"),
        );
        assert_eq!(t.text, cr("a = 2\nb = 5\n"));
    }

    #[test]
    fn json_big_ints_and_trailing_zeros_survive() {
        let b = "{\n  \"big\": 12345678901234567890123,\n  \"e\": 1.50,\n  \"n\": 1\n}\n";
        let o = "{\n  \"big\": 12345678901234567890123,\n  \"e\": 1.50,\n  \"n\": 2\n}\n";
        let t =
            "{\n  \"big\": 12345678901234567890123,\n  \"e\": 1.50,\n  \"n\": 1,\n  \"m\": 1\n}\n";
        let m = merge3(MergeStrategy::Json, b, o, t);
        assert_eq!(
            m.text,
            "{\n  \"big\": 12345678901234567890123,\n  \"e\": 1.50,\n  \"n\": 2,\n  \"m\": 1\n}\n"
        );
    }
}
