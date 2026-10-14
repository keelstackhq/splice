mod common;
use common::{git, git_commit_all, splice_cmd, tag, template_repo, tree_snapshot};
use splice::lock::Lock;
use std::path::Path;
use tempfile::TempDir;

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// A committed repo created by `splice new` at `at` (e.g. `v1.0.0`) with extra args.
fn new_repo(tpl: &TempDir, at: &str, extra: &[&str]) -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    splice_cmd()
        .args(["new", s(tpl.path()), s(repo.path()), "--ref", at])
        .args(extra)
        .args(["--data", "project=demo", "--yes", "--json"])
        .assert()
        .success();
    git_commit_all(repo.path(), "new");
    repo
}

fn read(repo: &TempDir, rel: &str) -> String {
    std::fs::read_to_string(repo.path().join(rel)).unwrap()
}

fn json(out: Vec<u8>) -> serde_json::Value {
    serde_json::from_slice(&out).unwrap()
}

fn lock_bytes(repo: &TempDir) -> Vec<u8> {
    std::fs::read(repo.path().join(".splice/lock.toml")).unwrap()
}

fn porcelain(repo: &TempDir) -> String {
    let o = std::process::Command::new("git")
        .current_dir(repo.path())
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    String::from_utf8(o.stdout).unwrap()
}

#[test]
fn dirty_tree_refused() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, "v1.0.0", &[]);
    std::fs::write(repo.path().join("x"), "x").unwrap();
    splice_cmd()
        .args(["update", "-C", s(repo.path()), "--yes"])
        .assert()
        .code(2);
    assert_eq!(
        Lock::read(repo.path()).unwrap().unwrap().template.version,
        semver::Version::new(1, 0, 0)
    );
    splice_cmd()
        .args(["update", "-C", s(repo.path()), "--yes", "--allow-dirty"])
        .assert()
        .success();
    assert_eq!(
        Lock::read(repo.path()).unwrap().unwrap().template.version,
        semver::Version::new(2, 0, 0)
    );
}

#[test]
fn no_work_tree_refused_unless_allow_dirty() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, "v1.0.0", &[]);
    std::fs::remove_dir_all(repo.path().join(".git")).unwrap();
    splice_cmd()
        .args(["update", "-C", s(repo.path()), "--yes"])
        .assert()
        .code(2);
    splice_cmd()
        .args(["update", "-C", s(repo.path()), "--yes", "--allow-dirty"])
        .assert()
        .success();
}

/// A project in `sub` of a committed parent repo (a monorepo).
fn monorepo(tpl: &TempDir) -> TempDir {
    let parent = tempfile::tempdir().unwrap();
    git(parent.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(parent.path().join("other.txt"), "o\n").unwrap();
    let sub = parent.path().join("sub");
    splice_cmd()
        .args(["new", s(tpl.path()), s(&sub), "--ref", "v1.0.0"])
        .args(["--data", "project=demo", "--yes"])
        .assert()
        .success();
    git_commit_all(parent.path(), "all");
    parent
}

#[test]
fn project_in_a_monorepo_subdir_updates() {
    let tpl = template_repo(&["v1", "v2"]);
    let parent = monorepo(&tpl);
    let sub = parent.path().join("sub");
    splice_cmd()
        .args(["update", "-C", s(&sub), "--yes"])
        .assert()
        .success();
}

#[test]
fn dirt_inside_the_subdir_refuses_but_outside_is_allowed() {
    let tpl = template_repo(&["v1", "v2"]);
    let parent = monorepo(&tpl);
    let sub = parent.path().join("sub");
    std::fs::write(parent.path().join("other.txt"), "changed\n").unwrap();
    std::fs::write(parent.path().join("untracked.txt"), "u\n").unwrap();
    std::fs::write(sub.join("x"), "x").unwrap();
    splice_cmd()
        .args(["update", "-C", s(&sub), "--yes"])
        .assert()
        .code(2);
    std::fs::remove_file(sub.join("x")).unwrap();
    splice_cmd()
        .args(["update", "-C", s(&sub), "--yes"])
        .assert()
        .success();
}

#[test]
fn data_is_validated_even_when_up_to_date() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, "v2.0.0", &[]);
    for k in ["scope=x", "nope=1"] {
        splice_cmd()
            .args(["update", "-C", s(repo.path()), "--yes", "--data", k])
            .assert()
            .code(2);
    }
}

