//! `splice adopt` end to end against the fixture template.

mod common;

use common::{commit_all, git_stdout, splice_cmd, template_repo};
use std::path::Path;
use tempfile::TempDir;

fn s(d: &TempDir) -> &str {
    d.path().to_str().unwrap()
}

/// A committed repo holding `files` (path, content).
fn repo_with(files: &[(&str, &str)]) -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("unrelated.txt"), "keep me\n").unwrap();
    for (p, c) in files {
        let full = repo.path().join(p);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, c).unwrap();
    }
    commit_all(repo.path());
    repo
}

fn adopt(tpl: &TempDir, repo: &Path, extra: &[&str]) -> assert_cmd::assert::Assert {
    splice_cmd()
        .args(["adopt", s(tpl), "-C", repo.to_str().unwrap()])
        .args(["--data", "project=demo", "--yes", "--no-post", "--json"])
        .args(extra)
        .assert()
}

fn json(a: assert_cmd::assert::Assert) -> serde_json::Value {
    serde_json::from_slice(&a.get_output().stdout).expect("one JSON object")
}

fn action<'a>(r: &'a serde_json::Value, path: &str) -> &'a str {
    r["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["path"] == path)
        .unwrap_or_else(|| panic!("no change for {path}: {r}"))["action"]
        .as_str()
        .unwrap()
}

fn msgs(r: &serde_json::Value) -> Vec<String> {
    r["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap().to_string())
        .collect()
}

fn lock(repo: &TempDir) -> std::path::PathBuf {
    repo.path().join(".splice/lock.toml")
}

#[test]
fn adopt_creates_files_and_keeps_unrelated() {
    let tpl = template_repo(&["v1"]);
    let repo = repo_with(&[]);
    let r = json(adopt(&tpl, repo.path(), &[]).success());
    assert_eq!(r["command"], "adopt");
    assert_eq!(action(&r, ".editorconfig"), "create");
    assert!(repo.path().join("package.json").exists());
    assert_eq!(
        std::fs::read_to_string(repo.path().join("unrelated.txt")).unwrap(),
        "keep me\n"
    );
    assert!(lock(&repo).exists());
    splice_cmd()
        .args(["check", "--offline", "-C", s(&repo)])
        .assert()
        .success();
}

#[test]
fn owned_drift_refuses_then_force_replaces() {
    let tpl = template_repo(&["v1"]);
    let repo = repo_with(&[(".editorconfig", "mine\n")]);
    let r = json(adopt(&tpl, repo.path(), &[]).code(1));
    assert_eq!(action(&r, ".editorconfig"), "conflict");
    assert!(
        msgs(&r)
            .iter()
            .any(|m| m.starts_with("refuse: `.editorconfig`")),
        "{r}"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join(".editorconfig")).unwrap(),
        "mine\n"
    );
    assert!(!lock(&repo).exists());
    assert!(!repo.path().join("package.json").exists());

    let r = json(adopt(&tpl, repo.path(), &["--force"]).success());
    assert!(
        msgs(&r)
            .iter()
            .any(|m| m.starts_with("replace: `.editorconfig`") && m.ends_with("replaced")),
        "{r}"
    );
    assert_ne!(
        std::fs::read_to_string(repo.path().join(".editorconfig")).unwrap(),
        "mine\n"
    );
    assert!(lock(&repo).exists());
}

#[test]
fn owned_identical_is_skipped() {
    let tpl = template_repo(&["v1"]);
    let first = repo_with(&[]);
    json(adopt(&tpl, first.path(), &[]).success());
    let same = std::fs::read(first.path().join(".editorconfig")).unwrap();
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join(".editorconfig"), same).unwrap();
    commit_all(repo.path());
    let r = json(adopt(&tpl, repo.path(), &[]).success());
    assert_eq!(action(&r, ".editorconfig"), "skip");
}

