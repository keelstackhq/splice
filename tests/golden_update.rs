//! Golden `splice update` v1 → v2 with user edits on every class, plus crash recovery (spec §8.3).

mod common;

use common::{git, splice_cmd, template_repo, tree_snapshot};
use splice::lock::Lock;
use std::path::Path;
use tempfile::TempDir;

const DATE: &str = "2026-03-01T00:00:00+00:00";
const USER_README: &str = "# demo\n\nMy own words, edited by the user.\n";

/// Hide what depends on the machine: the lock's source path, commit and splice version.
fn redact(snapshot: &str) -> String {
    let mut out = snapshot.to_string();
    for (re, to) in [
        (r#"source = ".*""#, r#"source = "[source]""#),
        (r#"commit = ".*""#, r#"commit = "[commit]""#),
        (r#"splice = ".*""#, r#"splice = "[splice]""#),
        (
            r"\.splice/lock\.toml [0-9a-f]{64}",
            ".splice/lock.toml [sha]",
        ),
    ] {
        out = regex::Regex::new(re)
            .unwrap()
            .replace_all(&out, to)
            .into_owned();
    }
    out
}

fn commit_all(repo: &Path) {
    git(repo, DATE, &["add", "-A"]);
    git(repo, DATE, &["commit", "-q", "-m", "state"]);
}

/// `splice new --ref v<version>` of the app piece into a fresh, committed repo.
fn new_at(tpl: &TempDir, version: &str) -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    splice_cmd()
        .args([
            "new",
            tpl.path().to_str().unwrap(),
            repo.path().to_str().unwrap(),
        ])
        .args(["--ref", &format!("v{version}")])
        .args([
            "--piece",
            "app:web",
            "--data",
            "project=demo",
            "--yes",
            "--json",
        ])
        .assert()
        .success();
    commit_all(repo.path());
    repo
}

fn edit(repo: &TempDir, rel: &str, f: impl FnOnce(String) -> String) {
    let path = repo.path().join(rel);
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(path, f(text)).unwrap();
}

/// The "User edit" column of the plan's fixture table (never the owned tsconfig: owned drift
/// refuses the update, covered in cli_update).
fn apply_user_edits(repo: &TempDir) {
    edit(repo, "package.json", |t| {
        t.replace(
            "  \"private\": true,\n",
            "  \"private\": true,\n  \"dependencies\": {\n    \"left-pad\": \"1.3.0\"\n  },\n",
        )
    });
    edit(repo, "config/settings.toml", |t| {
        format!("# local note\n{t}\n[local]\nx = 1\n")
    });
    edit(repo, ".github/workflows/ci.yml", |t| {
        t.replace("ubuntu-latest", "macos-latest")
    });
    edit(repo, "notes.txt", |t| {
        t.replace("line five", "line 5, mine")
    });
    std::fs::write(repo.path().join("README.md"), USER_README).unwrap();
    commit_all(repo.path());
}

fn update_cmd(repo: &TempDir) -> assert_cmd::Command {
    let mut cmd = splice_cmd();
    cmd.current_dir(repo.path())
        .args(["update", "--yes", "--json"]);
    cmd
}

fn read(repo: &TempDir, rel: &str) -> String {
    std::fs::read_to_string(repo.path().join(rel)).unwrap()
}

#[test]
fn golden_v1_to_v2_with_user_edits() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "1.0.0");
    apply_user_edits(&repo);
    let out = update_cmd(&repo)
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    insta::assert_json_snapshot!(serde_json::from_slice::<serde_json::Value>(&out).unwrap());
    insta::assert_snapshot!(redact(&tree_snapshot(repo.path())));
    assert!(repo.path().join(".splice/migrated.txt").exists());
    assert!(
        !repo.path().join("tool.config.mts").exists()
            && repo.path().join("tool.config.ts").exists()
    );
    assert_eq!(read(&repo, "README.md"), USER_README); // seed untouched
}

