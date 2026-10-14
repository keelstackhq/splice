mod common;
use common::{git_commit_all, splice_cmd, tag, template_repo, tree_snapshot};
use std::path::Path;
use tempfile::TempDir;

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// A committed repo created by `splice new` at `v1.0.0` with the `app:web` piece.
fn new_repo(tpl: &TempDir) -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    splice_cmd()
        .args(["new", s(tpl.path()), s(repo.path()), "--ref", "v1.0.0"])
        .args(["--piece", "app:web", "--data", "project=demo", "--yes"])
        .assert()
        .success();
    git_commit_all(repo.path(), "new");
    repo
}

fn porcelain(repo: &TempDir) -> String {
    let o = std::process::Command::new("git")
        .current_dir(repo.path())
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    String::from_utf8(o.stdout).unwrap()
}

fn json(out: Vec<u8>) -> serde_json::Value {
    serde_json::from_slice(&out).unwrap()
}

/// The user edit that makes the v1 -> v2 update conflict in `ci.yml`.
fn edit_ci(repo: &TempDir) {
    let p = repo.path().join(".github/workflows/ci.yml");
    let t = std::fs::read_to_string(&p).unwrap();
    std::fs::write(&p, t.replace("ubuntu-latest", "ubuntu-22.04")).unwrap();
    git_commit_all(repo.path(), "edit ci");
}

#[test]
fn diff_writes_nothing() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl);
    edit_ci(&repo);
    let before = tree_snapshot(repo.path());
    let out = splice_cmd()
        .args(["diff", "-C", s(repo.path()), "--yes"])
        .assert()
        .success() // exit 0 although `update` would conflict
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("--- a/.editorconfig"), "{text}");
    assert!(text.contains("+++ b/.editorconfig"), "{text}");
    assert!(text.contains("+indent_size = 4"), "{text}");
    assert!(text.contains("<<<<<<< ours"), "conflict preview: {text}");
    // a removal is one line, a creation diffs against /dev/null
    assert!(text.contains("delete   tool.config.mts"), "{text}");
    assert!(
        text.contains("--- /dev/null\n+++ b/tool.config.ts"),
        "{text}"
    );
    assert_eq!(porcelain(&repo), "");
    assert_eq!(tree_snapshot(repo.path()), before);
}

#[test]
fn diff_json_changes_match_update() {
    let tpl = template_repo(&["v1", "v2"]);
    let a = new_repo(&tpl);
    let b = new_repo(&tpl);
    edit_ci(&a);
    edit_ci(&b);
    let d = json(
        splice_cmd()
            .args(["diff", "-C", s(a.path()), "--yes", "--json"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    let u = json(
        splice_cmd()
            .args(["update", "-C", s(b.path()), "--yes", "--json"])
            .assert()
            .code(1)
            .get_output()
            .stdout
            .clone(),
    );
    assert_eq!(d["command"], "diff");
    assert_eq!(d["ok"], true);
    assert_eq!(d["changes"], u["changes"]);
    assert!(
        !d.to_string().contains("indent_size = 4"),
        "no patch text: {d}"
    );
    assert_eq!(porcelain(&a), "");
}

#[test]
fn diff_reports_owned_drift_without_failing() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl);
    std::fs::write(repo.path().join("apps/web/tsconfig.json"), "{\"mine\":1}\n").unwrap();
    git_commit_all(repo.path(), "edit");
    let out = splice_cmd()
        .args(["diff", "-C", s(repo.path()), "--yes", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json(out);
    assert!(
        v["messages"].to_string().contains("apps/web/tsconfig.json"),
        "{v}"
    );
    assert!(
        v["messages"].to_string().contains("update would refuse"),
        "{v}"
    );
    // update would refuse with no changes; diff still lists the planned ones
    assert!(!v["changes"].as_array().unwrap().is_empty(), "{v}");
    assert_eq!(porcelain(&repo), "");
}

#[test]
fn diff_up_to_date_prints_nothing_and_works_on_a_dirty_tree() {
    let tpl = template_repo(&["v1"]);
    let repo = new_repo(&tpl);
    std::fs::write(repo.path().join("scratch"), "x").unwrap();
    let out = splice_cmd()
        .args(["diff", "-C", s(repo.path()), "--yes"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(String::from_utf8(out).unwrap(), "0 changes\n");
}

#[test]
fn diff_never_prompts_a_new_var_without_default_is_usage() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl);
    let p = tpl.path().join("splice.toml");
    let t = std::fs::read_to_string(&p).unwrap();
    std::fs::write(
        &p,
        t.replace("[vars]\n", "[vars]\nextra = { prompt = \"extra thing\" }\n"),
    )
    .unwrap();
    git_commit_all(tpl.path(), "extra var");
    tag(&tpl, "v2.1.0");
    let out = splice_cmd()
        .args(["diff", "-C", s(repo.path()), "--yes", "--json"])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let v = json(out);
    assert!(v["messages"].to_string().contains("extra"), "{v}");
    assert_eq!(porcelain(&repo), "");
}

#[test]
fn diff_uses_the_default_of_a_new_var() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl);
    let p = tpl.path().join("splice.toml");
    let t = std::fs::read_to_string(&p).unwrap();
    std::fs::write(
        &p,
        t.replace(
            "[vars]\n",
            "[vars]\nextra = { prompt = \"extra thing\", default = \"d\" }\n",
        ),
    )
    .unwrap();
    git_commit_all(tpl.path(), "extra var");
    tag(&tpl, "v2.1.0");
    splice_cmd()
        .args(["diff", "-C", s(repo.path()), "--yes"])
        .assert()
        .success();
}