#[test]
fn shared_merges_or_conflicts_with_markers() {
    let tpl = template_repo(&["v1"]);
    let repo = repo_with(&[
        ("config/settings.toml", "[extra]\nx = 1\n"),
        ("notes.txt", "mine\n"),
    ]);
    let r = json(adopt(&tpl, repo.path(), &[]).code(1));
    assert_eq!(action(&r, "config/settings.toml"), "merge");
    assert_eq!(action(&r, "notes.txt"), "conflict");
    let toml = std::fs::read_to_string(repo.path().join("config/settings.toml")).unwrap();
    assert!(
        toml.contains("x = 1") && toml.contains("[settings]"),
        "{toml}"
    );
    let notes = std::fs::read_to_string(repo.path().join("notes.txt")).unwrap();
    assert!(
        notes.contains("<<<<<<<") && notes.contains("mine"),
        "{notes}"
    );
    assert!(lock(&repo).exists());
    // The lock is written, but `check` fails until every marker is resolved.
    let check = |repo: &TempDir| {
        splice_cmd()
            .args(["check", "--offline", "-C", s(repo), "--json"])
            .assert()
    };
    let r = json(check(&repo).code(1));
    assert_eq!(action(&r, "notes.txt"), "conflict", "{r}");
    std::fs::write(repo.path().join("notes.txt"), "mine\n").unwrap();
    check(&repo).success();
}

#[test]
fn seed_is_kept() {
    let tpl = template_repo(&["v1"]);
    let repo = repo_with(&[("README.md", "my readme\n")]);
    let r = json(adopt(&tpl, repo.path(), &[]).success());
    assert_eq!(action(&r, "README.md"), "skip");
    assert_eq!(
        std::fs::read(repo.path().join("README.md")).unwrap(),
        b"my readme\n"
    );
}

#[test]
fn dry_run_writes_nothing_and_mirrors_exit() {
    let tpl = template_repo(&["v1"]);
    let repo = repo_with(&[(".editorconfig", "mine\n"), ("README.md", "r\n")]);
    let r = json(adopt(&tpl, repo.path(), &["--dry-run"]).code(1));
    assert_eq!(action(&r, ".editorconfig"), "conflict");
    assert_eq!(action(&r, "README.md"), "skip");
    assert_eq!(action(&r, "package.json"), "create");
    assert!(
        msgs(&r)
            .iter()
            .any(|m| m.starts_with("refuse: `.editorconfig`")),
        "{r}"
    );
    assert!(msgs(&r).iter().any(|m| m == "dry run: nothing written"));
    assert_eq!(git_stdout(repo.path(), &["status", "--porcelain"]), "");
    assert!(!lock(&repo).exists());

    let r = json(adopt(&tpl, repo.path(), &["--dry-run", "--force"]).success());
    assert!(!msgs(&r).iter().any(|m| m.starts_with("refuse: ")), "{r}");
    let replace = msgs(&r)
        .into_iter()
        .find(|m| m.starts_with("replace: `.editorconfig`"))
        .unwrap_or_else(|| panic!("{r}"));
    assert!(replace.ends_with("would be replaced"), "{replace}");
    assert_eq!(action(&r, ".editorconfig"), "update");
    assert_eq!(git_stdout(repo.path(), &["status", "--porcelain"]), "");
}

#[test]
fn existing_lock_is_usage_error() {
    let tpl = template_repo(&["v1"]);
    let repo = repo_with(&[]);
    json(adopt(&tpl, repo.path(), &[]).success());
    commit_all(repo.path());
    adopt(&tpl, repo.path(), &[]).code(2);
}

#[test]
fn dirty_tree_refused_unless_allowed() {
    let tpl = template_repo(&["v1"]);
    let repo = repo_with(&[]);
    std::fs::write(repo.path().join("wip.txt"), "x\n").unwrap();
    adopt(&tpl, repo.path(), &[]).code(2);
    assert!(!lock(&repo).exists());
    adopt(&tpl, repo.path(), &["--allow-dirty"]).success();
    assert!(lock(&repo).exists());
}