#[test]
fn rerun_after_restore_succeeds() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "1.0.0");
    apply_user_edits(&repo);
    let lock_v1 = std::fs::read(repo.path().join(".splice/lock.toml")).unwrap();

    // The v2 migration step fails: exit 3, lock still describes v1, files were written, the
    // restore hint is on stderr and the step's marker file is absent.
    let before = tree_snapshot(repo.path());
    let failed = splice_cmd()
        .current_dir(repo.path())
        .args(["update", "--yes"])
        .env("SPLICE_FIXTURE_FAIL", "1")
        .assert()
        .code(3);
    let stderr = String::from_utf8_lossy(&failed.get_output().stderr).into_owned();
    assert!(
        stderr.contains("git checkout -- . && git clean -fd"),
        "{stderr}"
    );
    assert_ne!(tree_snapshot(repo.path()), before);
    assert!(!repo.path().join(".splice/migrated.txt").exists());
    assert_eq!(
        std::fs::read(repo.path().join(".splice/lock.toml")).unwrap(),
        lock_v1
    );

    // Restore as the error message tells, then update again without the failure.
    let restore = std::process::Command::new("sh")
        .current_dir(repo.path())
        .args(["-c", "git checkout -- . && git clean -fd"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(restore.status.success());
    update_cmd(&repo)
        .env_remove("SPLICE_FIXTURE_FAIL")
        .assert()
        .code(1); // the yaml conflict
    let lock = Lock::read(repo.path()).unwrap().unwrap();
    assert_eq!(lock.template.version.to_string(), "2.0.0");

    // The re-run lands exactly where an update that never failed lands.
    let reference = new_at(&tpl, "1.0.0");
    apply_user_edits(&reference);
    update_cmd(&reference).assert().code(1);
    assert_eq!(
        redact(&tree_snapshot(repo.path())),
        redact(&tree_snapshot(reference.path()))
    );
    assert!(read(&repo, ".github/workflows/ci.yml").contains("<<<<<<< ours"));
}

#[test]
fn resolve_conflict_then_check_passes() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "1.0.0");
    apply_user_edits(&repo);
    update_cmd(&repo).assert().code(1);
    // Resolve the conflict by keeping the template side, then commit.
    let ci = read(&repo, ".github/workflows/ci.yml");
    assert!(ci.contains("<<<<<<< ours") && ci.contains(">>>>>>> template"));
    std::fs::write(
        repo.path().join(".github/workflows/ci.yml"),
        "name: ci\non: [push]\njobs:\n  test:\n    runs-on: ubuntu-24.04\n    steps:\n      - run: echo demo\n",
    )
    .unwrap();
    commit_all(repo.path());
    splice_cmd()
        .current_dir(repo.path())
        .args(["check", "--offline"])
        .assert()
        .code(0);
}

/// Shared-file edits that merge cleanly with v2 (no conflict, so `[[post]]` runs).
fn apply_clean_edits(repo: &TempDir) {
    edit(repo, "package.json", |t| {
        t.replace(
            "  \"private\": true,\n",
            "  \"private\": true,\n  \"dependencies\": {\n    \"left-pad\": \"1.3.0\"\n  },\n",
        )
    });
    edit(repo, "config/settings.toml", |t| {
        format!("# local note\n{t}\n[local]\nx = 1\n")
    });
    edit(repo, "notes.txt", |t| {
        t.replace("line five", "line 5, mine")
    });
    std::fs::write(repo.path().join("README.md"), USER_README).unwrap();
    commit_all(repo.path());
}

#[test]
fn rerun_after_failed_post_without_restore_converges() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "1.0.0");
    apply_clean_edits(&repo);
    let lock_v1 = std::fs::read(repo.path().join(".splice/lock.toml")).unwrap();

    // `echo post> .post-ran` fails while `.post-ran` is a directory (empty: git ignores it).
    std::fs::remove_file(repo.path().join(".post-ran")).unwrap();
    commit_all(repo.path());
    std::fs::create_dir(repo.path().join(".post-ran")).unwrap();
    let out = update_cmd(&repo)
        .assert()
        .code(3)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["ok"], false);
    let changes = v["changes"].as_array().unwrap();
    assert!(
        changes.iter().any(|c| c["path"] == "package.json"),
        "the applied changes are listed: {v}"
    );
    assert!(v.to_string().contains("post command"), "{v}");
    assert_eq!(
        std::fs::read(repo.path().join(".splice/lock.toml")).unwrap(),
        lock_v1
    );

    // Fix the cause, re-run on the dirty tree without restoring.
    std::fs::remove_dir(repo.path().join(".post-ran")).unwrap();
    update_cmd(&repo).arg("--allow-dirty").assert().success();
    let lock = Lock::read(repo.path()).unwrap().unwrap();
    assert_eq!(lock.template.version.to_string(), "2.0.0");

    // Same result as an update that never failed: no lost edits, no markers.
    let reference = new_at(&tpl, "1.0.0");
    apply_clean_edits(&reference);
    std::fs::remove_file(reference.path().join(".post-ran")).unwrap();
    commit_all(reference.path());
    update_cmd(&reference).assert().success();
    let snap = redact(&tree_snapshot(repo.path()));
    assert_eq!(snap, redact(&tree_snapshot(reference.path())));
    assert!(!snap.contains("<<<<<<<"), "{snap}");
    assert!(read(&repo, "package.json").contains("left-pad"));
    assert!(read(&repo, "notes.txt").contains("line 5, mine"));
    assert_eq!(read(&repo, "README.md"), USER_README);
}
