mod common;
use common::splice_cmd;

#[test]
fn bad_flag_with_json_is_one_json_object_exit_2() {
    let out = splice_cmd()
        .args(["new", "--json", "--bogus"])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["ok"], false);
    assert_eq!(v["command"], "new");
}

fn json_of(args: &[&str]) -> serde_json::Value {
    let out = splice_cmd()
        .args(args)
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&out).unwrap()
}

#[test]
fn json_before_subcommand() {
    let v = json_of(&["--json", "new", "--bogus"]);
    assert_eq!(v["ok"], false);
    assert_eq!(v["command"], "new");
}

#[test]
fn dir_flag_before_subcommand() {
    let v = json_of(&["-C", "/tmp", "--json", "new", "--bogus"]);
    assert_eq!(v["command"], "new");
}

#[test]
fn dir_value_named_like_subcommand_is_skipped() {
    let v = json_of(&["-C", "add", "--json", "new", "--bogus"]);
    assert_eq!(v["command"], "new");
}

#[test]
fn json_equals_true_is_accepted() {
    let v = json_of(&["new", "--json=true", "--bogus"]);
    assert_eq!(v["command"], "new");
}

#[test]
fn json_help_and_version_exit_0() {
    splice_cmd().args(["--json", "--help"]).assert().code(0);
    splice_cmd().args(["--json", "--version"]).assert().code(0);
}

#[test]
fn human_error_stdout_is_only_summary_and_message_once_on_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let o = splice_cmd()
        .args(["check", "--offline", "-C", dir.path().to_str().unwrap()])
        .assert()
        .code(2)
        .get_output()
        .clone();
    let out = String::from_utf8(o.stdout).unwrap();
    let err = String::from_utf8(o.stderr).unwrap();
    assert_eq!(out.lines().count(), 1, "stdout: {out:?}");
    assert!(!out.contains("has no .splice/lock.toml"));
    assert_eq!(
        err.matches("has no .splice/lock.toml").count(),
        1,
        "stderr: {err:?}"
    );
}
