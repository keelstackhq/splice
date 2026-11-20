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

fn tagged(dir: &std::path::Path) {
    common::git(
        dir,
        "2026-01-01T00:00:00+00:00",
        &["init", "-q", "-b", "main"],
    );
    common::commit_tag(dir, "v1", "v1.0.0");
}

#[test]
fn min_splice_refused() {
    let tpl = template_repo(&["v1"]);
    let path = tpl.path().join("splice.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        text.replace("min_splice = \"0.1.0\"", "min_splice = \"99.0.0\""),
    )
    .unwrap();
    common::commit_tag(tpl.path(), "v3", "v3.0.0");
    splice_cmd().args(["pieces", s(&tpl)]).assert().code(3);
}

#[test]
fn default_name_matches_new_yes() {
    let tpl = common::inline_template(&[
        ("base", "id = \"base\"\ndescription = \"b\"\n", &[]),
        (
            "plain",
            "id = \"plain\"\ndescription = \"p\"\nmultiple = true\n",
            &[],
        ),
        (
            "dflt",
            "id = \"dflt\"\ndescription = \"d\"\nmultiple = true\n[vars]\nname = { prompt = \"n\", default = \"web\" }\n",
            &[],
        ),
        (
            "nodflt",
            "id = \"nodflt\"\ndescription = \"n\"\nmultiple = true\n[vars]\nname = { prompt = \"n\" }\n",
            &[],
        ),
        ("single", "id = \"single\"\ndescription = \"s\"\n", &[]),
    ]);
    tagged(tpl.path());
    let out = splice_cmd()
        .args(["pieces", s(&tpl), "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let names: Vec<(String, serde_json::Value)> = v["pieces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["id"].as_str().unwrap().to_string(),
                p["default_name"].clone(),
            )
        })
        .collect();
    assert_eq!(
        names,
        [
            ("base".to_string(), serde_json::Value::Null),
            ("dflt".into(), "web".into()),
            ("nodflt".into(), serde_json::Value::Null),
            ("plain".into(), "plain".into()),
            ("single".into(), serde_json::Value::Null),
        ]
    );
    // null for `nodflt` is real: `new --yes` cannot name it.
    let dest = tempfile::tempdir().unwrap();
    splice_cmd()
        .args(["new", s(&tpl), s(&dest), "--yes", "--piece", "nodflt"])
        .assert()
        .code(2);
}