#[test]
fn lock_at_a_newer_prerelease_is_up_to_date() {
    let tpl = template_repo(&["v1", "v2"]);
    tag(&tpl, "v3.0.0-rc.1");
    let repo = new_repo(&tpl, "v3.0.0-rc.1", &[]);
    let before = lock_bytes(&repo);
    let out = splice_cmd()
        .args(["update", "-C", s(repo.path()), "--yes", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(json(out)["messages"].to_string().contains("up to date"));
    assert_eq!(lock_bytes(&repo), before);
    splice_cmd()
        .args(["update", "-C", s(repo.path()), "--yes", "--to", "1.0.0"])
        .assert()
        .code(2);
}

#[test]
fn lock_from_a_ref_says_so_and_suggests_to() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, "main", &[]);
    let out = splice_cmd()
        .args(["update", "-C", s(repo.path()), "--yes", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let m = json(out)["messages"].to_string();
    assert!(m.contains("ref") && m.contains("--to"), "{m}");
    assert!(!m.contains("moved"), "{m}");
}

#[test]
fn up_to_date_is_noop() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, "v2.0.0", &[]);
    let before = lock_bytes(&repo);
    let out = splice_cmd()
        .args(["update", "-C", s(repo.path()), "--yes", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json(out);
    assert_eq!(v["command"], "update");
    assert_eq!(v["changes"].as_array().unwrap().len(), 0);
    assert!(v["messages"].to_string().contains("up to date"), "{v}");
    assert_eq!(lock_bytes(&repo), before);
    assert_eq!(porcelain(&repo), "");
}

#[test]
fn downgrade_refused() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, "v2.0.0", &[]);
    splice_cmd()
        .args(["update", "-C", s(repo.path()), "--yes", "--to", "1.0.0"])
        .assert()
        .code(2);
}

#[test]
fn min_splice_refused_before_write() {
    let tpl = template_repo(&["v1", "v2", "v3"]);
    let repo = new_repo(&tpl, "v1.0.0", &[]);
    splice_cmd()
        .args(["update", "-C", s(repo.path()), "--yes", "--to", "3.0.0"])
        .assert()
        .code(3);
    assert_eq!(porcelain(&repo), "");
}

