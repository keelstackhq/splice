//! `splice explain` against the fixture template.

mod common;

use common::{splice_cmd, template_repo};
use tempfile::TempDir;

/// A repo made by `splice new` from the v1 fixture; the template dir is returned to keep it alive.
fn new_repo(extra: &[&str]) -> (TempDir, TempDir) {
    let tpl = template_repo(&["v1"]);
    let repo = tempfile::tempdir().unwrap();
    splice_cmd()
        .args([
            "new",
            tpl.path().to_str().unwrap(),
            repo.path().to_str().unwrap(),
        ])
        .args(["--data", "project=demo", "--yes", "--json"])
        .args(extra)
        .assert()
        .success();
    (tpl, repo)
}

fn json_of(cmd: &mut assert_cmd::Command) -> serde_json::Value {
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).expect("stdout is exactly one JSON object")
}

fn explain_json(repo: &TempDir, path: &str) -> serde_json::Value {
    let mut cmd = splice_cmd();
    cmd.current_dir(repo.path())
        .args(["explain", path, "--json"]);
    json_of(&mut cmd)
}

/// Human output of a successful `explain`.
fn stdout_of(repo: &TempDir, path: &str) -> String {
    let out = splice_cmd()
        .current_dir(repo.path())
        .args(["explain", path])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap()
}

#[test]
fn explains_classes() {
    let (_tpl, repo) = new_repo(&["--piece", "app:web", "--data", "project=demo"]);
    let out = stdout_of(&repo, "apps/web/package.json");
    for want in ["shared", "app/web", "merge=json"] {
        assert!(out.contains(want), "missing `{want}` in {out:?}");
    }
    let v = explain_json(&repo, "apps/web/src/main.ts");
    assert_eq!(
        v["changes"][0],
        serde_json::json!({"path":"apps/web/src/main.ts","action":"skip","class":"seed"})
    );
    let msgs = v["messages"].as_array().unwrap();
    assert!(msgs.iter().any(|m| m == "glob=src/**"), "{msgs:?}");
    assert!(msgs.iter().any(|m| m == "piece=app/web"), "{msgs:?}");
}

#[test]
fn normalises_dot_slash_and_backslashes() {
    let (_tpl, repo) = new_repo(&["--piece", "app:web"]);
    for p in ["./apps/web/tsconfig.json", "apps\\web\\tsconfig.json"] {
        let v = explain_json(&repo, p);
        assert_eq!(v["changes"][0]["path"], "apps/web/tsconfig.json", "{p}");
        assert_eq!(v["changes"][0]["class"], "owned");
    }
}

#[test]
fn unmanaged_path() {
    let (_tpl, repo) = new_repo(&[]);
    std::fs::write(repo.path().join("notes.md"), "mine\n").unwrap();
    let v = explain_json(&repo, "notes.md");
    assert_eq!(v["ok"], true);
    assert_eq!(v["changes"], serde_json::json!([]));
    assert_eq!(v["messages"], serde_json::json!(["unmanaged"]));
    assert!(stdout_of(&repo, "notes.md").contains("unmanaged"));
}

#[test]
fn path_outside_the_repo_is_a_usage_error() {
    let (_tpl, repo) = new_repo(&[]);
    for p in ["../x", "/etc/passwd"] {
        splice_cmd()
            .current_dir(repo.path())
            .args(["explain", p])
            .assert()
            .code(2);
    }
}

#[test]
fn works_offline_after_new() {
    let (tpl, repo) = new_repo(&["--piece", "app:web"]);
    drop(tpl); // removes the template repository
    let v = explain_json(&repo, "apps/web/tsconfig.json");
    assert_eq!(v["changes"][0]["class"], "owned");
}

#[test]
fn human_mode_prints_each_line_once() {
    let (_tpl, repo) = new_repo(&["--piece", "app:web"]);
    std::fs::write(repo.path().join("notes.md"), "mine\n").unwrap();
    for p in ["apps/web/package.json", "notes.md"] {
        let out = splice_cmd()
            .current_dir(repo.path())
            .args(["explain", p])
            .assert()
            .success();
        let stderr = String::from_utf8_lossy(&out.get_output().stderr).into_owned();
        assert_eq!(stderr, "", "{p}: the stdout line is not repeated on stderr");
    }
}
