//! `splice update` end to end against the fixture template (spec §8.3).

mod common;

use common::{amend_tip, git, splice_cmd, template_repo, template_repo_tagged};
use splice::lock::Lock;
use std::path::Path;
use std::process::Command as Proc;
use tempfile::TempDir;

const DATE: &str = "2026-03-01T00:00:00+00:00";

/// `splice new` from `tpl` into a fresh repo, then commit everything so the tree is clean.
fn new_repo(tpl: &TempDir, extra: &[&str]) -> TempDir {
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
    commit_all(repo.path());
    repo
}

fn commit_all(repo: &Path) {
    git(repo, DATE, &["add", "-A"]);
    git(repo, DATE, &["commit", "-q", "-m", "state"]);
}

fn update(repo: &TempDir, args: &[&str]) -> assert_cmd::assert::Assert {
    splice_cmd()
        .current_dir(repo.path())
        .arg("update")
        .args(args)
        .args(["--yes", "--json"])
        .assert()
}

fn json(out: &[u8]) -> serde_json::Value {
    serde_json::from_slice(out).expect("stdout is exactly one JSON object")
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

fn read(repo: &TempDir, rel: &str) -> String {
    std::fs::read_to_string(repo.path().join(rel)).unwrap()
}

fn lock_bytes(repo: &TempDir) -> Vec<u8> {
    std::fs::read(repo.path().join(".splice/lock.toml")).unwrap()
}

fn lock(repo: &TempDir) -> Lock {
    Lock::read(repo.path()).unwrap().unwrap()
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

#[test]
fn updates_v1_to_v2() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0", "--piece", "app"]);
    std::fs::remove_file(repo.path().join(".post-ran")).unwrap();
    commit_all(repo.path());

    let out = update(&repo, &[]).success().get_output().stdout.clone();
    let v = json(&out);
    assert_eq!(v["command"], "update");
    assert_eq!(v["ok"], true);
    let l = lock(&repo);
    assert_eq!(l.template.version.to_string(), "2.0.0");
    assert!(!repo.path().join("tool.config.mts").exists());
    assert!(!repo.path().join("old.txt").exists());
    assert!(repo.path().join("tool.config.ts").exists());
    assert!(read(&repo, ".editorconfig").contains("indent_size = 4"));
    assert!(read(&repo, "apps/web/tsconfig.json").contains("noUncheckedIndexedAccess"));
    // Migration step and post command (when ∋ update) ran.
    assert!(repo.path().join(".splice/migrated.txt").exists());
    assert!(repo.path().join(".post-ran").exists());
}

#[test]
fn dirty_tree_refused() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0"]);
    std::fs::write(repo.path().join("x"), "dirty").unwrap();
    let before = lock_bytes(&repo);
    let out = update(&repo, &[]).code(2).get_output().stdout.clone();
    assert!(messages(&json(&out)).contains("--allow-dirty"));
    assert_eq!(lock_bytes(&repo), before);

    update(&repo, &["--allow-dirty"]).success();
    assert_eq!(lock(&repo).template.version.to_string(), "2.0.0");
    assert_eq!(read(&repo, "x"), "dirty");
}

#[test]
fn outside_work_tree_refused() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0"]);
    std::fs::remove_dir_all(repo.path().join(".git")).unwrap();
    update(&repo, &[]).code(2);
    update(&repo, &["--allow-dirty"]).success();
}

#[test]
fn up_to_date_is_noop() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &[]);
    let before = lock_bytes(&repo);
    let out = update(&repo, &[]).success().get_output().stdout.clone();
    let v = json(&out);
    assert!(messages(&v).contains("up to date"));
    assert_eq!(v["changes"].as_array().unwrap().len(), 0);
    assert_eq!(lock_bytes(&repo), before);
    assert_eq!(porcelain(&repo), "");
}

#[test]
fn moved_tag_proceeds_with_message() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &[]);
    let old_commit = lock(&repo).template.commit;
    let notes = tpl.path().join("pieces/base/files/notes.txt.jinja");
    let text = std::fs::read_to_string(&notes).unwrap();
    std::fs::write(&notes, format!("{text}Moved.\n")).unwrap();
    amend_tip(tpl.path(), "v2.0.0");

    let out = update(&repo, &[]).success().get_output().stdout.clone();
    assert!(messages(&json(&out)).contains("moved"));
    assert_ne!(lock(&repo).template.commit, old_commit);
    assert!(read(&repo, "notes.txt").contains("Moved."));
}

#[test]
fn downgrade_refused() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &[]);
    let before = lock_bytes(&repo);
    let out = update(&repo, &["--to", "1.0.0"])
        .code(2)
        .get_output()
        .stdout
        .clone();
    assert!(messages(&json(&out)).contains("downgrade"));
    assert_eq!(lock_bytes(&repo), before);
}

#[test]
fn conflicts_written_lock_records_target() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0"]);
    let notes = read(&repo, "notes.txt");
    let mut lines: Vec<&str> = notes.lines().collect();
    lines[1] = "My own second line.";
    std::fs::write(repo.path().join("notes.txt"), lines.join("\n") + "\n").unwrap();
    commit_all(repo.path());

    let out = update(&repo, &[]).code(1).get_output().stdout.clone();
    let v = json(&out);
    assert!(messages(&v).contains("notes.txt"));
    let text = read(&repo, "notes.txt");
    assert!(text.contains("<<<<<<< ours") && text.contains(">>>>>>> template"));
    assert_eq!(lock(&repo).template.version.to_string(), "2.0.0");
}