#[test]
fn owned_drift_refused_then_forced() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, "v1.0.0", &["--piece", "app:web"]);
    let p = repo.path().join("apps/web/tsconfig.json");
    std::fs::write(&p, "{\"mine\":true}\n").unwrap();
    git_commit_all(repo.path(), "edit");
    let before = tree_snapshot(repo.path());
    let out = splice_cmd()
        .args(["update", "-C", s(repo.path()), "--yes", "--json"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let v = json(out);
    assert_eq!(v["ok"], false);
    assert_eq!(v["changes"].as_array().unwrap().len(), 0, "{v}");
    assert!(
        v["messages"].to_string().contains("apps/web/tsconfig.json"),
        "{v}"
    );
    assert_eq!(tree_snapshot(repo.path()), before);
    assert_eq!(porcelain(&repo), "");

    let out = splice_cmd()
        .args(["update", "-C", s(repo.path()), "--yes", "--force", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json(out);
    assert!(
        v["changes"].as_array().unwrap().iter().any(|c| {
            c["path"] == "apps/web/tsconfig.json"
                && c["action"] == "update"
                && c["class"] == "owned"
        }),
        "{v}"
    );
    assert!(!read(&repo, "apps/web/tsconfig.json").contains("mine"));
}

#[test]
fn new_var_takes_default_with_yes() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, "v1.0.0", &[]);
    splice_cmd()
        .args(["update", "-C", s(repo.path()), "--yes"])
        .assert()
        .success();
    let lock = Lock::read(repo.path()).unwrap().unwrap();
    assert_eq!(lock.vars["license"], "MIT");
    assert_eq!(lock.template.version, semver::Version::new(2, 0, 0));
}

#[test]
fn new_var_without_default_is_usage_error_unless_given() {
    let tpl = template_repo(&["v1"]);
    let p = tpl.path().join("splice.toml");
    let mut text = std::fs::read_to_string(&p).unwrap();
    text.push_str("extra = { prompt = \"extra\" }\n");
    std::fs::write(&p, text).unwrap();
    git_commit_all(tpl.path(), "v2");
    tag(&tpl, "v2.0.0");
    let repo = new_repo_at_v1_without_extra(&tpl);
    let before = tree_snapshot(repo.path());
    let out = splice_cmd()
        .args(["update", "-C", s(repo.path()), "--yes", "--json"])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let m = json(out)["messages"].to_string();
    assert!(m.contains("extra") && m.contains("--data"), "{m}");
    assert_eq!(tree_snapshot(repo.path()), before);
    splice_cmd()
        .args([
            "update",
            "-C",
            s(repo.path()),
            "--yes",
            "--data",
            "extra=hi",
        ])
        .assert()
        .success();
    assert_eq!(
        Lock::read(repo.path()).unwrap().unwrap().vars["extra"],
        "hi"
    );
}

fn new_repo_at_v1_without_extra(tpl: &TempDir) -> TempDir {
    new_repo(tpl, "v1.0.0", &[])
}

#[test]
fn data_for_a_var_that_is_not_new_is_usage_error() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, "v1.0.0", &[]);
    splice_cmd()
        .args(["update", "-C", s(repo.path()), "--yes", "--data", "scope=x"])
        .assert()
        .code(2);
    assert_eq!(porcelain(&repo), "");
}

#[test]
fn removed_piece_is_template_error() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, "v1.0.0", &["--piece", "app", "--piece", "lib"]);
    std::fs::remove_dir_all(tpl.path().join("pieces/lib")).unwrap();
    git_commit_all(tpl.path(), "drop lib");
    tag(&tpl, "v3.0.0");
    let out = splice_cmd()
        .args(["update", "-C", s(repo.path()), "--yes", "--json"])
        .assert()
        .code(3)
        .get_output()
        .stdout
        .clone();
    assert!(json(out)["messages"].to_string().contains("lib"));
    assert_eq!(porcelain(&repo), "");
}

#[test]
fn lock_warnings_are_messages() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, "v2.0.0", &[]);
    let mut text = String::from_utf8(lock_bytes(&repo)).unwrap();
    text = text.replacen("[template]", "bogus_key = 1\n\n[template]", 1);
    std::fs::write(repo.path().join(".splice/lock.toml"), text).unwrap();
    git_commit_all(repo.path(), "lock");
    let out = splice_cmd()
        .args(["update", "-C", s(repo.path()), "--yes", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(json(out)["messages"].to_string().contains("bogus_key"));
}

#[cfg(unix)]
#[test]
fn symlinked_target_path_is_template_error() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, "v1.0.0", &[]);
    std::fs::remove_file(repo.path().join(".editorconfig")).unwrap();
    std::os::unix::fs::symlink("notes.txt", repo.path().join(".editorconfig")).unwrap();
    git_commit_all(repo.path(), "link");
    let out = splice_cmd()
        .args(["update", "-C", s(repo.path()), "--yes", "--json"])
        .assert()
        .code(3)
        .get_output()
        .stdout
        .clone();
    assert!(json(out)["messages"].to_string().contains("unsafe path"));
    assert_eq!(porcelain(&repo), "");
}

#[test]
fn update_applies_v2_and_runs_step_and_post() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, "v1.0.0", &[]);
    let out = splice_cmd()
        .args(["update", "-C", s(repo.path()), "--yes", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json(out);
    assert_eq!(v["ok"], true);
    assert!(repo.path().join("tool.config.ts").exists());
    assert!(!repo.path().join("tool.config.mts").exists());
    assert!(repo.path().join("CHANGELOG.md").exists());
    assert!(repo.path().join(".splice/migrated.txt").exists());
}
