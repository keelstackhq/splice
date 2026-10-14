mod common;
use common::{git_commit_all, splice_cmd, template_repo, tree_snapshot};
use std::path::Path;
use tempfile::TempDir;

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn new_at(tpl: &TempDir, at: &str) -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    splice_cmd()
        .args(["new", s(tpl.path()), s(repo.path()), "--ref", at])
        .args(["--piece", "app:web", "--data", "project=demo", "--yes"])
        .assert()
        .success();
    git_commit_all(repo.path(), "new");
    repo
}

fn read(repo: &TempDir, rel: &str) -> String {
    std::fs::read_to_string(repo.path().join(rel)).unwrap()
}

fn write(repo: &TempDir, rel: &str, text: &str) {
    std::fs::write(repo.path().join(rel), text).unwrap();
}

/// Run `check` and return (exit code, parsed `--json` stdout).
fn check(repo: &TempDir, extra: &[&str]) -> (i32, serde_json::Value) {
    let out = splice_cmd()
        .current_dir(repo.path())
        .args(["check", "--json"])
        .args(extra)
        .output()
        .unwrap();
    let v = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)));
    (out.status.code().unwrap(), v)
}

/// A repo updated v1 -> v2 with a conflict in ci.yml.
fn conflicted(tpl: &TempDir) -> TempDir {
    let repo = new_at(tpl, "v1.0.0");
    let t = read(&repo, ".github/workflows/ci.yml");
    write(
        &repo,
        ".github/workflows/ci.yml",
        &t.replace("ubuntu-latest", "ubuntu-22.04"),
    );
    git_commit_all(repo.path(), "user edit");
    splice_cmd()
        .current_dir(repo.path())
        .args(["update", "--yes"])
        .assert()
        .code(1);
    repo
}

#[test]
fn clean_repo_passes() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "v2.0.0");
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["changes"], serde_json::json!([]));
    assert_eq!(v["ok"], true);
    assert_eq!(v["command"], "check");
}

#[test]
fn owned_drift_fails() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "v2.0.0");
    write(&repo, ".editorconfig", "# mine\n");
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 1);
    assert_eq!(v["ok"], false);
    assert_eq!(
        v["changes"],
        serde_json::json!([{"path":".editorconfig","action":"update","class":"owned"}])
    );
}

#[test]
fn deleted_owned_file_is_drift() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "v2.0.0");
    std::fs::remove_file(repo.path().join(".editorconfig")).unwrap();
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 1);
    assert_eq!(
        v["changes"],
        serde_json::json!([{"path":".editorconfig","action":"update","class":"owned"}])
    );
}

#[test]
fn deleted_shared_file_is_not_drift() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "v2.0.0");
    std::fs::remove_file(repo.path().join("notes.txt")).unwrap();
    assert_eq!(check(&repo, &["--offline"]).0, 0);
}

#[test]
fn markers_fail_and_stray_separator_does_not() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = conflicted(&tpl);
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 1, "{v}");
    assert_eq!(
        v["changes"],
        serde_json::json!([{"path":".github/workflows/ci.yml","action":"conflict","class":"shared"}])
    );

    // resolve, then a lone `=======` in a seed (not in the lock) and in a shared file
    write(
        &repo,
        ".github/workflows/ci.yml",
        "name: ci\non: push\njobs:\n  test:\n    runs-on: ubuntu-22.04\n",
    );
    for f in ["README.md", "notes.txt"] {
        let t = read(&repo, f);
        write(&repo, f, &format!("{t}\n=======\n"));
    }
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["changes"], serde_json::json!([]));
}

#[test]
fn marker_block_in_a_seed_is_ignored() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "v2.0.0");
    write(
        &repo,
        "README.md",
        "<<<<<<< ours\na\n=======\nb\n>>>>>>> template\n",
    );
    assert_eq!(check(&repo, &["--offline"]).0, 0);
}

#[test]
fn update_available() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "v1.0.0");
    let (code, v) = check(&repo, &[]);
    assert_eq!(code, 1, "{v}");
    assert_eq!(v["ok"], false);
    assert_eq!(v["changes"], serde_json::json!([]));
    let msgs = v["messages"].to_string();
    assert!(
        msgs.contains("update available: 1.0.0 \u{2192} 2.0.0"),
        "{msgs}"
    );

    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 0, "{v}");
}

#[test]
fn up_to_date_online_passes() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "v2.0.0");
    assert_eq!(check(&repo, &[]).0, 0);
}

#[test]
fn resolve_conflict_then_check_passes() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = conflicted(&tpl);
    assert_eq!(check(&repo, &["--offline"]).0, 1);
    write(
        &repo,
        ".github/workflows/ci.yml",
        "name: ci\non: push\njobs:\n  test:\n    runs-on: ubuntu-22.04\n",
    );
    git_commit_all(repo.path(), "resolved");
    splice_cmd()
        .current_dir(repo.path())
        .args(["check", "--offline"])
        .assert()
        .code(0);
}

#[test]
fn fetch_failure_is_message_not_error() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "v2.0.0");
    drop(tpl); // the template repo is deleted
    let (code, v) = check(&repo, &[]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["ok"], true);
    assert!(
        !v["messages"].as_array().unwrap().is_empty(),
        "a failed fetch is reported as a message: {v}"
    );
}

#[test]
fn offline_needs_no_template() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "v2.0.0");
    drop(tpl);
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["messages"], serde_json::json!([]));
}

#[test]
fn fetch_failure_does_not_hide_drift() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "v2.0.0");
    drop(tpl);
    write(&repo, ".editorconfig", "# mine\n");
    assert_eq!(check(&repo, &[]).0, 1);
}

#[test]
fn check_never_writes() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = conflicted(&tpl);
    write(&repo, ".editorconfig", "# mine\n");
    let tree = tree_snapshot(repo.path());
    let lock = std::fs::read(repo.path().join(".splice/lock.toml")).unwrap();
    for extra in [&["--offline"][..], &[]] {
        check(&repo, extra);
        assert_eq!(tree_snapshot(repo.path()), tree);
        assert_eq!(
            std::fs::read(repo.path().join(".splice/lock.toml")).unwrap(),
            lock
        );
    }
}

#[test]
fn no_lock_is_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    splice_cmd()
        .args(["check", "--offline", "-C", s(dir.path())])
        .assert()
        .code(2);
}

#[test]
fn half_resolved_file_fails() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "v2.0.0");
    let t = read(&repo, "notes.txt");
    write(&repo, "notes.txt", &format!("{t}>>>>>>> template\n"));
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 1, "{v}");
    assert_eq!(
        v["changes"],
        serde_json::json!([{"path":"notes.txt","action":"conflict","class":"shared"}])
    );
}