#[test]
fn min_splice_refused_before_write() {
    let tpl = template_repo(&["v1", "v2", "v3"]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0"]);
    let out = update(&repo, &["--to", "3.0.0"])
        .code(3)
        .get_output()
        .stdout
        .clone();
    assert!(messages(&json(&out)).contains("99.0.0"));
    assert_eq!(porcelain(&repo), "");
}

#[test]
fn owned_drift_refused_then_forced() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0", "--piece", "app"]);
    std::fs::write(
        repo.path().join("apps/web/tsconfig.json"),
        "{ \"mine\": 1 }\n",
    )
    .unwrap();
    commit_all(repo.path());

    let out = update(&repo, &[]).code(1).get_output().stdout.clone();
    let v = json(&out);
    assert_eq!(v["ok"], false);
    assert_eq!(v["changes"].as_array().unwrap().len(), 0);
    assert!(messages(&v).contains("apps/web/tsconfig.json"));
    assert_eq!(porcelain(&repo), "");

    let out = update(&repo, &["--force"])
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json(&out);
    assert!(read(&repo, "apps/web/tsconfig.json").contains("noUncheckedIndexedAccess"));
    let change = v["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["path"] == "apps/web/tsconfig.json")
        .expect("tsconfig change");
    assert_eq!(change["action"], "update");
    assert_eq!(change["class"], "owned");
}

#[test]
fn new_var_takes_default_with_yes() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0"]);
    update(&repo, &[]).success();
    let l = lock(&repo);
    assert_eq!(l.vars.get("license").map(String::as_str), Some("MIT"));
    assert_eq!(l.vars.get("project").map(String::as_str), Some("demo"));
}

#[test]
fn new_var_from_data_and_unknown_data_refused() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0"]);
    update(&repo, &["--data", "nope=1"]).code(2);
    update(&repo, &["--data", "license=ISC"]).success();
    assert_eq!(
        lock(&repo).vars.get("license").map(String::as_str),
        Some("ISC")
    );
}

#[test]
fn locked_template_var_in_data_refused() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0"]);
    let before = lock_bytes(&repo);
    let out = update(&repo, &["--data", "project=other"])
        .code(2)
        .get_output()
        .stdout
        .clone();
    assert!(
        messages(&json(&out)).contains("`project` is locked at `demo`; update keeps lock values")
    );
    assert_eq!(lock_bytes(&repo), before);
    assert_eq!(porcelain(&repo), "");
}

#[test]
fn piece_var_in_data_refused() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0", "--piece", "app"]);
    let before = lock_bytes(&repo);
    update(&repo, &["--data", "name=other"]).code(2);
    assert_eq!(lock_bytes(&repo), before);
    assert_eq!(porcelain(&repo), "");
}

#[test]
fn prerelease_lock_without_newer_stable_is_up_to_date() {
    let tpl = template_repo_tagged(&[("v1", "v1.0.0"), ("v2", "v2.0.0-rc.1")]);
    let repo = new_repo(&tpl, &["--ref", "v2.0.0-rc.1"]);
    let before = lock_bytes(&repo);
    let out = update(&repo, &[]).success().get_output().stdout.clone();
    assert!(
        messages(&json(&out)).contains(
            "up to date (no newer stable than 2.0.0-rc.1; use --to <tag> for a prerelease)"
        )
    );
    assert_eq!(lock_bytes(&repo), before);
    assert_eq!(porcelain(&repo), "");
    // An explicit older --to is still a downgrade.
    update(&repo, &["--to", "1.0.0"]).code(2);
}

#[test]
fn removed_piece_is_template_error() {
    let tpl = template_repo_tagged(&[("v1", "v1.0.0"), ("v2-nolib", "v2.0.0")]);
    let repo = new_repo(
        &tpl,
        &["--ref", "v1.0.0", "--piece", "app", "--piece", "lib"],
    );
    let before = lock_bytes(&repo);
    let out = update(&repo, &[]).code(3).get_output().stdout.clone();
    assert!(messages(&json(&out)).contains("lib"));
    assert_eq!(lock_bytes(&repo), before);
    assert_eq!(porcelain(&repo), "");
}

#[test]
fn remove_paths_reach_the_planner() {
    // A [[remove]] of a path the target still renders is ignored with a warning: proof that the
    // rendered `[[remove]]` paths are passed to the planner.
    let tpl = template_repo(&["v1", "v2"]);
    let mig = tpl.path().join("migrations/2.0.0.toml");
    let text = std::fs::read_to_string(&mig).unwrap();
    std::fs::write(&mig, format!("{text}\n[[remove]]\npath = \"notes.txt\"\n")).unwrap();
    amend_tip(tpl.path(), "v2.0.0");
    let repo = new_repo(&tpl, &["--ref", "v1.0.0"]);
    let out = update(&repo, &[]).success().get_output().stdout.clone();
    assert!(messages(&json(&out)).contains("still renders it"));
    assert!(repo.path().join("notes.txt").exists());
}
