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

fn code2(args: &[&str]) {
    splice_cmd().args(args).assert().code(2);
}

#[test]
fn ref_and_track_conflict() {
    code2(&[
        "--json", "channel", "add", "x", "--ref", "v1", "--track", "main",
    ]);
}

#[test]
fn channel_and_all_conflict() {
    code2(&["update", "--channel", "a", "--all"]);
}

#[test]
fn filter_flags_need_channel() {
    code2(&["update", "--only", "a/**"]);
    code2(&["update", "--exclude", "a/**"]);
}

#[test]
fn all_rejects_to() {
    code2(&["update", "--all", "--to", "2.0.0"]);
}

#[test]
fn all_rejects_data_only_exclude() {
    for f in [["--data", "a=b"], ["--only", "a"], ["--exclude", "a"]] {
        splice_cmd()
            .args(["update", "--all"])
            .args(f)
            .assert()
            .code(2);
    }
}

#[test]
fn update_to_alone_is_valid() {
    // B3: `--to` needs no `--channel`; the failure is the missing lock, not clap.
    let dir = tempfile::tempdir().unwrap();
    let out = splice_cmd()
        .arg("-C")
        .arg(dir.path())
        .args(["--json", "update", "--allow-dirty", "--to", "1.0.0"])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let msg = v["messages"].to_string();
    assert!(msg.contains(".splice/lock.toml"), "{msg}");
    assert!(!msg.contains("--channel"), "{msg}");
}

#[test]
fn clap_error_json_names_channel_command() {
    for (args, name) in [
        (vec!["--json", "channel", "add"], "channel add"),
        (vec!["--json", "channel", "remove"], "channel remove"),
    ] {
        let out = splice_cmd()
            .args(args)
            .assert()
            .code(2)
            .get_output()
            .stdout
            .clone();
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["command"], name);
    }
}

#[test]
fn help_lists_channel() {
    let out = splice_cmd()
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(String::from_utf8_lossy(&out).contains("channel"));
}

#[test]
fn channel_commands_are_stubs_until_implemented() {
    for args in [
        vec!["channel", "add", "src"],
        vec!["channel", "remove", "rig"],
    ] {
        let out = splice_cmd()
            .arg("--json")
            .args(args)
            .assert()
            .code(2)
            .get_output()
            .stdout
            .clone();
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert!(
            v["messages"].to_string().contains("not implemented yet"),
            "{v}"
        );
    }
}
