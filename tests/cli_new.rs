//! `splice new` end to end against the fixture template.

mod common;

use common::{amend_tip, commit_tag, splice_cmd, template_repo, tree_snapshot};
use std::path::Path;
use std::time::Duration;
use tempfile::TempDir;

fn s(d: &TempDir) -> &str {
    d.path().to_str().unwrap()
}

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

fn json(out: Vec<u8>) -> serde_json::Value {
    serde_json::from_slice(&out).expect("stdout is exactly one JSON object")
}

/// Replace the base piece's `[[post]]` command and republish tag `v1.0.0`.
fn with_post(tpl: &TempDir, run: &str) {
    let path = tpl.path().join("pieces/base/piece.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    let text = text.replace("echo post> .post-ran", run);
    assert!(text.contains(run));
    std::fs::write(path, text).unwrap();
    amend_tip(tpl.path(), "v1.0.0");
}

fn new_ok(tpl: &TempDir, dest: &Path, extra: &[&str]) -> serde_json::Value {
    let out = splice_cmd()
        .args(["new", s(tpl), dest.to_str().unwrap()])
        .args(["--data", "project=demo", "--yes", "--json"])
        .args(extra)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    json(out)
}

#[test]
fn new_renders_fixture_and_writes_lock() {
    let tpl = template_repo(&["v1"]);
    let dest = tempfile::tempdir().unwrap();
    let r = new_ok(&tpl, dest.path(), &["--piece", "app:web"]);
    assert_eq!(r["ok"], true);
    assert_eq!(r["command"], "new");
    assert!(dest.path().join(".splice/lock.toml").exists());
    assert!(dest.path().join(".git").exists()); // git init
    assert!(dest.path().join(".post-ran").exists()); // post ran
    let lock = std::fs::read_to_string(dest.path().join(".splice/lock.toml")).unwrap();
    assert!(lock.contains("name = \"web\""), "{lock}");
    // The instance name feeds the `target` path.
    assert!(dest.path().join("apps/web/package.json").exists());
    insta::assert_snapshot!(redact(&tree_snapshot(dest.path())));
    insta::assert_json_snapshot!(r);
}

#[test]
fn piece_name_sets_the_name_var() {
    let tpl = template_repo(&["v1"]);
    let dest = tempfile::tempdir().unwrap();
    new_ok(&tpl, dest.path(), &["--piece", "app:admin"]);
    assert!(dest.path().join("apps/admin/package.json").exists());
    assert!(!dest.path().join("apps/web").exists());
}

#[test]
fn missing_var_noninteractive_is_usage_json() {
    // No --data project=…, stdin is not a TTY: exit 2, one JSON object, no prompt, no hang.
    let tpl = template_repo(&["v1"]);
    let dest = tempfile::tempdir().unwrap();
    let out = splice_cmd()
        .args(["new", s(&tpl), s(&dest), "--json"])
        .timeout(Duration::from_secs(20))
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let r = json(out);
    assert_eq!(r["ok"], false);
    let m = r["messages"][0].as_str().unwrap();
    assert!(m.contains("`project`"), "{m}");
    assert!(m.contains("--data project="), "{m}");
    assert_eq!(std::fs::read_dir(dest.path()).unwrap().count(), 0);
}

#[test]
fn nonempty_dest_refused_without_force() {
    let tpl = template_repo(&["v1"]);
    let dest = tempfile::tempdir().unwrap();
    std::fs::write(dest.path().join("README.md"), "mine\n").unwrap();
    std::fs::write(dest.path().join(".editorconfig"), "old\n").unwrap();
    splice_cmd()
        .args(["new", s(&tpl), s(&dest), "--data", "project=demo", "--yes"])
        .assert()
        .code(2);
    assert!(!dest.path().join(".splice").exists());
    assert_eq!(
        std::fs::read(dest.path().join(".editorconfig")).unwrap(),
        b"old\n"
    );

    let r = new_ok(&tpl, dest.path(), &["--force"]);
    // The seed survives, the owned file is overwritten.
    assert_eq!(
        std::fs::read(dest.path().join("README.md")).unwrap(),
        b"mine\n"
    );
    assert_ne!(
        std::fs::read(dest.path().join(".editorconfig")).unwrap(),
        b"old\n"
    );
    let changes = r["changes"].as_array().unwrap();
    let find = |p: &str| changes.iter().find(|c| c["path"] == p).unwrap().clone();
    assert_eq!(find("README.md")["action"], "skip");
    assert_eq!(find(".editorconfig")["action"], "update");
    assert_eq!(find("package.json")["action"], "create");
}

#[test]
fn existing_lock_is_refused_even_with_force() {
    let tpl = template_repo(&["v1"]);
    let dest = tempfile::tempdir().unwrap();
    new_ok(&tpl, dest.path(), &[]);
    splice_cmd()
        .args([
            "new",
            s(&tpl),
            s(&dest),
            "--data",
            "project=demo",
            "--yes",
            "--force",
        ])
        .assert()
        .code(2);
}

#[test]
fn requires_checked() {
    let tpl = template_repo(&["v1"]);
    let dest = tempfile::tempdir().unwrap();
    let out = splice_cmd()
        .args([
            "new",
            s(&tpl),
            s(&dest),
            "--piece",
            "lib",
            "--data",
            "project=demo",
        ])
        .args(["--yes", "--json"])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let r = json(out);
    assert!(r["messages"][0].as_str().unwrap().contains("`app`"));
    assert_eq!(std::fs::read_dir(dest.path()).unwrap().count(), 0);
}

#[test]
fn piece_selection_errors_are_usage() {
    let tpl = template_repo(&["v1"]);
    for pieces in [
        vec!["nope"],           // unknown piece
        vec!["app", "app"],     // same default name twice
        vec!["app:a", "app:a"], // duplicate instance name
        vec!["base"],           // base is always included
        vec!["lib:x", "app"],   // lib is not `multiple`
    ] {
        let dest = tempfile::tempdir().unwrap();
        let mut cmd = splice_cmd();
        cmd.args(["new", s(&tpl), s(&dest), "--data", "project=demo", "--yes"]);
        for p in &pieces {
            cmd.args(["--piece", p]);
        }
        cmd.assert().code(2);
        assert_eq!(
            std::fs::read_dir(dest.path()).unwrap().count(),
            0,
            "{pieces:?}"
        );
    }
}

#[test]
fn bad_pattern_data_is_usage() {
    let tpl = template_repo(&["v1"]);
    let dest = tempfile::tempdir().unwrap();
    splice_cmd()
        .args([
            "new",
            s(&tpl),
            s(&dest),
            "--data",
            "project=demo",
            "--data",
            "scope=Bad!",
        ])
        .args(["--yes"])
        .assert()
        .code(2);
    // A piece var is checked too, including a name given through --piece.
    splice_cmd()
        .args([
            "new",
            s(&tpl),
            s(&dest),
            "--data",
            "project=demo",
            "--piece",
            "app:Bad!",
        ])
        .args(["--yes"])
        .assert()
        .code(2);
    // Typos in --data keys are not silently ignored.
    splice_cmd()
        .args([
            "new",
            s(&tpl),
            s(&dest),
            "--data",
            "project=demo",
            "--data",
            "scpoe=x",
        ])
        .args(["--yes"])
        .assert()
        .code(2);
    assert_eq!(std::fs::read_dir(dest.path()).unwrap().count(), 0);
}

#[test]
fn no_post_flag() {
    let tpl = template_repo(&["v1"]);
    let dest = tempfile::tempdir().unwrap();
    new_ok(&tpl, dest.path(), &["--no-post"]);
    assert!(!dest.path().join(".post-ran").exists());
    assert!(dest.path().join(".splice/lock.toml").exists());
}

#[test]
fn min_splice_refused() {
    let tpl = template_repo(&["v1"]);
    let path = tpl.path().join("splice.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        text.replace("min_splice = \"0.1.0\"", "min_splice = \"99.0.0\""),
    )
    .unwrap();
    commit_tag(tpl.path(), "v3", "v3.0.0");
    let dest = tempfile::tempdir().unwrap();
    let out = splice_cmd()
        .args([
            "new",
            s(&tpl),
            s(&dest),
            "--ref",
            "v3.0.0",
            "--data",
            "project=demo",
        ])
        .args(["--yes", "--json"])
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
fn lock_is_written_after_post() {
    // The post command runs before the lock exists, so a failing post leaves no lock.
    let tpl = template_repo(&["v1"]);
    with_post(&tpl, "test ! -e .splice/lock.toml && echo ok> .post-ran");
    let dest = tempfile::tempdir().unwrap();
    new_ok(&tpl, dest.path(), &[]);
    assert!(
        dest.path().join(".post-ran").exists(),
        "post saw a lock or did not run"
    );
    assert!(dest.path().join(".splice/lock.toml").exists());
}

#[test]
fn failing_post_is_env_error_without_lock() {
    let tpl = template_repo(&["v1"]);
    with_post(&tpl, "echo boom && exit 7");
    let dest = tempfile::tempdir().unwrap();
    let out = splice_cmd()
        .args([
            "new",
            s(&tpl),
            s(&dest),
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
    // Post output goes to stderr: stdout is still exactly one object.
    let r = json(out);
    let m = r["messages"][0].as_str().unwrap();
    assert!(m.contains("exit 7") && m.contains("echo boom"), "{m}");
    assert!(!dest.path().join(".splice/lock.toml").exists());
    assert!(dest.path().join("package.json").exists());
}
