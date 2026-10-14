mod common;
use common::{splice_cmd, template_repo, template_repo_min_splice, tree_snapshot};
use std::path::Path;
use tempfile::tempdir;

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// Replace values that vary per run (temp template path, commit sha, lock file hash).
fn redact(snapshot: &str) -> String {
    snapshot
        .lines()
        .map(|l| {
            if l.starts_with("source = ") {
                "source = \"<template>\"".to_string()
            } else if l.starts_with("commit = ") {
                "commit = \"<sha>\"".to_string()
            } else if l.starts_with(".splice/lock.toml ") {
                ".splice/lock.toml <hash>".to_string()
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn json(out: Vec<u8>) -> serde_json::Value {
    serde_json::from_slice(&out).unwrap()
}

fn base_args<'a>(tpl: &'a Path, dest: &'a Path) -> Vec<&'a str> {
    vec![
        "new",
        s(tpl),
        s(dest),
        "--data",
        "project=demo",
        "--yes",
        "--json",
    ]
}

#[test]
fn new_renders_fixture_and_writes_lock() {
    let tpl = template_repo(&["v1"]);
    let dest = tempdir().unwrap();
    let out = splice_cmd()
        .args([
            "new",
            s(tpl.path()),
            s(dest.path()),
            "--piece",
            "app:web",
            "--data",
            "project=demo",
            "--yes",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let r = json(out);
    assert_eq!(r["ok"], true);
    assert_eq!(r["command"], "new");
    assert!(dest.path().join(".splice/lock.toml").exists());
    assert!(dest.path().join(".git").exists());
    assert!(dest.path().join(".post-ran").exists());
    insta::assert_snapshot!(redact(&tree_snapshot(dest.path())));
    insta::assert_json_snapshot!(r);
}

#[test]
fn missing_var_noninteractive_is_usage_json() {
    let tpl = template_repo(&["v1"]);
    let dest = tempdir().unwrap();
    let out = splice_cmd()
        .args(["new", s(tpl.path()), s(dest.path()), "--json"])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let r = json(out);
    assert_eq!(r["ok"], false);
    assert!(
        r["messages"][0]
            .as_str()
            .unwrap()
            .contains("--data project=")
    );
    assert_eq!(std::fs::read_dir(dest.path()).unwrap().count(), 0);
}

#[test]
fn nonempty_dest_refused_without_force() {
    let tpl = template_repo(&["v1"]);
    let dest = tempdir().unwrap();
    std::fs::write(dest.path().join("x.txt"), "x").unwrap();
    splice_cmd()
        .args(base_args(tpl.path(), dest.path()))
        .assert()
        .code(2);
    assert!(!dest.path().join(".splice").exists());
    // --force: owned/shared overwritten, seeds kept.
    std::fs::write(dest.path().join("README.md"), "mine\n").unwrap();
    std::fs::write(dest.path().join(".editorconfig"), "old\n").unwrap();
    let mut a = base_args(tpl.path(), dest.path());
    a.push("--force");
    let r = json(
        splice_cmd()
            .args(a)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    );
    assert_eq!(
        std::fs::read_to_string(dest.path().join("README.md")).unwrap(),
        "mine\n"
    );
    assert_ne!(
        std::fs::read_to_string(dest.path().join(".editorconfig")).unwrap(),
        "old\n"
    );
    assert!(dest.path().join("x.txt").exists());
    let ch = r["changes"].as_array().unwrap();
    assert!(
        ch.iter()
            .any(|c| c["path"] == "README.md" && c["action"] == "skip")
    );
    assert!(
        ch.iter()
            .any(|c| c["path"] == ".editorconfig" && c["action"] == "update")
    );
}

#[test]
fn existing_lock_is_usage_even_with_force() {
    let tpl = template_repo(&["v1"]);
    let dest = tempdir().unwrap();
    splice_cmd()
        .args(base_args(tpl.path(), dest.path()))
        .assert()
        .success();
    let mut a = base_args(tpl.path(), dest.path());
    a.push("--force");
    splice_cmd().args(a).assert().code(2);
}

#[test]
fn splice_regular_file_is_clear_error() {
    let tpl = template_repo(&["v1"]);
    let dest = tempdir().unwrap();
    std::fs::write(dest.path().join(".splice"), "x").unwrap();
    let mut a = base_args(tpl.path(), dest.path());
    a.push("--force");
    let out = splice_cmd()
        .args(a)
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let m = json(out)["messages"][0].as_str().unwrap().to_string();
    assert!(
        m.contains(".splice") && m.contains("not a directory"),
        "{m}"
    );
}

#[test]
fn requires_checked() {
    let tpl = template_repo(&["v1"]);
    let dest = tempdir().unwrap();
    let mut a = base_args(tpl.path(), dest.path());
    a.extend(["--piece", "lib"]);
    let out = splice_cmd()
        .args(a)
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let m = json(out)["messages"][0].as_str().unwrap().to_string();
    assert!(m.contains("app"), "{m}");
    assert_eq!(std::fs::read_dir(dest.path()).unwrap().count(), 0);
}

#[test]
fn bad_pattern_data_is_usage() {
    let tpl = template_repo(&["v1"]);
    let dest = tempdir().unwrap();
    let mut a = base_args(tpl.path(), dest.path());
    a.extend(["--data", "scope=Bad!"]);
    splice_cmd().args(a).assert().code(2);
    assert_eq!(std::fs::read_dir(dest.path()).unwrap().count(), 0);
}

#[test]
fn unsafe_piece_name_and_name_var_are_usage() {
    let tpl = template_repo(&["v1"]);
    for extra in [
        vec!["--piece", "app:../x"],
        vec!["--piece", "app:a/b"],
        vec!["--piece", "app", "--data", "name=../x"],
        vec!["--piece", "nope"],
        vec!["--piece", "base"],
        vec!["--piece", "app:web", "--piece", "app:web"],
        vec!["--data", "bogus=1"],
        vec!["--data", "noequals"],
    ] {
        let dest = tempdir().unwrap();
        let mut a = base_args(tpl.path(), dest.path());
        a.extend(extra.iter().copied());
        splice_cmd().args(a).assert().code(2);
        assert_eq!(
            std::fs::read_dir(dest.path()).unwrap().count(),
            0,
            "{extra:?}"
        );
    }
}

#[test]
fn explicit_name_sets_name_var() {
    let tpl = template_repo(&["v1"]);
    let dest = tempdir().unwrap();
    let mut a = base_args(tpl.path(), dest.path());
    a.extend(["--piece", "app:site", "--piece", "app:docs"]);
    splice_cmd().args(a).assert().success();
    assert!(dest.path().join("apps/site/tsconfig.json").exists());
    assert!(dest.path().join("apps/docs/tsconfig.json").exists());
}

#[test]
fn no_post_flag() {
    let tpl = template_repo(&["v1"]);
    let dest = tempdir().unwrap();
    let mut a = base_args(tpl.path(), dest.path());
    a.push("--no-post");
    splice_cmd().args(a).assert().success();
    assert!(!dest.path().join(".post-ran").exists());
}

#[test]
fn post_output_goes_to_stderr_in_json_mode() {
    let tpl = template_repo(&["v1"]);
    let dest = tempdir().unwrap();
    let out = splice_cmd()
        .args(base_args(tpl.path(), dest.path()))
        .assert()
        .success();
    let o = out.get_output();
    serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap();
}

#[test]
fn min_splice_refused() {
    let tpl = template_repo_min_splice("99.0.0");
    let dest = tempdir().unwrap();
    let out = splice_cmd()
        .args([
            "new",
            s(tpl.path()),
            s(dest.path()),
            "--ref",
            "v1.0.0",
            "--data",
            "project=demo",
            "--yes",
            "--json",
        ])
        .assert()
        .code(3)
        .get_output()
        .stdout
        .clone();
    assert!(
        json(out)["messages"][0]
            .as_str()
            .unwrap()
            .contains("99.0.0")
    );
    assert_eq!(std::fs::read_dir(dest.path()).unwrap().count(), 0);
}

#[test]
fn git_init_skipped_inside_a_work_tree() {
    let tpl = template_repo(&["v1"]);
    let outer = tempdir().unwrap();
    common::git_commit_all_init(outer.path());
    let dest = outer.path().join("sub");
    splice_cmd()
        .args(base_args(tpl.path(), &dest))
        .assert()
        .success();
    assert!(!dest.join(".git").exists());
}

#[test]
fn manifest_warnings_are_report_messages() {
    let tpl = template_repo_min_splice("0.1.0");
    // add an unknown key and commit as a new tag
    let p = tpl.path().join("splice.toml");
    let t = std::fs::read_to_string(&p).unwrap();
    std::fs::write(
        &p,
        t.replace("base = \"base\"\n", "base = \"base\"\nfuture_key = 1\n"),
    )
    .unwrap();
    common::git_commit_all(tpl.path(), "warn");
    common::tag(&tpl, "v1.1.0");
    let dest = tempdir().unwrap();
    let out = splice_cmd()
        .args(base_args(tpl.path(), dest.path()))
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let r = json(out);
    assert!(r["messages"].to_string().contains("future_key"), "{r}");
}
