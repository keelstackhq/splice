//! `splice pieces` end to end against the fixture template.

mod common;

use common::{splice_cmd, template_repo};

fn s(d: &tempfile::TempDir) -> &str {
    d.path().to_str().unwrap()
}

#[test]
fn json_lists_pieces_and_template_vars() {
    let tpl = template_repo(&["v1"]);
    let out = splice_cmd()
        .args(["pieces", s(&tpl), "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).expect("exactly one JSON object");
    assert_eq!(v["ok"], true);
    assert_eq!(v["command"], "pieces");
    assert_eq!(v["changes"], serde_json::json!([]));
    assert_eq!(v["template"]["name"], "fixture");
    assert_eq!(v["template"]["version"], "1.0.0");
    assert_eq!(
        v["template"]["vars"][1],
        serde_json::json!({"name": "scope", "prompt": "npm scope without @", "default": "acme"})
    );
    assert_eq!(v["template"]["vars"][0]["default"], serde_json::Value::Null);
    let ids: Vec<&str> = v["pieces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["app", "base", "lib"]);
    assert_eq!(
        v["pieces"][0],
        serde_json::json!({
            "id": "app",
            "description": "App piece",
            "base": false,
            "multiple": true,
            "default_name": "web",
            "requires": ["base"],
            "vars": [{"name": "name", "prompt": "app name", "default": "web"}],
        })
    );
    assert_eq!(v["pieces"][1]["base"], true);
    assert_eq!(v["pieces"][1]["default_name"], serde_json::Value::Null);
    assert_eq!(v["pieces"][2]["requires"], serde_json::json!(["app"]));
    insta::assert_json_snapshot!(v);
}

#[test]
fn human_output_is_one_block_per_piece() {
    let tpl = template_repo(&["v1"]);
    let out = splice_cmd()
        .args(["pieces", s(&tpl)])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    insta::assert_snapshot!(String::from_utf8(out).unwrap());
}

#[test]
fn bad_source_is_usage() {
    splice_cmd().args(["pieces", "gh:nope"]).assert().code(2);
}

#[test]
fn missing_template_is_env_error() {
    let dir = tempfile::tempdir().unwrap();
    splice_cmd()
        .args(["pieces", s(&dir), "--json"])
        .assert()
        .code(3);
}

#[test]
fn works_without_a_repo_and_needs_no_clean_tree() {
    let tpl = template_repo(&["v1"]);
    let cwd = tempfile::tempdir().unwrap();
    splice_cmd()
        .current_dir(cwd.path())
        .args(["pieces", s(&tpl)])
        .assert()
        .success();
}
