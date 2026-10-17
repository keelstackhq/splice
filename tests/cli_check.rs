//! `splice check` against the fixture template (spec §8.7).

mod common;

use common::{commit_all, splice_cmd, template_repo};
use tempfile::TempDir;

/// `splice new` of the app piece from `tpl` (at `--ref` when given) into a committed repo.
fn new_repo(tpl: &TempDir, git_ref: Option<&str>) -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    let mut cmd = splice_cmd();
    cmd.args([
        "new",
        tpl.path().to_str().unwrap(),
        repo.path().to_str().unwrap(),
    ])
    .args(["--piece", "app:web", "--data", "project=demo", "--yes"]);
    if let Some(r) = git_ref {
        cmd.args(["--ref", r]);
    }
    cmd.assert().success();
    commit_all(repo.path());
    repo
}

fn check(repo: &TempDir, args: &[&str]) -> (i32, serde_json::Value) {
    let out = splice_cmd()
        .current_dir(repo.path())
        .arg("check")
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    let v = serde_json::from_slice(&out.stdout).expect("stdout is exactly one JSON object");
    (out.status.code().unwrap(), v)
}

fn messages(v: &serde_json::Value) -> String {
    v["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn clean_repo_passes() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, None);
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 0);
    assert_eq!(v["ok"], true);
    assert_eq!(v["command"], "check");
    assert_eq!(v["changes"], serde_json::json!([]));
    // Online too: the lock is already at the latest stable tag.
    assert_eq!(check(&repo, &[]).0, 0);
}

#[test]
fn owned_drift_fails() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, None);
    std::fs::write(repo.path().join(".editorconfig"), "root = false\n").unwrap();
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 1);
    assert_eq!(v["ok"], false);
    assert_eq!(
        v["changes"],
        serde_json::json!([{"path":".editorconfig","action":"update","class":"owned"}])
    );
    // A deleted owned file is drift too.
    std::fs::remove_file(repo.path().join(".editorconfig")).unwrap();
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 1);
    assert_eq!(v["changes"][0]["path"], ".editorconfig");
}

#[test]
fn markers_fail_and_stray_separator_does_not() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, Some("v1.0.0"));
    let ci = repo.path().join(".github/workflows/ci.yml");
    let text = std::fs::read_to_string(&ci).unwrap();
    std::fs::write(&ci, text.replace("ubuntu-latest", "macos-latest")).unwrap();
    commit_all(repo.path());
    splice_cmd()
        .current_dir(repo.path())
        .args(["update", "--yes", "--json"])
        .assert()
        .code(1);
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 1);
    assert_eq!(
        v["changes"],
        serde_json::json!([{"path":".github/workflows/ci.yml","action":"conflict","class":"shared"}])
    );

    // Resolved; a lone `=======` in a seed (not in the lock) and in a shared file is plain text.
    std::fs::write(&ci, "name: ci\non: [push]\n").unwrap();
    for rel in ["README.md", "notes.txt"] {
        let p = repo.path().join(rel);
        let mut t = std::fs::read_to_string(&p).unwrap();
        t.push_str("=======\n");
        std::fs::write(p, t).unwrap();
    }
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!((code, v["changes"].clone()), (0, serde_json::json!([])));

    // CRLF line endings: markers are still found.
    std::fs::write(
        &ci,
        "name: ci\r\n<<<<<<< ours\r\na\r\n||||||| base\r\nb\r\n=======\r\nc\r\n>>>>>>> template\r\n",
    )
    .unwrap();
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 1);
    assert_eq!(
        v["changes"],
        serde_json::json!([{"path":".github/workflows/ci.yml","action":"conflict","class":"shared"}])
    );
    assert!(messages(&v).contains("line 2"), "{v}");
}

#[test]
fn update_available() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, Some("v1.0.0"));
    let (code, v) = check(&repo, &[]);
    assert_eq!(code, 1);
    assert_eq!(v["ok"], false);
    assert!(
        messages(&v).contains("update available: 1.0.0 → 2.0.0"),
        "{v}"
    );
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 0);
    assert!(!messages(&v).contains("update available"));
}

#[test]
fn fetch_failure_is_message_not_error() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, Some("v1.0.0"));
    let path = tpl.path().to_path_buf();
    drop(tpl); // removes the template directory
    assert!(!path.exists());
    let (code, v) = check(&repo, &[]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["ok"], true);
    assert!(messages(&v).contains("could not look for updates"), "{v}");
}

#[test]
fn check_writes_nothing() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, Some("v1.0.0"));
    let lock = std::fs::read(repo.path().join(".splice/lock.toml")).unwrap();
    check(&repo, &[]);
    assert_eq!(
        std::fs::read(repo.path().join(".splice/lock.toml")).unwrap(),
        lock
    );
    let st = std::process::Command::new("git")
        .current_dir(repo.path())
        .args(["status", "--porcelain"])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(st.stdout.is_empty());
}

#[test]
fn no_lock_is_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    splice_cmd()
        .current_dir(dir.path())
        .args(["check", "--json"])
        .assert()
        .code(2);
}

#[test]
fn unsafe_lock_path_is_refused_not_read() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, None);
    let lock = repo.path().join(".splice/lock.toml");
    let text = std::fs::read_to_string(&lock).unwrap();
    let text = text.replacen(
        "[files]\n",
        "[files]\n\"../outside.txt\" = { class = \"owned\", sha256 = \"00\" }\n",
        1,
    );
    assert!(text.contains("../outside.txt"));
    std::fs::write(&lock, text).unwrap();
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 3, "{v}");
    assert!(messages(&v).contains("../outside.txt"), "{v}");
}

#[test]
fn markers_report_the_lock_class() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, None);
    let p = repo.path().join(".editorconfig");
    let mut t = std::fs::read_to_string(&p).unwrap();
    t.push_str("<<<<<<< ours\na\n||||||| base\nb\n=======\nc\n>>>>>>> template\n");
    std::fs::write(&p, t).unwrap();
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 1);
    assert_eq!(
        v["changes"],
        serde_json::json!([
            {"path":".editorconfig","action":"update","class":"owned"},
            {"path":".editorconfig","action":"conflict","class":"owned"}
        ])
    );
}
