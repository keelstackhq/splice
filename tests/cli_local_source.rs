mod common;
use common::{git, git_commit_all, splice_cmd, template_repo};
use predicates::prelude::*;
use std::path::Path;
use tempfile::TempDir;

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// `$R/tpl` (a clone of the v1+v2 template, tags included) and `$R/demo` made from `./tpl`.
fn setup(source: &str, dest: &str) -> TempDir {
    let origin = template_repo(&["v1", "v2"]);
    let root = tempfile::tempdir().unwrap();
    git(
        root.path(),
        &[
            "clone",
            "-q",
            s(origin.path()),
            &root.path().join("tpl").to_string_lossy(),
        ],
    );
    splice_cmd()
        .current_dir(root.path())
        .args(["new", source, dest, "--ref", "v1.0.0", "--piece", "app:web"])
        .args(["--data", "project=demo", "--yes"])
        .assert()
        .success();
    git_commit_all(&root.path().join(dest), "new");
    root
}

fn lock_source(repo: &Path) -> String {
    let t: toml::Value =
        toml::from_str(&std::fs::read_to_string(repo.join(".splice/lock.toml")).unwrap()).unwrap();
    t["template"]["source"].as_str().unwrap().to_string()
}

#[test]
fn new_stores_source_relative_to_dest() {
    let root = setup("./tpl", "demo");
    assert_eq!(lock_source(&root.path().join("demo")), "../tpl");
}

#[test]
fn new_keeps_subdir_and_rewrites_nested_dest() {
    let root = setup("./tpl", "a/b");
    assert_eq!(lock_source(&root.path().join("a/b")), "../../tpl");
}

#[test]
fn new_keeps_absolute_source_as_given() {
    let origin = template_repo(&["v1"]);
    let root = tempfile::tempdir().unwrap();
    splice_cmd()
        .current_dir(root.path())
        .args([
            "new",
            s(origin.path()),
            "demo",
            "--data",
            "project=demo",
            "--yes",
        ])
        .assert()
        .success();
    assert_eq!(lock_source(&root.path().join("demo")), s(origin.path()));
}

#[test]
fn commands_work_from_inside_the_repo_with_a_relative_source() {
    let root = setup("./tpl", "demo");
    let demo = root.path().join("demo");
    splice_cmd()
        .current_dir(&demo)
        .args(["explain", "apps/web/package.json"])
        .assert()
        .success();
    splice_cmd()
        .current_dir(&demo)
        .args(["check"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("update available: 1.0.0"));
    splice_cmd()
        .current_dir(&demo)
        .args(["diff"])
        .assert()
        .code(predicate::in_iter([0, 1]));
    splice_cmd()
        .current_dir(&demo)
        .args(["add", "app", "--name", "admin", "--yes"])
        .assert()
        .success();
    git_commit_all(&demo, "add");
    splice_cmd()
        .current_dir(&demo)
        .args(["update", "--yes"])
        .assert()
        .success();
}

#[test]
fn legacy_lock_source_resolves_from_the_repo_root_not_cwd() {
    let root = setup("./tpl", "demo");
    let demo = root.path().join("demo");
    // a lock written as "../tpl" works from a subdirectory via -C too
    splice_cmd()
        .current_dir(root.path())
        .args(["-C", "demo", "explain", "package.json"])
        .assert()
        .success();
    assert!(demo.join(".splice/lock.toml").exists());
}

#[test]
fn missing_local_source_is_a_clear_error_and_never_uses_the_repo_git() {
    let root = setup("./tpl", "demo");
    let demo = root.path().join("demo");
    git(&demo, &["init", "-q", "-b", "main"]);
    std::fs::remove_dir_all(root.path().join("tpl")).unwrap();
    // the tree is cached, so only the online lookups touch the source
    let found = || {
        predicate::str::contains("template source ../tpl not found")
            .and(predicate::str::contains("resolved to"))
    };
    splice_cmd()
        .current_dir(&demo)
        .args(["update", "--yes"])
        .assert()
        .code(3)
        .stderr(found());
    splice_cmd()
        .current_dir(&demo)
        .args(["check"])
        .assert()
        .success()
        .stderr(found());
    // a plain directory that is not a git repo, inside the generated repo's own work tree
    std::fs::create_dir(demo.join("notgit")).unwrap();
    splice_cmd()
        .current_dir(root.path())
        .args(["new", "./demo/notgit", "x", "--data", "project=x", "--yes"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "template source ./demo/notgit not found",
        ));
}
