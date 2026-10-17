//! `splice diff` end to end (spec §8.8): the update plan, printed, with nothing written.

mod common;

use common::{commit_all, splice_cmd, template_repo};
use std::process::Command as Proc;
use tempfile::TempDir;

/// A repo at v1 of `tpl`, committed, with a shared edit that conflicts with v2 (`ci.yml`) and a
/// shared edit that merges cleanly (`notes.txt`), committed too.
fn edited_repo(tpl: &TempDir) -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    splice_cmd()
        .args([
            "new",
            tpl.path().to_str().unwrap(),
            repo.path().to_str().unwrap(),
        ])
        .args([
            "--ref",
            "v1.0.0",
            "--data",
            "project=demo",
            "--yes",
            "--json",
        ])
        .assert()
        .success();
    let ci = repo.path().join(".github/workflows/ci.yml");
    let text = std::fs::read_to_string(&ci).unwrap();
    std::fs::write(&ci, text.replace("ubuntu-latest", "macos-latest")).unwrap();
    let notes = repo.path().join("notes.txt");
    let mut text = std::fs::read_to_string(&notes).unwrap();
    text.push_str("mine\n");
    std::fs::write(&notes, text).unwrap();
    commit_all(repo.path());
    repo
}

fn porcelain(repo: &TempDir) -> String {
    let out = Proc::new("git")
        .current_dir(repo.path())
        .args(["status", "--porcelain"])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap()
}

fn json(out: &[u8]) -> serde_json::Value {
    serde_json::from_slice(out).expect("stdout is exactly one JSON object")
}

#[test]
fn diff_writes_nothing() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = edited_repo(&tpl);
    // Owned drift too: `update` would refuse; `diff` still reports.
    std::fs::write(repo.path().join("tool.config.mts"), "mine\n").unwrap();
    commit_all(repo.path());
    let out = splice_cmd()
        .current_dir(repo.path())
        .arg("diff")
        .assert()
        .code(0)
        .get_output()
        .clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stdout.contains("--- a/.editorconfig"), "{stdout}");
    assert!(stdout.contains("+++ b/.editorconfig"), "{stdout}");
    assert!(stdout.contains("+indent_size = 4"), "{stdout}");
    // The conflict shows its markers; removals get one line each.
    assert!(stdout.contains("+<<<<<<< ours"), "{stdout}");
    assert!(stdout.contains("delete old.txt"), "{stdout}");
    assert!(
        stderr.contains("tool.config.mts") && stderr.contains("edited locally"),
        "{stderr}"
    );
    assert_eq!(porcelain(&repo), "");
}

#[test]
fn diff_json_changes_match_update() {
    let tpl = template_repo(&["v1", "v2"]);
    let (a, b) = (edited_repo(&tpl), edited_repo(&tpl));
    let diff = splice_cmd()
        .current_dir(a.path())
        .args(["diff", "--json"])
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let diff = json(&diff);
    assert_eq!(diff["command"], "diff");
    assert_eq!(diff["ok"], true);
    assert_eq!(porcelain(&a), "");
    let update = splice_cmd()
        .current_dir(b.path())
        .args(["update", "--yes", "--json"])
        .assert()
        .code(1) // the ci.yml conflict
        .get_output()
        .stdout
        .clone();
    let update = json(&update);
    assert!(!diff["changes"].as_array().unwrap().is_empty());
    assert_eq!(diff["changes"], update["changes"]);
}

#[test]
fn diff_up_to_date_and_downgrade() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = edited_repo(&tpl);
    let out = splice_cmd()
        .current_dir(repo.path())
        .args(["diff", "--to", "1.0.0", "--json"])
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    assert_eq!(json(&out)["changes"], serde_json::json!([]));
    let tpl2 = template_repo(&["v1", "v2"]);
    let repo2 = tempfile::tempdir().unwrap();
    splice_cmd()
        .args([
            "new",
            tpl2.path().to_str().unwrap(),
            repo2.path().to_str().unwrap(),
        ])
        .args(["--data", "project=demo", "--yes", "--json"])
        .assert()
        .success();
    splice_cmd()
        .current_dir(repo2.path())
        .args(["diff", "--to", "1.0.0", "--json"])
        .assert()
        .code(2);
}