fn post_template(run: &str) -> TempDir {
    let tpl = template_repo(&["v1"]);
    let path = tpl.path().join("pieces/base/piece.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, text.replace("echo post> .post-ran", run)).unwrap();
    common::amend_tip(tpl.path(), "v1.0.0");
    tpl
}

fn adopt_with_post(tpl: &TempDir, repo: &Path) -> assert_cmd::assert::Assert {
    splice_cmd()
        .args(["adopt", s(tpl), "-C", repo.to_str().unwrap()])
        .args(["--data", "project=demo", "--yes", "--json"])
        .assert()
}

#[test]
fn failing_post_leaves_no_lock() {
    let tpl = post_template("exit 7");
    let repo = repo_with(&[]);
    let r = json(adopt_with_post(&tpl, repo.path()).code(3));
    assert!(!lock(&repo).exists());
    assert!(
        msgs(&r)
            .iter()
            .any(|m| m.contains("no .splice/lock.toml was written")),
        "{r}"
    );
}

#[test]
fn post_runs_then_lock_is_written() {
    let tpl = post_template("touch .post-marker");
    let repo = repo_with(&[]);
    json(adopt_with_post(&tpl, repo.path()).success());
    assert!(repo.path().join(".post-marker").exists());
    assert!(lock(&repo).exists());
}

#[cfg(unix)]
#[test]
fn dry_run_checks_targets() {
    let tpl = template_repo(&["v1"]);
    let outside = tempfile::tempdir().unwrap();
    let repo = repo_with(&[]);
    std::os::unix::fs::symlink(outside.path(), repo.path().join("config")).unwrap();
    commit_all(repo.path());
    adopt(&tpl, repo.path(), &["--dry-run"]).code(3);
    assert!(!outside.path().join("settings.toml").exists());
}

#[test]
fn dry_run_shared_conflict_exits_1_and_writes_nothing() {
    let tpl = template_repo(&["v1"]);
    let repo = repo_with(&[("notes.txt", "mine\n")]);
    let r = json(adopt(&tpl, repo.path(), &["--dry-run"]).code(1));
    assert_eq!(action(&r, "notes.txt"), "conflict");
    assert_eq!(git_stdout(repo.path(), &["status", "--porcelain"]), "");
    assert!(!lock(&repo).exists());
}

#[test]
fn outside_a_git_work_tree_is_usage_error() {
    let tpl = template_repo(&["v1"]);
    let dir = tempfile::tempdir().unwrap();
    adopt(&tpl, dir.path(), &[]).code(2);
    assert!(!lock(&dir).exists());
}

/// A template that ships a file into splice's own state (`.splice/lock.toml`, `.splice/channels/`)
/// is a template error (exit 3) and nothing is written; `.splice/migrations/` stays allowed
/// (contract §7: migration scripts ship there as owned files).
#[test]
fn template_cannot_plant_splice_state() {
    fn tpl_with(path: &str) -> TempDir {
        let toml = format!(
            "id = \"base\"\ndescription = \"d\"\nmultiple = false\n[files]\nowned = [\"{path}\"]\n"
        );
        let tpl = common::inline_template(&[("base", &toml, &[(path, "x\n")])]);
        common::git(
            tpl.path(),
            "2026-01-01T00:00:00+00:00",
            &["init", "-q", "-b", "main"],
        );
        common::commit_tag(tpl.path(), "v1", "v1.0.0");
        tpl
    }
    let run = |tpl: &TempDir, repo: &Path, extra: &[&str]| {
        splice_cmd()
            .args([
                "adopt",
                s(tpl),
                "-C",
                repo.to_str().unwrap(),
                "--yes",
                "--no-post",
                "--json",
            ])
            .args(extra)
            .assert()
    };
    for planted in [".splice/lock.toml", ".splice/channels/evil.toml"] {
        let tpl = tpl_with(planted);
        let repo = repo_with(&[]);
        for extra in [&["--dry-run"][..], &[][..]] {
            let out = run(&tpl, repo.path(), extra).code(3);
            let err = String::from_utf8_lossy(&out.get_output().stderr).to_string()
                + &String::from_utf8_lossy(&out.get_output().stdout);
            assert!(err.contains("is reserved"), "{planted} {extra:?}: {err}");
            assert!(!repo.path().join(planted).exists(), "{planted} {extra:?}");
            assert!(
                !repo.path().join(".splice/lock.toml").exists(),
                "{planted} {extra:?}"
            );
        }
    }
    let tpl = tpl_with(".splice/migrations/1.0.0-x.mjs");
    let repo = repo_with(&[]);
    run(&tpl, repo.path(), &[]).success();
    assert!(repo.path().join(".splice/migrations/1.0.0-x.mjs").exists());
}
