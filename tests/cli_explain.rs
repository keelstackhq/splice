mod common;
use common::{splice_cmd, template_repo};
use predicates::prelude::*;
use std::path::Path;
use tempfile::TempDir;

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn new_repo(extra: &[&str]) -> (TempDir, TempDir) {
    let tpl = template_repo(&["v1"]);
    let repo = tempfile::tempdir().unwrap();
    splice_cmd()
        .args(["new", s(tpl.path()), s(repo.path())])
        .args(extra)
        .args(["--data", "project=demo", "--yes", "--json"])
        .assert()
        .success();
    (tpl, repo)
}

fn json_of(cmd: &mut assert_cmd::Command) -> serde_json::Value {
    let out = cmd.assert().success().get_output().stdout.clone();
    serde_json::from_slice(&out).unwrap()
}

fn msgs(v: &serde_json::Value) -> Vec<String> {
    v["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn explains_classes() {
    let (_tpl, repo) = new_repo(&["--piece", "app:web"]);
    splice_cmd()
        .current_dir(repo.path())
        .args(["explain", "apps/web/package.json"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("shared")
                .and(predicate::str::contains("app/web"))
                .and(predicate::str::contains("merge=json")),
        );
    let v = json_of(splice_cmd().current_dir(repo.path()).args([
        "explain",
        "apps/web/src/main.ts",
        "--json",
    ]));
    assert_eq!(
        v["changes"][0],
        serde_json::json!({"path":"apps/web/src/main.ts","action":"skip","class":"seed"})
    );
    assert!(msgs(&v).iter().any(|m| m == "glob=src/**"), "{v}");
    assert!(msgs(&v).iter().any(|m| m == "piece=app/web"), "{v}");
    assert!(msgs(&v).iter().any(|m| m.starts_with("merge=")), "{v}");
}

#[test]
fn default_glob_is_named() {
    let (_tpl, repo) = new_repo(&[]);
    let v = json_of(
        splice_cmd()
            .current_dir(repo.path())
            .args(["explain", "apps.txt", "--json"]),
    );
    assert_eq!(v["changes"][0]["class"], "owned");
    assert!(msgs(&v).iter().any(|m| m == "glob=default"), "{v}");
    assert!(msgs(&v).iter().any(|m| m == "piece=base/base"), "{v}");
}

#[test]
fn unmanaged_path() {
    let (_tpl, repo) = new_repo(&[]);
    std::fs::write(repo.path().join("notes.md"), "x\n").unwrap();
    for p in ["notes.md", "../x", "/etc/passwd", "a/../package.json"] {
        let v = json_of(
            splice_cmd()
                .current_dir(repo.path())
                .args(["explain", p, "--json"]),
        );
        assert_eq!(v["ok"], true);
        assert_eq!(v["changes"].as_array().unwrap().len(), 0, "{p}");
        assert!(msgs(&v).iter().any(|m| m == "unmanaged"), "{p}: {v}");
    }
}

#[test]
fn normalises_dot_slash_and_backslash() {
    let (_tpl, repo) = new_repo(&["--piece", "app:web"]);
    for p in ["./apps/web/package.json", "apps\\web\\package.json"] {
        let v = json_of(
            splice_cmd()
                .current_dir(repo.path())
                .args(["explain", p, "--json"]),
        );
        assert_eq!(v["changes"][0]["path"], "apps/web/package.json", "{p}");
    }
}

#[test]
fn works_offline_after_new() {
    let (tpl, repo) = new_repo(&[]);
    drop(tpl); // deletes the template repo
    let v =
        json_of(
            splice_cmd()
                .current_dir(repo.path())
                .args(["explain", "package.json", "--json"]),
        );
    assert_eq!(v["changes"][0]["class"], "shared");
}

#[test]
fn without_lock_is_usage() {
    let d = tempfile::tempdir().unwrap();
    splice_cmd()
        .current_dir(d.path())
        .args(["explain", "x"])
        .assert()
        .code(2);
}

#[test]
fn lock_warnings_are_messages() {
    let (_tpl, repo) = new_repo(&[]);
    let p = repo.path().join(".splice/lock.toml");
    let text = std::fs::read_to_string(&p).unwrap();
    std::fs::write(&p, format!("future_key = 1\n{text}")).unwrap();
    let v =
        json_of(
            splice_cmd()
                .current_dir(repo.path())
                .args(["explain", "package.json", "--json"]),
        );
    assert!(msgs(&v).iter().any(|m| m.contains("future_key")), "{v}");
}
